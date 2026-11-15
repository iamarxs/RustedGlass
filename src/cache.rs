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

/// The cached copy of a still, refreshed to the front of the cache, if it is in there.
pub fn cached(path: &Path) -> Option<Bitmap> {
    let key = key(path)?;
    STILLS.with_borrow_mut(|cache| take_hit(cache, &key))
}

/// Stores a still that was just decoded, so the next visit needs no decode.
pub fn store(path: &Path, decoded: decode::Decoded) -> Bitmap {
    let pixels = Bitmap::from_decoded(decoded);
    if let Some(key) = key(path) {
        STILLS.with_borrow_mut(|cache| insert(cache, key, pixels.clone_pixels()));
    }
    pixels
}

/// The cached copy of `key`, moved to the front of the cache.
fn take_hit(cache: &mut VecDeque<(Key, Bitmap)>, key: &Key) -> Option<Bitmap> {
    let index = cache.iter().position(|(saved, _)| saved == key)?;
    let hit = cache.remove(index)?;
    let pixels = hit.1.clone_pixels();
    cache.push_front(hit);
    Some(pixels)
}

/// A new still at the front of the cache, evicting from the back until the budget holds.
fn insert(cache: &mut VecDeque<(Key, Bitmap)>, key: Key, pixels: Bitmap) {
    if pixels.pixel_bytes() > BUDGET {
        return;
    }
    cache.push_front((key, pixels));
    let mut total: usize = cache.iter().map(|(_, image)| image.pixel_bytes()).sum();
    while total > BUDGET {
        if let Some((_, evicted)) = cache.pop_back() {
            total -= evicted.pixel_bytes();
        }
    }
}
