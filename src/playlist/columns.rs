//! Native column layout, cell text, and ordering for the playlist.

use std::cell::{Cell, RefCell};

use windows::core::PWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Controls::{
    HDF_SORTDOWN, HDF_SORTUP, HDITEMW, HDI_FORMAT, HDM_GETITEMCOUNT, HDM_GETITEMW, HDM_SETITEMW,
    LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW, LVM_DELETECOLUMN, LVM_GETCOLUMNWIDTH, LVM_GETHEADER,
    LVM_INSERTCOLUMNW,
};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, PostMessageW, SendMessageW, GA_ROOT, WM_RBUTTONUP,
};

use crate::photo::Column;
use crate::{state, w32};

mod details;
mod header;

pub(crate) fn reset_metadata() {
    details::reset();
}
pub(crate) use details::{metadata_ready, seed_metadata};
pub(crate) use header::header_notify;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Sort {
    revision: u64,
    column: Column,
    descending: bool,
    pending: bool,
}

fn slot(column: Column) -> usize {
    Column::ALL.iter().position(|c| *c == column).unwrap_or(0)
}

fn default_width(column: Column) -> i32 {
    match column {
        Column::Name => 240,
        Column::FullPath => 340,
        Column::Width | Column::Height => 64,
        Column::Type => 80,
        Column::Size => 96,
        Column::Date => 140,
    }
}

thread_local! {
    static SHOWN: Cell<[bool; 7]> = const { Cell::new([true; 7]) };
    static WIDTHS: RefCell<[i32; 7]> = RefCell::new(Column::ALL.map(default_width));
    static SORT: Cell<Option<Sort>> = const { Cell::new(None) };
}

pub(crate) fn column_visible(column: Column) -> bool {
    SHOWN.get()[slot(column)]
}

pub(crate) fn toggle_column(column: Column) {
    let mut shown = SHOWN.get();
    let at = slot(column);
    if shown[at] && shown.iter().filter(|&&visible| visible).count() == 1 {
        return;
    }
    remember_widths();
    shown[at] = !shown[at];
    SHOWN.set(shown);
    rebuild_columns();
}

fn visible_columns() -> impl Iterator<Item = Column> {
    Column::ALL
        .into_iter()
        .zip(SHOWN.get())
        .filter_map(|(column, shown)| shown.then_some(column))
}

/// True while a shown column's cell comes from the image header: those columns make
/// every batch pay for a header read, the stat-only ones do not.
pub(super) fn header_columns_shown() -> bool {
    visible_columns().any(|column| matches!(column, Column::Width | Column::Height | Column::Type))
}

pub(super) fn cell_text(row: usize, subitem: usize) -> String {
    let Some(column) = visible_columns().nth(subitem) else {
        return String::new();
    };
    super::with_row(row, |path| {
        details::with_entries(|entries| column.text(path, entries.get(path)))
    })
    .unwrap_or_default()
}

pub fn sort_column(subitem: i32) {
    if state::read_state(|state| state.browsing_import_preview()) == Some(true) {
        return;
    }
    let Some(column) = usize::try_from(subitem)
        .ok()
        .and_then(|index| visible_columns().nth(index))
    else {
        return;
    };
    state::with_state(|state| {
        if state.photos.is_empty() {
            return;
        }
        let revision = state.photos.rows_token();
        let previous = SORT.get().filter(|sort| sort.revision == revision);
        let descending =
            next_direction(previous.map(|sort| (sort.column, sort.descending)), column);
        SORT.set(Some(Sort {
            revision,
            column,
            descending,
            pending: true,
        }));
    });
}

pub(super) fn sort_pending() -> bool {
    SORT.get()
        .is_some_and(|sort| sort.pending && sort.column.needs_metadata())
}

fn next_direction(previous: Option<(Column, bool)>, column: Column) -> bool {
    match previous {
        Some((by, descending)) if by == column => !descending,
        _ => true,
    }
}

/// Apply the latest header click once the matching metadata is ready, then queue the next batch.
pub fn request_metadata() -> bool {
    let (arrow_changed, sorted) = state::with_state(|state| {
        let revision = state.photos.rows_token();
        let before = SORT.get();
        let sort = before.filter(|sort| {
            sort.revision == revision && (!sort.pending || column_visible(sort.column))
        });
        SORT.set(sort);
        details::sync(&state.photos);
        if !state.playlist_visible {
            details::end_sort();
            details::queue(state);
            return (before != sort, false);
        }
        // A pending metadata-column sort gathers every row in bounded batches first; its
        // temporary per-row map is released as soon as the sort has run.
        let meta_sort = sort.filter(|sort| sort.pending && sort.column.needs_metadata());
        let new_sort =
            details::want_sort(meta_sort.map(|sort| sort.column), revision, &state.photos);
        let mut sorted = false;
        if let Some(sort) = sort.filter(|sort| sort.pending) {
            let applied = if !sort.column.needs_metadata() || details::sort_ready() {
                details::with_sort_entries(|entries| {
                    state
                        .photos
                        .sort_by_column(sort.column, sort.descending, entries);
                });
                true
            } else {
                new_sort && details::sort_if_cached(&mut state.photos, sort.column, sort.descending)
            };
            if applied {
                sorted = true;
                details::end_sort();
                SORT.set(Some(Sort {
                    revision: state.photos.rows_token(),
                    pending: false,
                    ..sort
                }));
                details::sync(&state.photos);
            }
        }
        details::queue(state);
        (before != SORT.get(), sorted)
    })
    .unwrap_or_default();
    if arrow_changed {
        update_arrow();
    }
    sorted
}

