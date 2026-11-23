use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::{decode, render::Bitmap};

/// Pixels plus the GDI surface of every drawn entry. One HDC and one HBITMAP per drawn entry,
/// so the entry cap bounds the handles as well as the memory.
const BUDGET: usize = 96 * 1024 * 1024;
const MAX_ENTRIES: usize = 64;
/// The file's length and modified time: the identity that decides whether a decoded still
/// is still this file's content.
pub type FileStat = (u64, SystemTime);

type Key = (PathBuf, u64, SystemTime);
thread_local! {
    static STILLS: RefCell<VecDeque<(Key, Bitmap)>> = const { RefCell::new(VecDeque::new()) };
}

pub fn key(path: &Path) -> Option<Key> {
    stat(path).map(|(len, modified)| (path.to_owned(), len, modified))
}

/// The file's length and modified time, in one stat.
pub fn stat(path: &Path) -> Option<FileStat> {
    let metadata = path.metadata().ok()?;
    Some((metadata.len(), metadata.modified().ok()?))
}

/// The cached copy of a still, refreshed to the front of the cache, if it is in there.
#[cfg(test)]
pub fn cached(path: &Path) -> Option<Bitmap> {
    cached_with_stat(path).map(|(_, bitmap)| bitmap)
}

/// The cached copy of a still together with the file stat that vouched for it, so a hit
/// reports the file size from the same stat that keyed the entry.
pub fn cached_with_stat(path: &Path) -> Option<(FileStat, Bitmap)> {
    let key = key(path)?;
    STILLS.with_borrow_mut(|cache| take_hit(cache, &key).map(|bitmap| ((key.1, key.2), bitmap)))
}

/// Stores a still that was just decoded, so the next visit needs no decode: the tests'
/// entry point, which vouches for the file with a fresh stat at store time.
#[cfg(test)]
pub fn store(path: &Path, decoded: decode::Decoded) -> Bitmap {
    store_checked(path, stat(path), decoded)
}

/// Stores a still decoded from `path`, so the next visit needs no decode — unless the file
/// changed while it decoded. `before` is the file's stat sampled before the decode began;
/// a changed or vanished file is left out, so its stale pixels cannot poison the next
/// visit, while the decoded image still shows.
pub fn store_checked(path: &Path, before: Option<FileStat>, decoded: decode::Decoded) -> Bitmap {
    let pixels = Bitmap::from_decoded(decoded);
    let unchanged = before.is_some_and(|before| stat(path).is_some_and(|now| now == before));
    if let (Some((len, modified)), true) = (before, unchanged) {
        STILLS.with_borrow_mut(|cache| {
            insert(
                cache,
                (path.to_owned(), len, modified),
                pixels.clone_pixels(),
            )
        });
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

/// Reserve both the pixels and their same-sized lazy GDI surface before inserting a still.
fn insert(cache: &mut VecDeque<(Key, Bitmap)>, key: Key, pixels: Bitmap) {
    if pixels.pixel_bytes() > BUDGET / 2 {
        return;
    }
    cache.push_front((key, pixels));
    let mut pixel_bytes: usize = cache.iter().map(|(_, image)| image.pixel_bytes()).sum();
    while pixel_bytes > BUDGET / 2 || cache.len() > MAX_ENTRIES {
        let Some((_, evicted)) = cache.pop_back() else {
            break;
        };
        pixel_bytes -= evicted.pixel_bytes();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny() -> Bitmap {
        Bitmap::from_decoded(tiny_decoded())
    }

    fn tiny_decoded() -> decode::Decoded {
        decode::Decoded {
            width: 1,
            height: 1,
            rgba: vec![0, 0, 0, 255],
        }
    }

    fn key(n: u64) -> Key {
        (PathBuf::from(format!("{n}.png")), n, SystemTime::UNIX_EPOCH)
    }

    #[test]
    fn the_cache_evicts_by_entry_count_as_well_as_bytes() {
        let mut cache = VecDeque::new();
        for n in 0..MAX_ENTRIES as u64 + 2 {
            insert(&mut cache, key(n), tiny());
        }
        assert_eq!(cache.len(), MAX_ENTRIES, "the handle count stays bounded");
        assert_eq!(cache.front().unwrap().0, key(MAX_ENTRIES as u64 + 1));
    }

    #[test]
    fn a_file_that_changed_while_decoding_shows_but_is_not_stored() {
        let path = crate::test_path("cache-changed-during-decode.png");
        std::fs::write(&path, b"first").unwrap();
        let before = stat(&path).unwrap();

        // The ordinary size change stands in for any mid-decode file change.
        std::fs::write(&path, b"first but rewritten longer").unwrap();
        let pixels = store_checked(&path, Some(before), tiny_decoded());
        assert_eq!(pixels.width, 1, "the decoded image still shows");
        assert!(cached(&path).is_none(), "a changed file is not cached");

        // An unchanged file is stored, an identity that is missing is not.
        let before = stat(&path).unwrap();
        store_checked(&path, Some(before), tiny_decoded());
        assert!(cached(&path).is_some());
        std::fs::remove_file(&path).unwrap();
        store_checked(&path, None, tiny_decoded());
        assert!(
            cached(&path).is_none(),
            "a stat that went missing is not cached"
        );
    }
}
