use super::*;
use windows::Win32::Foundation::LRESULT;
use windows::Win32::UI::Controls::{
    LVFI_PARTIAL, LVFI_STRING, LVFI_WRAP, LVIF_TEXT, NMLVDISPINFOW, NMLVFINDITEMW,
};
/// The list view control, the same one the C# `ImageListView` uses.
pub(super) unsafe fn create(parent: HWND) -> HWND {
    let created = CreateWindowExW(
        Default::default(),
        PCWSTR(w32::wide("SysListView32").as_ptr()),
        PCWSTR(w32::wide("Playlist").as_ptr()),
        WS_CHILD
            | WS_VISIBLE
            | WS_TABSTOP
            | WS_CLIPCHILDREN
            | WINDOW_STYLE(LVS_REPORT | LVS_NOCOLUMNHEADER | LVS_OWNERDATA)
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
    let _ = SendMessageW(
        created,
        LVM_SETEXTENDEDLISTVIEWSTYLE,
        Some(WPARAM(LVS_EX_FULLROWSELECT as usize)),
        Some(LPARAM(LVS_EX_FULLROWSELECT as isize)),
    );
    let mut header = w32::wide("Playlist");
    let column = LVCOLUMNW {
        mask: LVCF_TEXT,
        cx: WIDTH - 12,
        pszText: PWSTR(header.as_mut_ptr()),
        cchTextMax: 8,
        ..Default::default()
    };
    let _ = SendMessageW(
        created,
        LVM_INSERTCOLUMNW,
        Some(WPARAM(0)),
        Some(LPARAM(&column as *const _ as isize)),
    );
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
    let name = row_name(item.iItem as usize).unwrap_or_default();
    let text = w32::wide(&name);
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
    ROWS.with(|stored| {
        let rows = stored.borrow();
        let start = (request.iStart.max(0) as usize).min(rows.len());
        let wrap = if flags.contains(LVFI_WRAP) { start } else { 0 };
        (start..rows.len())
            .chain(0..wrap)
            .find(|&row| {
                let name = shown_name(&rows[row]).to_lowercase();
                if partial {
                    name.starts_with(&query)
                } else {
                    name == query
                }
            })
            .map_or(LRESULT(-1), |row| LRESULT(row as isize))
    })
}