/// The columns of a freshly created list, in `Column::ALL` order.
pub(super) fn install(list: HWND) {
    rebuild_columns();
    unsafe {
        let header = HWND(SendMessageW(list, LVM_GETHEADER, None, None).0 as *mut _);
        if !header.0.is_null() {
            // The header's right-click opens the playlist menu, as a row's does.
            let _ = SetWindowSubclass(header, Some(header_proc), 1, 0);
        }
    }
}

/// Reads the native widths of the shown columns, so a hidden one comes back as it was.
fn remember_widths() {
    let list = super::list();
    if list == super::NO_LIST {
        return;
    }
    for (index, column) in visible_columns().enumerate() {
        let width =
            unsafe { SendMessageW(list, LVM_GETCOLUMNWIDTH, Some(WPARAM(index)), None) }.0 as i32;
        if width > 0 {
            WIDTHS.with(|widths| widths.borrow_mut()[slot(column)] = width);
        }
    }
}

/// Rebuilds the list's columns from the shown flags, keeping their native widths.
fn rebuild_columns() {
    let list = super::list();
    if list == super::NO_LIST {
        return;
    }
    let shown = visible_columns();
    unsafe {
        let header = HWND(SendMessageW(list, LVM_GETHEADER, None, None).0 as *mut _);
        let mut count = if header.0.is_null() {
            0
        } else {
            SendMessageW(header, HDM_GETITEMCOUNT, None, None).0.max(0)
        };
        while count > 0 {
            let _ = SendMessageW(list, LVM_DELETECOLUMN, Some(WPARAM(0)), None);
            count -= 1;
        }
        for (index, column) in shown.enumerate() {
            insert_column(list, index, column);
        }
        update_arrow();
        w32::invalidate(list);
    }
}

unsafe fn insert_column(list: HWND, index: usize, column: Column) {
    let mut title = w32::wide(column.title());
    let width = WIDTHS.with(|widths| widths.borrow()[slot(column)]);
    let item = LVCOLUMNW {
        mask: LVCF_TEXT | LVCF_WIDTH,
        cx: width,
        pszText: PWSTR(title.as_mut_ptr()),
        cchTextMax: title.len() as i32,
        ..Default::default()
    };
    let _ = unsafe {
        SendMessageW(
            list,
            LVM_INSERTCOLUMNW,
            Some(WPARAM(index)),
            Some(LPARAM(&item as *const _ as isize)),
        )
    };
}

/// The native sort arrow of the header: on the column of the last sort, up or down.
fn update_arrow() {
    let list = super::list();
    if list == super::NO_LIST {
        return;
    }
    unsafe {
        let header = HWND(SendMessageW(list, LVM_GETHEADER, None, None).0 as *mut _);
        if header.0.is_null() {
            return;
        }
        let count = SendMessageW(header, HDM_GETITEMCOUNT, None, None).0.max(0) as usize;
        let arrow = SORT.get().and_then(|sort| {
            visible_columns()
                .position(|column| column == sort.column)
                .filter(|&index| index < count)
                .map(|index| (index, sort.descending))
        });
        for index in 0..count {
            let mut item = HDITEMW {
                mask: HDI_FORMAT,
                ..Default::default()
            };
            if SendMessageW(
                header,
                HDM_GETITEMW,
                Some(WPARAM(index)),
                Some(LPARAM(&mut item as *mut _ as isize)),
            )
            .0 == 0
            {
                continue;
            }
            let mut fmt = item.fmt.0 & !(HDF_SORTUP.0 | HDF_SORTDOWN.0);
            if let Some((at, down)) = arrow {
                if at == index {
                    fmt |= if down { HDF_SORTDOWN.0 } else { HDF_SORTUP.0 };
                }
            }
            if item.fmt.0 == fmt {
                continue;
            }
            item.fmt.0 = fmt;
            let _ = SendMessageW(
                header,
                HDM_SETITEMW,
                Some(WPARAM(index)),
                Some(LPARAM(&item as *const _ as isize)),
            );
        }
    }
}

unsafe extern "system" fn header_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if message == WM_RBUTTONUP {
        unsafe {
            let root = GetAncestor(hwnd, GA_ROOT);
            let _ = PostMessageW(Some(root), WM_RBUTTONUP, wparam, lparam);
        }
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn any_column_hides_but_the_last_one_stays() {
        assert!(Column::ALL.iter().all(|column| column_visible(*column)));
        for column in Column::ALL.iter().skip(1) {
            toggle_column(*column);
        }
        toggle_column(Column::Name);
        assert!(!column_visible(Column::Date));
        assert!(column_visible(Column::Name), "the last column cannot hide");
    }

    #[test]
    fn first_click_descends_and_repeats_toggle() {
        assert!(next_direction(None, Column::Date));
        assert!(!next_direction(Some((Column::Date, true)), Column::Date));
        assert!(next_direction(Some((Column::Date, true)), Column::Name));
    }
}
