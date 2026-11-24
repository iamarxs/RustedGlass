//! Bounded background batches for the playlist's current set of paths.
//!
//! Read the visible page first, then prefetch the rest of the playlist in
//! bounded batches. The cache keeps every read detail for as long as the
//! playlist lives.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::Controls::{LVM_GETTOPINDEX, LVM_REDRAWITEMS};
use windows::Win32::UI::WindowsAndMessaging::SendMessageW;

use crate::photo::{Column, Details, FileFacts, PhotoList};

mod cache;
use cache::{header_column, Cache, BATCH};

type Display = HashMap<PathBuf, Details>;

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
}

pub(super) fn with_entries<T>(use_entries: impl FnOnce(&Display) -> T) -> T {
    CACHE.with(|cache| use_entries(cache.borrow().entries()))
}

pub(super) fn sync(photos: &PhotoList) {
    CACHE.with(|cache| cache.borrow_mut().sync(photos));
}

pub(super) fn reset() {
    CACHE.with(|cache| cache.borrow_mut().reset());
}

/// Seeds the stat fields of just-published rows: the lead's import calls this with the
/// facts its enumeration already gathered, so Size and Date answer without reads.
pub(crate) fn seed_metadata(photos: &PhotoList, entries: Vec<(PathBuf, FileFacts)>) {
    CACHE.with(|cache| cache.borrow_mut().seed(photos, entries));
}

/// Starts or drops the all-rows fetch behind a pending metadata-column sort.
pub(super) fn want_sort(column: Option<Column>, revision: u64, photos: &PhotoList) -> bool {
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let Some(column) = column else {
            cache.set_sort(None);
            return false;
        };
        let same = cache
            .sort()
            .is_some_and(|fetch| fetch.revision() == revision)
            && cache.sort_reads_header() == header_column(column);
        if !same {
            cache.set_sort(Some(cache::SortFetch::new(
                revision,
                photos.len(),
                header_column(column),
            )));
        }
        !same
    })
}

/// A fully cached column can sort directly with no second details map.
pub(super) fn sort_if_cached(photos: &mut PhotoList, column: Column, descending: bool) -> bool {
    CACHE.with(|cache| {
        let cache = cache.borrow();
        let entries = cache.entries();
        let header = header_column(column);
        if !photos.visible_rows().all(|path| {
            entries.get(path.as_path()).is_some_and(|read| {
                if header {
                    read.header_read()
                } else {
                    read.facts().is_some()
                }
            })
        }) {
            return false;
        }
        photos.sort_by_column_with(column, descending, |path| entries.get(path));
        true
    })
}

/// True when the pending metadata-column sort has every row's details.
pub(super) fn sort_ready() -> bool {
    CACHE.with(|cache| cache.borrow().sort().is_some_and(|fetch| fetch.complete()))
}

/// The entries a metadata sort runs on: the all-rows fetch of a pending one. A sort
/// without a fetch gets the empty map — its signature still wants a map, and columns
/// that need no metadata never look into it.
pub(super) fn with_sort_entries<T>(use_entries: impl FnOnce(&HashMap<PathBuf, Details>) -> T) -> T {
    CACHE.with(|cache| match cache.borrow().sort() {
        Some(fetch) => use_entries(fetch.entries()),
        None => use_entries(&HashMap::new()),
    })
}

/// Releases the sort's temporary per-row map, proportional to the row count.
pub(super) fn end_sort() {
    CACHE.with(|cache| cache.borrow_mut().set_sort(None));
}

/// The rows the native list currently shows: top index and count per page. A hidden
/// pane shows no rows at all.
fn visible_range() -> (usize, usize) {
    let list = super::super::list();
    if list == super::super::NO_LIST || !super::super::is_visible() {
        return (0, 0);
    }
    unsafe {
        let top = SendMessageW(list, LVM_GETTOPINDEX, None, None).0.max(0) as usize;
        (top, super::super::rows_per_page())
    }
}

/// Up to `BATCH` on-screen rows needing a further read (+1 for the partially visible row).
fn on_screen_batch(
    entries: &Display,
    photos: &PhotoList,
    header_columns: bool,
) -> Vec<(PathBuf, Option<FileFacts>)> {
    let (top, per_page) = visible_range();
    if per_page == 0 {
        return Vec::new();
    }
    let end = (top + per_page + 1).min(photos.len());
    (top..end)
        .filter_map(|position| photos.path_at(position))
        .filter(|path| {
            entries
                .get(path.as_path())
                .map_or(true, |read| !cache::display_complete(read, header_columns))
        })
        .take(BATCH)
        .map(|path| {
            (
                path.clone(),
                entries.get(path.as_path()).and_then(Details::facts),
            )
        })
        .collect()
}

