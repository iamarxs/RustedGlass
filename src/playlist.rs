//! The playlist fills a native list view and keeps the current row marked.
//! Custom drawing and drag handling live in `listdraw`.

use std::cell::Cell;
use std::path::Path;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Controls::{
    LIST_VIEW_ITEM_STATE_FLAGS, LVIS_FOCUSED, LVIS_SELECTED, LVITEMW, LVM_ENSUREVISIBLE,
    LVM_GETNEXTITEM, LVM_GETSELECTEDCOUNT, LVM_SETBKCOLOR, LVM_SETEXTENDEDLISTVIEWSTYLE,
    LVM_SETITEMCOUNT, LVM_SETITEMSTATE, LVM_SETTEXTBKCOLOR, LVNI_FOCUSED, LVNI_SELECTED,
    LVSICF_NOINVALIDATEALL, LVSICF_NOSCROLL, LVS_EX_FULLROWSELECT, LVS_OWNERDATA, LVS_REPORT,
    LVS_SHOWSELALWAYS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, SendMessageW, SetWindowPos, ShowWindow, SWP_NOZORDER, SW_HIDE, SW_SHOWNA,
    WINDOW_STYLE, WS_CHILD, WS_CLIPCHILDREN, WS_TABSTOP, WS_VISIBLE,
};

use crate::dialog::PANEL_COLOR;
use crate::listdraw;
use crate::tools;
use crate::{state, w32};

pub use crate::listdraw::{begin_drag, custom_draw};

mod columns;
pub(crate) use columns::toggle_column;
pub(crate) use columns::{
    column_visible, header_notify, metadata_ready, request_metadata, reset_metadata, seed_metadata,
    sort_column,
};

/// The list view of a hidden playlist keeps this size; a shown one is as wide as the window.
const WIDTH: i32 = 320;
const NO_LIST: HWND = HWND(std::ptr::null_mut());

// The list is only ever used from the UI thread, and a `static` would need to be `Sync`.
thread_local! {
    static LIST: Cell<HWND> = const { Cell::new(NO_LIST) };
    static CURRENT: Cell<Option<usize>> = const { Cell::new(None) };
    static SYNCED: Cell<Option<(u64, u64)>> = const { Cell::new(None) };
}

fn list() -> HWND {
    LIST.with(|cell| cell.get())
}

fn set_list(value: HWND) {
    LIST.set(value);
}

pub fn invalidate() {
    if list() != NO_LIST {
        unsafe {
            w32::invalidate(list());
        }
    }
}

pub fn row_name(row: usize) -> Option<String> {
    with_row(row, shown_name)
}

pub(super) fn with_row<T>(row: usize, read: impl FnOnce(&Path) -> T) -> Option<T> {
    state::read_state(|state| state.photos.path_at(row).map(|path| read(path))).flatten()
}

pub fn is_visible() -> bool {
    let hwnd = list();
    hwnd != NO_LIST
        && unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd).as_bool() }
}

pub fn owns(handle: HWND) -> bool {
    handle != NO_LIST && handle == list()
}

/// The focused row, `None` when there is no list.
pub fn focused() -> Option<usize> {
    let list = list();
    if list == NO_LIST {
        return None;
    }
    unsafe {
        let row = SendMessageW(
            list,
            LVM_GETNEXTITEM,
            Some(WPARAM(usize::MAX)),
            Some(LPARAM(LVNI_FOCUSED as isize)),
        )
        .0;
        if row < 0 {
            None
        } else {
            Some(row as usize)
        }
    }
}

/// Every selected row, in list order. `Ctrl`-clicks and `Shift`-clicks make more than one.
pub fn selected() -> Vec<usize> {
    let list = list();
    if list == NO_LIST {
        return Vec::new();
    }
    unsafe {
        let mut rows = Vec::new();
        let mut after = -1isize;
        loop {
            let row = SendMessageW(
                list,
                LVM_GETNEXTITEM,
                Some(WPARAM(after as usize)),
                Some(LPARAM(LVNI_SELECTED as isize)),
            )
            .0;
            if row < 0 {
                return rows;
            }
            after = row;
            rows.push(row as usize);
        }
    }
}

/// Create the native control before refreshing its layout and visible rows.
pub fn show(parent: HWND) {
    unsafe {
        let _ = ensure_list(parent);
    }
}

pub fn hide() {
    listdraw::end_drag();
    let list = list();
    if list != NO_LIST {
        unsafe {
            let _ = ShowWindow(list, SW_HIDE);
        }
    }
}

