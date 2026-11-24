use super::*;
use std::path::PathBuf;
use windows::Win32::Foundation::{LRESULT, RECT};
use windows::Win32::UI::Controls::{
    LVFI_PARTIAL, LVFI_STRING, LVFI_WRAP, LVIF_TEXT, LVM_ENSUREVISIBLE, LVM_GETCOUNTPERPAGE,
    LVM_GETITEMRECT, LVM_GETTOPINDEX, LVM_SCROLL, LVS_EX_DOUBLEBUFFER, NMLVDISPINFOW,
    NMLVFINDITEMW,
};

pub(crate) struct ScrollAnchor {
    file: usize,
    path: PathBuf,
}

impl ScrollAnchor {
    pub(super) fn position(&self, photos: &crate::photo::PhotoList) -> Option<usize> {
        let file = if photos.files.get(self.file) == Some(&self.path) {
            self.file
        } else {
            photos.files.iter().position(|path| path == &self.path)?
        };
        photos.position_of_file(file)
    }
}

pub(crate) fn capture_background_scroll() -> Option<ScrollAnchor> {
    let list = list();
    if list == NO_LIST
        || state::read_state(|state| {
            state.playlist_visible && (state.importing() || columns::sort_pending())
        }) != Some(true)
    {
        return None;
    }
    let top = top_row(list);
    state::read_state(|state| {
        Some(ScrollAnchor {
            file: state.photos.file_index(top)?,
            path: state.photos.path_at(top)?.clone(),
        })
    })
    .flatten()
}

fn top_row(list: HWND) -> usize {
    unsafe { SendMessageW(list, LVM_GETTOPINDEX, None, None).0.max(0) as usize }
}

pub(super) fn restore_top_row(list: HWND, row: usize) {
    let top = top_row(list);
    let mut bounds = RECT::default(); // LVIR_BOUNDS
    unsafe {
        if SendMessageW(
            list,
            LVM_GETITEMRECT,
            Some(WPARAM(top)),
            Some(LPARAM(&mut bounds as *mut _ as isize)),
        )
        .0 != 0
        {
            let height = (bounds.bottom - bounds.top) as isize;
            let delta = (row as isize - top as isize).saturating_mul(height);
            let _ = SendMessageW(list, LVM_SCROLL, None, Some(LPARAM(delta)));
        }
    }
}

pub(crate) fn rows_per_page() -> usize {
    let list = list();
    if list == NO_LIST {
        return 0;
    }
    unsafe { SendMessageW(list, LVM_GETCOUNTPERPAGE, None, None).0.max(0) as usize }
}

pub(crate) fn browse_page(owner: HWND, down: bool) {
    let list = list();
    if list == NO_LIST {
        return;
    }
    let page = rows_per_page().max(1);
    let Some((current, last)) = state::read_state(|state| {
        if !state.playlist_visible {
            return None;
        }
        Some((state.photos.index, state.photos.len().checked_sub(1)?))
    })
    .flatten() else {
        return;
    };
    let row = if down {
        current.saturating_add(page).min(last)
    } else {
        current.saturating_sub(page)
    };
    if row != current {
        crate::input::jump_to(owner, row);
    }
    let _ = unsafe { SendMessageW(list, LVM_ENSUREVISIBLE, Some(WPARAM(row)), None) };
}
/// The list view control.
pub(super) unsafe fn create(parent: HWND) -> HWND {
    let created = CreateWindowExW(
        Default::default(),
        PCWSTR(w32::wide("SysListView32").as_ptr()),
        PCWSTR(w32::wide("Playlist").as_ptr()),
        WS_CHILD
            | WS_VISIBLE
            | WS_TABSTOP
            | WS_CLIPCHILDREN
            | WINDOW_STYLE(LVS_REPORT | LVS_OWNERDATA)
            | WINDOW_STYLE(LVS_SHOWSELALWAYS),
        0,
        0,
        WIDTH,
        200,
        Some(parent),
        None,
        Some(w32::instance()),
        None,
    )
    .unwrap_or(NO_LIST);
    // Full-row select: without it a click lands on the label only, and the row is not drawn as
    // selected, which is the only way its colour can be chosen.
    let styles = (LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER) as usize;
    let _ = SendMessageW(
        created,
        LVM_SETEXTENDEDLISTVIEWSTYLE,
        Some(WPARAM(styles)),
        Some(LPARAM(styles as isize)),
    );
    // The columns are inserted when the list is created: `columns::install` builds them
    // from the shown flags, in `Column::ALL` order.
    unsafe {
        windows::Win32::UI::Shell::DragAcceptFiles(created, true);
    }
    let _ = unsafe {
        SendMessageW(
            created,
            windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,
            Some(WPARAM(crate::style::font().0 as usize)),
            None,
        )
    };
    let background = PANEL_COLOR.0 as usize;
    let _ = SendMessageW(
        created,
        LVM_SETBKCOLOR,
        Some(WPARAM(background)),
        Some(LPARAM(0)),
    );
    let _ = SendMessageW(
        created,
        LVM_SETTEXTBKCOLOR,
        Some(WPARAM(background)),
        Some(LPARAM(0)),
    );
    created
}

/// A full path is too wide for the list, and the names in one folder differ from each other.
pub(super) fn shown_name(path: &Path) -> String {
    match path.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => path.to_string_lossy().into_owned(),
    }
}

pub unsafe fn display_info(lparam: LPARAM) {
    let item = unsafe { &mut (*(lparam.0 as *mut NMLVDISPINFOW)).item };
    if !item.mask.contains(LVIF_TEXT) || item.pszText.is_null() || item.cchTextMax <= 0 {
        return;
    }
    let text = super::columns::cell_text(item.iItem as usize, item.iSubItem as usize);
    let text = w32::wide(text.as_str());
    let mut count = (text.len() - 1).min(item.cchTextMax as usize - 1);
    if count > 0 && (0xD800..=0xDBFF).contains(&text[count - 1]) {
        count -= 1;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(text.as_ptr(), item.pszText.0, count);
        item.pszText.0.add(count).write(0);
    }
}

pub unsafe fn find_item(lparam: LPARAM) -> LRESULT {
    let request = unsafe { &*(lparam.0 as *const NMLVFINDITEMW) };
    let flags = request.lvfi.flags;
    let partial = flags.contains(LVFI_PARTIAL);
    if (!partial && !flags.contains(LVFI_STRING)) || request.lvfi.psz.is_null() {
        return LRESULT(-1);
    }
    let Ok(query) = (unsafe { request.lvfi.psz.to_string() }) else {
        return LRESULT(-1);
    };
    let query = query.to_lowercase();
    state::read_state(|state| {
        let photos = &state.photos;
        let start = (request.iStart.max(0) as usize).min(photos.len());
        let wrap = if flags.contains(LVFI_WRAP) { start } else { 0 };
        (start..photos.len())
            .chain(0..wrap)
            .find(|&row| {
                let Some(path) = photos.path_at(row) else {
                    return false;
                };
                let name = shown_name(path).to_lowercase();
                if partial {
                    name.starts_with(&query)
                } else {
                    name == query
                }
            })
            .map_or(LRESULT(-1), |row| LRESULT(row as isize))
    })
    .unwrap_or(LRESULT(-1))
}