pub(super) fn queue(state: &mut crate::state::State) {
    let selection_pending = state.prioritize_metadata_selection();
    let revision = state.photos.rows_token();
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.in_flight().is_some() || cache.revision() != Some(revision) {
            return;
        }
        // A pending metadata sort still has to cover every row, but the visible page
        // is served first even while it gathers; a hidden pane shows no rows.
        let sorting = cache
            .sort()
            .is_some_and(|fetch| fetch.revision() == revision && !fetch.complete());
        // Only a shown column makes a batch pay for a header read; stat fields
        // alone answer Size and Date.
        let header = super::header_columns_shown();
        let shown = state.playlist_visible && state.dialog == crate::dialog::Dialog::None;
        let visible = if shown {
            on_screen_batch(cache.entries(), &state.photos, header)
        } else {
            Vec::new()
        };
        let mut rewind_to = None;
        let mut prefetch_end = None;
        let offscreen = visible.is_empty();
        let (items, read_header) = if !offscreen {
            (visible, header)
        } else if sorting {
            let header = cache.sort_reads_header();
            // The explicit sort's offscreen header reads wait while the selected
            // image is loading; a stat-only sort costs no header read and continues.
            if defer_offscreen(selection_pending, header) {
                return;
            }
            rewind_to = Some(cache.sort().map(|fetch| fetch.next_row()).unwrap_or(0));
            (cache.sort_next_batch(&state.photos), header)
        } else {
            let (end, paths) = cache.prefetch(&state.photos, header);
            // Same wait for the offscreen prefetch: its pass is not advanced, so it
            // resumes where it stopped once the load is over. Stat-only work goes on.
            if defer_offscreen(selection_pending, header) && !paths.is_empty() {
                return;
            }
            prefetch_end = Some(end);
            (paths, header)
        };
        if items.is_empty() {
            return;
        }
        if state.request_metadata(revision, items, read_header, offscreen) {
            if let Some(end) = prefetch_end {
                cache.set_prefetch_in_flight(revision, end);
            } else {
                cache.set_in_flight(revision);
            }
        } else if let (Some(fetch), Some(next)) = (cache.sort_mut(), rewind_to) {
            if fetch.revision() == revision {
                fetch.rewind(next);
            }
        }
    });
}

pub(crate) fn metadata_ready(
    revision: u64,
    photos: &mut PhotoList,
    entries: Vec<(PathBuf, Details)>,
    complete: bool,
) -> bool {
    let changed = if revision == photos.rows_token() && super::super::is_visible() {
        let (top, per_page) = visible_range();
        let mut affected = (top..(top + per_page + 1).min(photos.len())).filter(|&row| {
            photos
                .path_at(row)
                .is_some_and(|path| entries.iter().any(|(updated, _)| updated == path))
        });
        affected
            .next()
            .map(|first| (first, affected.last().unwrap_or(first)))
    } else {
        None
    };
    let (delivered, sort_finished) = CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let delivered = cache.record(revision, photos.rows_token(), entries, complete);
        (
            delivered,
            cache.sort().is_some_and(|fetch| fetch.complete()),
        )
    });
    if let Some((first, last)) = changed.filter(|_| delivered) {
        let _ = unsafe {
            SendMessageW(
                super::super::list(),
                LVM_REDRAWITEMS,
                Some(WPARAM(first)),
                Some(LPARAM(last as isize)),
            )
        };
    }
    // The playlist paints its own rows. Only a sort needs a viewer refresh.
    delivered && sort_finished
}

/// While the selected image is still loading, an offscreen header read waits for it:
/// both the sort's remaining rows and the prefetch pass are offscreen, while the
/// visible page is served first regardless. Stat-only batches are not deferred.
fn defer_offscreen(load_pending: bool, read_header: bool) -> bool {
    load_pending && read_header
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list_of(count: usize) -> PhotoList {
        let paths: Vec<PathBuf> = (0..count)
            .map(|i| PathBuf::from(format!("C:\\photos\\img{i:04}.png")))
            .collect();
        PhotoList::from_files(&paths, vec![".png".to_string()])
    }

    #[test]
    fn a_hidden_pane_shows_no_rows() {
        // No list window exists in a test thread, the way the pane is hidden.
        let photos = list_of(8);
        assert!(on_screen_batch(&Display::new(), &photos, true).is_empty());
        assert!(on_screen_batch(&Display::new(), &photos, false).is_empty());
    }

    #[test]
    fn offscreen_header_reads_wait_for_the_load_but_stat_only_work_goes_on() {
        assert!(defer_offscreen(true, true));
        assert!(!defer_offscreen(true, false), "stat-only is not deferred");
        assert!(
            !defer_offscreen(false, true),
            "nothing waits once the load is over"
        );
        assert!(!defer_offscreen(false, false));
    }
}
