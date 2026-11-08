//! Small Win32 helpers shared by the viewer modules: wide strings, pointer coordinates,
//! client sizes, text drawing and repaint requests.

use std::path::Path;

use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, DrawTextW, FillRect, GetStockObject, SetBkMode, SetTextColor,
    TextOutW, DT_CALCRECT, DT_LEFT, DT_WORDBREAK, HBRUSH, HDC, HGDIOBJ, NULL_BRUSH, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetWindowRect, SendMessageW, WM_GETTEXT,
};

/// The module handle child controls take as their instance.
pub fn instance() -> HINSTANCE {
    let module = unsafe { GetModuleHandleW(None) }.unwrap_or_default();
    HINSTANCE(module.0)
}

/// UTF-16 with a trailing null, the form the wide Win32 APIs expect.
pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `lparam` of a pointer message: the x coordinate in client pixels, signed.
pub fn pointer_x(lparam: LPARAM) -> i32 {
    ((lparam.0 as u32) & 0xFFFF) as u16 as i16 as i32
}

/// `lparam` of a pointer message: the y coordinate in client pixels, signed.
pub fn pointer_y(lparam: LPARAM) -> i32 {
    ((lparam.0 as u32) >> 16) as u16 as i16 as i32
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// The window's outer rectangle as `(x, y, width, height)`.
pub unsafe fn window_rect(hwnd: HWND) -> (i32, i32, i32, i32) {
    let mut rect = RECT::default();
    let _ = unsafe { GetWindowRect(hwnd, &mut rect) };
    (
        rect.left,
        rect.top,
        rect.right - rect.left,
        rect.bottom - rect.top,
    )
}

pub unsafe fn client_size(hwnd: HWND) -> (u32, u32) {
    let mut rect = RECT::default();
    let _ = unsafe { GetClientRect(hwnd, &mut rect) };

    (
        (rect.right - rect.left).max(1) as u32,
        (rect.bottom - rect.top).max(1) as u32,
    )
}

/// Requests a repaint. Erasing is what clears the previous picture; without it a smaller or
/// moved image leaves the old one behind.
pub unsafe fn invalidate(hwnd: HWND) {
    let _ = unsafe { windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), None, true) };
}

/// Draws text at a client position in `color`, over whatever is already painted there.
pub unsafe fn draw_text(hdc: HDC, x: i32, y: i32, text: &str, color: COLORREF) {
    unsafe {
        let _ = SetTextColor(hdc, color);
        let _ = SetBkMode(hdc, TRANSPARENT);
        let _ = TextOutW(hdc, x, y, &wide(text));
    }
}

/// Paints a rectangle of the client area, for the panels behind the settings controls.
pub unsafe fn fill_rect(hdc: HDC, x: i32, y: i32, width: i32, height: i32, color: COLORREF) {
    let brush = unsafe { CreateSolidBrush(color) };
    let rect = RECT {
        left: x,
        top: y,
        right: x + width,
        bottom: y + height,
    };
    unsafe {
        let _ = FillRect(hdc, &rect, brush);
        let _ = DeleteObject(HGDIOBJ(brush.0));
    }
}

/// Colour and background for a static control or check box, so its text matches the panel it
/// sits on instead of a grey system box.
pub unsafe fn static_colour(hdc: HDC, color: COLORREF) -> HBRUSH {
    let _ = unsafe { SetTextColor(hdc, color) };
    let _ = unsafe { SetBkMode(hdc, TRANSPARENT) };
    let HGDIOBJ(brush) = unsafe { GetStockObject(NULL_BRUSH) };
    HBRUSH(brush as *mut _)
}

/// Pixels a wrapped block of text needs at `width`. `DT_CALCRECT` only sizes the text.
pub unsafe fn text_height(hdc: HDC, width: i32, text: &str) -> i32 {
    let mut chars = wide(text);
    // `DrawTextW` takes the text as a slice and sizes it by its length, so no null is passed on.
    chars.pop();
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: width.max(1),
        bottom: 0,
    };
    unsafe { DrawTextW(hdc, &mut chars, &mut rect, DT_WORDBREAK | DT_CALCRECT) }.max(0)
}

/// Draw text wrapped into `width`, and return the y just below it. The shortcuts panel needs this:
/// a description longer than its column was cut with an ellipsis and became unreadable.
pub unsafe fn draw_wrapped(
    hdc: HDC,
    x: i32,
    y: i32,
    width: i32,
    text: &str,
    color: COLORREF,
) -> i32 {
    let mut chars = wide(text);
    chars.pop();
    let _ = unsafe { SetTextColor(hdc, color) };
    let _ = unsafe { SetBkMode(hdc, TRANSPARENT) };
    let mut rect = RECT {
        left: x,
        top: y,
        right: x + width.max(1),
        bottom: y.saturating_add(8_000),
    };
    let bottom = unsafe { DrawTextW(hdc, &mut chars, &mut rect, DT_WORDBREAK | DT_LEFT) };
    // A draw that failed still has to move the next row down, or the list stacks on one line.
    if bottom > 0 {
        bottom
    } else {
        y + 22
    }
}

/// The signed wheel step carried in a `WM_MOUSEWHEEL` message: the delta is a 16-bit number in
/// the high word, so a step down has to be read as a negative one.
pub fn wheel_delta(wparam: WPARAM) -> i32 {
    ((wparam.0 >> 16) as u16) as i16 as i32
}

/// The control or menu id of a `WM_COMMAND`. The high word of the same parameter says what
/// happened to the control (`BN_CLICKED`, `EN_CHANGE`), and is not part of the id.
pub fn command_id(wparam: WPARAM) -> usize {
    (wparam.0 as u32 & u16::MAX as u32) as usize
}

/// Reads a control's text without the `Win32_UI_Controls` feature.
pub unsafe fn control_text(hwnd: HWND) -> String {
    let mut buffer = vec![0u16; 256];
    let len = unsafe {
        SendMessageW(
            hwnd,
            WM_GETTEXT,
            Some(WPARAM(255)),
            Some(LPARAM(buffer.as_mut_ptr() as isize)),
        )
    };
    let len = len.0.max(0) as usize;
    String::from_utf16_lossy(&buffer[..len.min(buffer.len())])
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::LPARAM;

    #[test]
    fn wide_strings_end_with_a_null() {
        assert_eq!(wide("ab"), vec![b'a' as u16, b'b' as u16, 0]);
    }

    #[test]
    fn pointer_coordinates_are_signed_halves_of_lparam() {
        let lparam = LPARAM((((-3_i32 as u32) << 16) | 4096) as isize);
        assert_eq!((pointer_x(lparam), pointer_y(lparam)), (4096, -3));
    }

    #[test]
    fn a_file_name_falls_back_to_the_whole_path() {
        assert_eq!(file_name(Path::new("pics/cat.png")), "cat.png");
        assert_eq!(file_name(Path::new("pics")), "pics");
    }

    #[test]
    fn a_wheel_step_is_the_high_word_of_wparam() {
        assert_eq!(wheel_delta(WPARAM(0x0078_01c2)), 120);
        assert_eq!(wheel_delta(WPARAM(0xff88_01c2)), -120);
    }

    #[test]
    fn a_command_id_is_the_low_word_of_wparam() {
        // `BN_CLICKED` of button 2002 arrives as both halves, and only the lower one is the id.
        assert_eq!(command_id(WPARAM((1 << 16) | 2002)), 2002);
        assert_eq!(command_id(WPARAM(2002)), 2002);
    }
}
