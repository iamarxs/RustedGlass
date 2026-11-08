use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::{decode, render::Bitmap};

const BUDGET: usize = 96 * 1024 * 1024;
type Key = (PathBuf, u64, SystemTime);
thread_local! {
    static STILLS: RefCell<VecDeque<(Key, Bitmap)>> = const { RefCell::new(VecDeque::new()) };
}

pub fn key(path: &Path) -> Option<Key> {
    let stat = path.metadata().ok()?;
    Some((path.to_owned(), stat.len(), stat.modified().ok()?))
}

pub fn load(path: &Path) -> Result<Bitmap, String> {
    let Some(key) = key(path) else {
        return decode::decode(path).map(Bitmap::from_decoded);
    };
    STILLS.with_borrow_mut(|cache| {
        if let Some(index) = cache.iter().position(|(saved, _)| *saved == key) {
            if let Some(hit) = cache.remove(index) {
                let pixels = hit.1.clone_pixels();
                cache.push_front(hit);
                return Ok(pixels);
            }
        }
        let pixels = Bitmap::from_decoded(decode::decode(path)?);
        if pixels.pixel_bytes() <= BUDGET {
            cache.push_front((key, pixels.clone_pixels()));
            let mut total: usize = cache.iter().map(|(_, image)| image.pixel_bytes()).sum();
            while total > BUDGET {
                if let Some((_, evicted)) = cache.pop_back() {
                    total -= evicted.pixel_bytes();
                }
            }
        }
        Ok(pixels)
    })
}
