use super::*;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
pub(crate) unsafe extern "system" fn prompt_key(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    parent: usize,
) -> LRESULT {
    if msg == WM_KEYDOWN && wparam.0 == VK_RETURN.0 as usize {
        let _ = PostMessageW(
            Some(HWND(parent as *mut core::ffi::c_void)),
            WM_COMMAND,
            WPARAM(id),
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
pub(crate) fn create(
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
    drop(SETTINGS_TOOLTIP.take());
    SCROLL.with(|cell| cell.set(0));
    SCROLL_MAX.with(|cell| cell.set(0));
    let handles = CONTROLS.with(|controls| controls.replace(Vec::new()));
    for (_, handle) in handles {
        unsafe {
            let _ = DestroyWindow(handle);
        }
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

pub(super) fn checkbox_handle(index: usize) -> Option<HWND> {
    CONTROLS.with_borrow(|controls| {
        controls
            .iter()
            .find(|(id, _)| *id == BASE_ID + index)
            .map(|(_, handle)| *handle)
    })
}

pub(super) fn enable_file_maximize(enabled: bool) {
    let handle = ROWS
        .iter()
        .position(|row| row.key == FILES_MAXIMIZED)
        .and_then(checkbox_handle);
    if let Some(handle) = handle {
        let _ = unsafe { EnableWindow(handle, enabled) };
    }
}

pub fn click_checkbox_label(hwnd: HWND, x: i32, y: i32) -> Option<usize> {
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{GetDC, GetTextExtentPoint32W, ReleaseDC, SelectObject};

    if x < CHECK_LABEL_X || y < HEADER {
        return None;
    }
    let index = ((y - HEADER) / ROW_HEIGHT) as usize;
    let row = ROWS.get(index)?;
    let label_x = CHECK_LABEL_X + row.indent();
    if row.kind != Kind::CheckBox || x < label_x || y < row_y(index) + ROW_LABEL_TOP {
        return None;
    }
    let checkbox = checkbox_handle(index)?;
    if !unsafe { IsWindowEnabled(checkbox) }.as_bool() {
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
            || x >= label_x + size.cx
            || y >= row_y(index) + ROW_LABEL_TOP + size.cy
        {
            return None;
        }
        set_checked(checkbox, !checked(checkbox));
        Some(BASE_ID + index)
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

pub fn normalize_slide_durations(id: usize) {
    let Some(row) = id.checked_sub(BASE_ID).and_then(|index| ROWS.get(index)) else {
        return;
    };
    if !matches!(row.key, "SlideshowInterval" | "SlideshowIntervalTo") {
        return;
    }
    let Some(maximum) = ROWS.iter().position(|row| row.key == "SlideshowIntervalTo") else {
        return;
    };
    if let Ok((_, seconds)) = slideshow_intervals() {
        let id = BASE_ID + maximum;
        if field_text(id)
            .and_then(|text| text.trim().parse::<i64>().ok())
            .is_some_and(|entered| entered < i64::from(seconds))
        {
            set_field_text(id, &seconds.to_string());
        }
    }
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
        .ok_or("Enter a whole number from 1 to 2,147,483 for the minimum slide duration.")?;
    let maximum = maximum
        .trim()
        .parse::<i64>()
        .ok()
        .filter(|&seconds| seconds <= i64::from(MAX_SECONDS))
        .ok_or("Enter a whole number up to 2,147,483 for the maximum slide duration.")?;
    Ok((minimum, maximum.max(i64::from(minimum)) as u32))
}
