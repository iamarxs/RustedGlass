//! The playlist fills a native list view and keeps the current row marked.
//! Custom drawing and drag handling live in `listdraw`.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Controls::{
    LIST_VIEW_ITEM_STATE_FLAGS, LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW, LVIS_FOCUSED, LVIS_SELECTED,
    LVITEMW, LVM_GETNEXTITEM, LVM_GETSELECTEDCOUNT, LVM_INSERTCOLUMNW, LVM_SETBKCOLOR,
    LVM_SETCOLUMNW, LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETITEMCOUNT, LVM_SETITEMSTATE,
    LVM_SETTEXTBKCOLOR, LVNI_FOCUSED, LVNI_SELECTED, LVS_EX_FULLROWSELECT, LVS_NOCOLUMNHEADER,
    LVS_OWNERDATA, LVS_REPORT, LVS_SHOWSELALWAYS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, SendMessageW, SetWindowPos, ShowWindow, SWP_NOZORDER, SW_HIDE, SW_SHOWNA,
    WINDOW_STYLE, WS_CHILD, WS_CLIPCHILDREN, WS_TABSTOP, WS_VISIBLE,
};

use crate::dialog::PANEL_COLOR;
use crate::listdraw;
use crate::photo::PhotoList;
use crate::tools;
use crate::{state, w32};

pub use crate::listdraw::{begin_drag, custom_draw};

/// The list view of a hidden playlist keeps this size; a shown one is as wide as the window.
const WIDTH: i32 = 320;
const NO_LIST: HWND = HWND(std::ptr::null_mut());

// The list is only ever used from the UI thread, and a `static` would need to be `Sync`.
thread_local! {
    static LIST: Cell<HWND> = const { Cell::new(NO_LIST) };
    // ListView can request labels synchronously while the viewer state is borrowed.
    static ROWS: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
    static CURRENT: Cell<Option<usize>> = const { Cell::new(None) };
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
    ROWS.with(|rows| rows.borrow().get(row).map(|path| shown_name(path)))
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

/// Fills the list with `rows`, marks `current` as the shown one, and puts the list below the band
/// of the tool bar: the rest of the window.
pub fn show(parent: HWND, photos: &PhotoList, width: i32, height: i32) {
    unsafe {
        let _ = ensure_list(parent);
        sync(photos);
        resize(width, height);
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

/// Fill the window below the toolbar when the playlist is visible.
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
        let _ = ShowWindow(list, SW_SHOWNA);
        let _ = SetWindowPos(
            list,
            None,
            0,
            top,
            width,
            (height - top).max(0),
            SWP_NOZORDER,
        );
        // `LVM_SETCOLUMNW` takes an `LVCOLUMNW`, not a width: passing the number makes comctl32
        // read it as a pointer and crash.
        let column = LVCOLUMNW {
            mask: LVCF_WIDTH,
            cx: (width - 24).max(1),
            ..Default::default()
        };
        let _ = SendMessageW(
            list,
            LVM_SETCOLUMNW,
            Some(WPARAM(0)),
            Some(LPARAM(&column as *const _ as isize)),
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
    created
}

/// Rebuilds the list from `rows`, which is how a change of order is shown as well.
pub fn sync(photos: &PhotoList) {
    listdraw::end_drag();
    let list = list();
    if list == NO_LIST {
        return;
    }
    let append = ROWS.with(|stored| {
        let mut rows = stored.borrow_mut();
        let append =
            rows.len() <= photos.len() && rows.iter().eq(photos.visible_rows().take(rows.len()));
        if append {
            let previous_len = rows.len();
            rows.extend(photos.visible_rows().skip(previous_len).cloned());
        } else {
            *rows = photos.visible_rows().cloned().collect();
        }
        append
    });
    unsafe {
        if !append {
            CURRENT.set(None);
            let _ = SendMessageW(list, LVM_SETITEMCOUNT, Some(WPARAM(0)), None);
        }
        let _ = SendMessageW(list, LVM_SETITEMCOUNT, Some(WPARAM(photos.len())), None);
        mark(photos.current().map(|_| photos.index));
        w32::invalidate(list);
    }
}

/// Rebuild only when the visible rows change, preserving scroll and selection on other refreshes.
pub fn refresh(photos: &PhotoList) {
    let list = list();
    if list == NO_LIST {
        return;
    }
    if ROWS.with(|stored| stored.borrow().iter().ne(photos.visible_rows())) {
        sync(photos);
    } else {
        mark(photos.current().map(|_| photos.index));
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
use control::{create, shown_name};

pub use control::{display_info, find_item};