/// Fill the right half of the window below the toolbar when the playlist is visible.
pub fn resize(width: i32, height: i32) {
    let list = list();
    if list == NO_LIST {
        return;
    }
    let shown = state::with_state(|state| {
        state.playlist_visible && state.dialog == crate::dialog::Dialog::None
    })
    .unwrap_or(false);
    if !shown {
        listdraw::end_drag();
        unsafe {
            let _ = ShowWindow(list, SW_HIDE);
        }
        return;
    }
    let top = band_top();
    unsafe {
        // The picture keeps the left half; the odd pixel goes to the list.
        let (x, list_width) = (width / 2, width - width / 2);
        // The columns keep the widths the header was last given; the rows just move.
        let _ = ShowWindow(list, SW_SHOWNA);
        let _ = SetWindowPos(
            list,
            None,
            x,
            top,
            list_width,
            (height - top).max(0),
            SWP_NOZORDER,
        );
    }
}

/// The band of the tool bar is the only thing above the list, and it is only there when the tool
/// bar is on.
fn band_top() -> i32 {
    state::with_state(|state| {
        if state.toolbar_visible {
            tools::BAND_HEIGHT
        } else {
            0
        }
    })
    .unwrap_or(0)
}

unsafe fn ensure_list(parent: HWND) -> HWND {
    if list() != NO_LIST {
        return list();
    }
    let created = create(parent);
    listdraw::install(created);
    set_list(created);
    columns::install(created);
    created
}

/// Rebuild only when the visible rows change, preserving scroll and selection on other refreshes.
pub(crate) fn refresh(anchor: Option<&ScrollAnchor>) {
    let list = list();
    if list == NO_LIST {
        return;
    }
    let old = SYNCED.get();
    let Some((rows, layout, len, current, restore)) = state::read_state(|state| {
        let photos = &state.photos;
        let layout = photos.layout_token();
        let restore = anchor
            .filter(|_| old.is_some_and(|(_, previous)| previous != layout))
            .and_then(|anchor| anchor.position(photos));
        (
            photos.rows_token(),
            layout,
            photos.len(),
            photos.current().map(|_| photos.index),
            restore,
        )
    }) else {
        return;
    };
    // Release State before the native calls, which can synchronously request row text.
    SYNCED.set(Some((rows, layout)));
    let changed = old.map(|(revision, _)| revision) != Some(rows);
    let rebuilt = old.map(|(_, old_layout)| old_layout) != Some(layout);
    if changed {
        listdraw::end_drag();
        unsafe {
            if rebuilt {
                CURRENT.set(None);
                let _ = SendMessageW(list, LVM_SETITEMCOUNT, Some(WPARAM(0)), None);
                let _ = SendMessageW(list, LVM_SETITEMCOUNT, Some(WPARAM(len)), None);
                w32::invalidate(list);
            } else {
                let flags = LVSICF_NOINVALIDATEALL | LVSICF_NOSCROLL;
                let _ = SendMessageW(
                    list,
                    LVM_SETITEMCOUNT,
                    Some(WPARAM(len)),
                    Some(LPARAM(flags as isize)),
                );
            }
        }
    }
    mark(current);
    if rebuilt {
        if let Some(row) = restore {
            control::restore_top_row(list, row);
        } else if let Some(row) = current {
            let _ = unsafe { SendMessageW(list, LVM_ENSUREVISIBLE, Some(WPARAM(row)), None) };
        }
    }
}

fn mark(current: Option<usize>) {
    if CURRENT.replace(current) == current {
        return;
    }
    let Some(row) = current else { return };
    if focused() == Some(row) {
        return;
    }
    let multiple = unsafe { SendMessageW(list(), LVM_GETSELECTEDCOUNT, None, None).0 > 1 };
    let mask = if multiple {
        LVIS_FOCUSED
    } else {
        LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0)
    };
    let mut item = LVITEMW {
        stateMask: mask,
        ..Default::default()
    };
    unsafe {
        let _ = SendMessageW(
            list(),
            LVM_SETITEMSTATE,
            Some(WPARAM(usize::MAX)),
            Some(LPARAM(&item as *const _ as isize)),
        );
        item.state = mask;
        let _ = SendMessageW(
            list(),
            LVM_SETITEMSTATE,
            Some(WPARAM(row)),
            Some(LPARAM(&item as *const _ as isize)),
        );
    }
}

mod control;
pub(crate) use control::{browse_page, rows_per_page};
pub(crate) use control::{capture_background_scroll, ScrollAnchor};
use control::{create, shown_name};

pub use control::{display_info, find_item};
