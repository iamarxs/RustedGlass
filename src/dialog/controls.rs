use super::*;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
pub(super) unsafe extern "system" fn prompt_key(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    parent: usize,
) -> LRESULT {
    if msg == WM_KEYDOWN && wparam.0 == VK_RETURN.0 as usize {
        let _ = PostMessageW(
            Some(HWND(parent as *mut core::ffi::c_void)),
            WM_COMMAND,
            WPARAM(PROMPT_ID),
            LPARAM(0),
        );
        return LRESULT(0);
    }
    if msg == WM_KEYDOWN && wparam.0 == VK_ESCAPE.0 as usize {
        let _ = PostMessageW(
            Some(HWND(parent as *mut core::ffi::c_void)),
            msg,
            wparam,
            lparam,
        );
        return LRESULT(0);
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

/// `ES_AUTOHSCROLL` lets a long path scroll inside the field instead of wrapping.
pub(super) fn create(
    class: *const u16,
    text: *const u16,
    style: WINDOW_STYLE,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    parent: HWND,
    id: usize,
    instance: windows::Win32::Foundation::HINSTANCE,
) -> windows::core::Result<HWND> {
    unsafe {
        CreateWindowExW(
            Default::default(),
            PCWSTR(class),
            PCWSTR(text),
            style,
            x,
            y + 2,
            width,
            height,
            Some(parent),
            Some(HMENU(id as *mut core::ffi::c_void)),
            Some(instance),
            None,
        )
    }
}

/// Turn off visual styles for a control. Themed controls paint themselves in system colours and
/// ignore the brush asked for in `WM_CTLCOLOR*`, which is what makes a label unreadable.
pub(super) fn untheme(handle: HWND) {
    let _ = unsafe { SetWindowTheme(handle, None, None) };
}

pub fn close() {
    SCROLL.with(|cell| cell.set(0));
    SCROLL_MAX.with(|cell| cell.set(0));
    let handles = CONTROLS.with(|controls| controls.replace(Vec::new()));
    for (_, handle) in handles {
        unsafe {
            let _ = DestroyWindow(handle);
        }
    }
}

/// What the prompt starts with: the current file name, or the folder a copy would land in.
pub(super) fn prefill(pending: Option<&Pending>, file: Option<&Path>) -> String {
    match (pending, file) {
        (Some(operation), Some(path)) => operation.prefill(path),
        _ => String::new(),
    }
}

pub(super) fn set_checked(handle: HWND, checked: bool) {
    let state = if checked { BST_CHECKED.0 } else { 0 };
    unsafe {
        let _ = SendMessageW(
            handle,
            BM_SETCHECK,
            Some(WPARAM(state as usize)),
            Some(LPARAM(0)),
        );
    }
}

pub(super) fn checked(handle: HWND) -> bool {
    unsafe { SendMessageW(handle, BM_GETCHECK, Some(WPARAM(0)), Some(LPARAM(0))).0 == 1 }
}

pub fn click_checkbox_label(hwnd: HWND, x: i32, y: i32) -> Option<usize> {
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{GetDC, GetTextExtentPoint32W, ReleaseDC, SelectObject};
    use windows::Win32::UI::WindowsAndMessaging::GetDlgItem;

    if x < CHECK_LABEL_X || y < HEADER {
        return None;
    }
    let index = ((y - HEADER) / ROW_HEIGHT) as usize;
    let row = ROWS.get(index)?;
    if row.kind != Kind::CheckBox || y < row_y(index) + ROW_LABEL_TOP {
        return None;
    }

    let text = w32::wide(row.label);
    let mut size = SIZE::default();
    unsafe {
        let dc = GetDC(Some(hwnd));
        if dc.is_invalid() {
            return None;
        }
        let previous = SelectObject(dc, crate::style::font());
        let measured = GetTextExtentPoint32W(dc, &text[..text.len() - 1], &mut size);
        let _ = SelectObject(dc, previous);
        let _ = ReleaseDC(Some(hwnd), dc);
        if !measured.as_bool()
            || x >= CHECK_LABEL_X + size.cx
            || y >= row_y(index) + ROW_LABEL_TOP + size.cy
        {
            return None;
        }
        let id = BASE_ID + index;
        let checkbox = GetDlgItem(Some(hwnd), id as i32).ok()?;
        set_checked(checkbox, !checked(checkbox));
        Some(id)
    }
}

pub(super) fn field_text(id: usize) -> Option<String> {
    CONTROLS.with(|controls| {
        controls
            .borrow()
            .iter()
            .find(|(control, _)| *control == id)
            .map(|(_, handle)| unsafe { w32::control_text(*handle) })
    })
}

/// What the prompt field holds: the name or folder the user typed.
pub fn prompt_text() -> Option<String> {
    field_text(PROMPT_ID)
}

/// Writes a picked folder into a path field, so the user can still edit it.
pub fn set_field_text(id: usize, text: &str) {
    let wide = w32::wide(text);
    CONTROLS.with(|controls| {
        if let Some((_, handle)) = controls.borrow().iter().find(|(control, _)| *control == id) {
            unsafe {
                let _ = SetWindowTextW(*handle, PCWSTR(wide.as_ptr()));
            }
        }
    });
}

pub fn slideshow_intervals() -> Result<(u32, u32), &'static str> {
    let text = |key| {
        ROWS.iter()
            .position(|row| row.key == key)
            .and_then(|index| field_text(BASE_ID + index))
            .unwrap_or_default()
    };
    parse_slideshow_intervals(&text("SlideshowInterval"), &text("SlideshowIntervalTo"))
}

pub(super) fn parse_slideshow_intervals(
    minimum: &str,
    maximum: &str,
) -> Result<(u32, u32), &'static str> {
    // SetTimer accepts at most 0x7fff_ffff milliseconds.
    const MAX_SECONDS: u32 = 0x7fff_ffff / 1000;
    let seconds = |text: &str| {
        text.trim()
            .parse::<u32>()
            .ok()
            .filter(|number| (1..=MAX_SECONDS).contains(number))
    };
    let minimum = seconds(minimum)
        .ok_or("Enter a whole number from 1 to 2,147,483 for “Show each image for”.")?;
    let maximum = seconds(maximum)
        .ok_or("Enter a whole number from 1 to 2,147,483 for “… or up to this many seconds”.")?;
    if maximum < minimum {
        return Err("“… or up to this many seconds” must be at least “Show each image for”.");
    }
    Ok((minimum, maximum))
}
