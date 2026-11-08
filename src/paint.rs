//! What the window paints: the dark background, the picture, the tool bar band, the status line
//! and the open panel.

use std::cell::{Cell, RefCell};

use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject,
    EndPaint, IntersectClipRect, RestoreDC, SaveDC, SelectObject, COLORONCOLOR, HALFTONE, HBITMAP,
    HDC, HGDIOBJ, PAINTSTRUCT, SRCCOPY,
};

use crate::dialog::{self, Dialog};
use crate::render;
use crate::state;
use crate::style;
use crate::w32;

pub const BACKGROUND: COLORREF = COLORREF(0x0011_1111);

thread_local! {
    static LIVE_RESIZE: Cell<bool> = const { Cell::new(false) };
    static BACK_BUFFER: RefCell<Option<Buffer>> = const { RefCell::new(None) };
}

pub fn is_live_resize() -> bool {
    LIVE_RESIZE.get()
}

pub fn set_live_resize(active: bool) {
    LIVE_RESIZE.set(active);
}

pub fn clear_buffer() {
    BACK_BUFFER.with(|buffer| {
        if let Ok(mut buffer) = buffer.try_borrow_mut() {
            *buffer = None;
        }
    });
}

/// Draw into a reusable bitmap, then copy the visible part to the window in one blit.
pub unsafe fn paint(hwnd: HWND) {
    let mut paint_struct = PAINTSTRUCT::default();
    let screen = unsafe { BeginPaint(hwnd, &mut paint_struct) };
    let area = w32::client_size(hwnd);
    BACK_BUFFER.with(|buffer| {
        let Ok(mut buffer) = buffer.try_borrow_mut() else {
            return;
        };
        let Some(back) = buffer_for_size(&mut buffer, screen, area) else {
            return;
        };
        let hdc = back.dc;
        let covered = crate::playlist::is_visible();
        let (width, height) = (area.0 as i32, area.1 as i32);
        let paint_height = if covered {
            state::with_state(|state| {
                if state.toolbar_visible {
                    height.min(crate::tools::BAND_HEIGHT)
                } else {
                    0
                }
            })
            .unwrap_or(height)
        } else {
            height
        };
        if paint_height == 0 {
            return;
        }

        unsafe {
            let _ = SelectObject(hdc, style::font());
            w32::fill_rect(hdc, 0, 0, width, paint_height, BACKGROUND);
        }
        let stretch = if LIVE_RESIZE.get() {
            COLORONCOLOR
        } else {
            HALFTONE
        };
        state::with_state(|state| unsafe {
            let factor = state.factor(area);
            let offset = state.offset;
            let mut render_error = None;
            if let Some(bitmap) = state.bitmap.as_mut() {
                if covered {
                    // The image still shows through the gaps between toolbar buttons.
                    let saved = SaveDC(hdc);
                    if saved != 0 {
                        let _ = IntersectClipRect(hdc, 0, 0, width, paint_height);
                        render_error =
                            render::draw(hdc, bitmap, area, factor, offset, stretch).err();
                        let _ = RestoreDC(hdc, saved);
                    }
                } else {
                    render_error = render::draw(hdc, bitmap, area, factor, offset, stretch).err();
                }
            }

            if state.dialog != Dialog::None {
                dialog::draw(&state.dialog, hdc, width, height);
            } else if !covered {
                if let Some(message) = render_error
                    .as_ref()
                    .or(state.notice.as_ref())
                    .or(state.message.as_ref())
                {
                    status(hdc, area, message);
                }
            }
        });
        back.present(screen, area.0, paint_height as u32);
    });
    let _ = unsafe { EndPaint(hwnd, &paint_struct) };
}

fn buffer_for_size(buffer: &mut Option<Buffer>, screen: HDC, size: (u32, u32)) -> Option<&Buffer> {
    if buffer.as_ref().map(|back| back.size) != Some(size) {
        *buffer = None;
        *buffer = Buffer::new(screen, size);
    }
    buffer.as_ref()
}

pub fn status(hdc: HDC, (width, height): (u32, u32), message: &str) {
    let width = width as i32;
    let bottom = height as i32 - 16;
    let left = (width - 640).max(32) / 2;
    if width < 48 || bottom < 36 {
        return;
    }
    unsafe {
        let _ = SelectObject(hdc, style::font());
    }
    style::surface(
        hdc,
        (left, bottom - 36, width - left, bottom),
        style::CONTROL,
    );
    style::caption(
        hdc,
        (left + 12, bottom - 36, width - left - 12, bottom),
        message,
    );
}

/// A bitmap in memory, painted on and then copied to the window in one Blot.
struct Buffer {
    size: (u32, u32),
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
}

impl Buffer {
    fn new(screen: HDC, size: (u32, u32)) -> Option<Buffer> {
        let (width, height) = size;
        unsafe {
            if width == 0 || height == 0 {
                return None;
            }
            let (width, height) = (width as i32, height as i32);
            let dc = CreateCompatibleDC(Some(screen));
            let bitmap = CreateCompatibleBitmap(screen, width, height);
            if dc.is_invalid() || bitmap.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
                let _ = DeleteDC(dc);
                return None;
            }
            let previous = SelectObject(dc, HGDIOBJ(bitmap.0));
            Some(Buffer {
                size,
                dc,
                bitmap,
                previous,
            })
        }
    }

    fn present(&self, screen: HDC, width: u32, height: u32) {
        let _ = unsafe {
            BitBlt(
                screen,
                0,
                0,
                width as i32,
                height as i32,
                Some(self.dc),
                0,
                0,
                SRCCOPY,
            )
        };
    }
}

impl Drop for Buffer {
    /// The bitmap has to be out of the memory device before either of them is freed.
    fn drop(&mut self) {
        unsafe {
            let _ = SelectObject(self.dc, self.previous);
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.dc);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Gdi::{GetDC, ReleaseDC};

    #[test]
    fn the_back_buffer_is_reused_at_the_same_size() {
        unsafe {
            let screen = GetDC(None);
            assert!(!screen.is_invalid());
            let mut buffer = None;
            let first = buffer_for_size(&mut buffer, screen, (32, 16))
                .unwrap()
                .bitmap;
            assert_eq!(
                buffer_for_size(&mut buffer, screen, (32, 16))
                    .unwrap()
                    .bitmap,
                first
            );
            assert_eq!(
                buffer_for_size(&mut buffer, screen, (64, 16)).unwrap().size,
                (64, 16)
            );
            assert!(buffer_for_size(&mut buffer, screen, (0, 0)).is_none());
            let _ = ReleaseDC(None, screen);
        }
    }
}
