use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{GetDC, GetTextExtentPoint32W, ReleaseDC, SelectObject};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GetCursorPos, GetParent, SendMessageW, SetLayeredWindowAttributes,
    SetWindowPos, LWA_ALPHA, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
    WINDOW_STYLE, WM_SETFONT, WS_BORDER, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TRANSPARENT, WS_POPUP,
};
const SS_CENTERIMAGE: u32 = 0x0200;
const SS_ENDELLIPSIS: u32 = 0x4000;
const SS_NOPREFIX: u32 = 0x0080;

pub(super) fn open(list: HWND, name: &str) -> Option<HWND> {
    let text = crate::w32::wide(name);
    let font = crate::style::font();
    let mut size = SIZE { cx: 180, cy: 18 };
    unsafe {
        let dc = GetDC(Some(list));
        if !dc.is_invalid() {
            let previous = SelectObject(dc, font);
            let _ = GetTextExtentPoint32W(dc, &text[..text.len() - 1], &mut size);
            let _ = SelectObject(dc, previous);
            let _ = ReleaseDC(Some(list), dc);
        }
        let owner = GetParent(list).ok()?;
        let preview = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            windows::core::w!("STATIC"),
            PCWSTR(text.as_ptr()),
            WS_POPUP
                | WS_BORDER
                | WINDOW_STYLE((SS_CENTERIMAGE | SS_ENDELLIPSIS | SS_NOPREFIX) as u32),
            0,
            0,
            (size.cx + 12).clamp(24, 440),
            size.cy + 8,
            Some(owner),
            None,
            Some(crate::w32::instance()),
            None,
        )
        .ok()?;
        let _ = SendMessageW(
            preview,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(0)),
        );
        if SetLayeredWindowAttributes(preview, Default::default(), 185, LWA_ALPHA).is_err() {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(preview);
            return None;
        }
        follow(preview);
        Some(preview)
    }
}

pub(super) fn follow(preview: HWND) {
    let mut cursor = POINT::default();
    unsafe {
        if GetCursorPos(&mut cursor).is_ok() {
            let _ = SetWindowPos(
                preview,
                None,
                cursor.x + 10,
                cursor.y + 12,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }
}
