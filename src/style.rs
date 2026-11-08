use std::cell::OnceCell;
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Dwm::DwmGetColorizationColor;
use windows::Win32::Graphics::Gdi::{
    CreateFontIndirectW, CreatePen, CreateSolidBrush, DeleteObject, DrawTextW, GetStockObject,
    RoundRect, SelectObject, SetBkMode, SetTextColor, DEFAULT_GUI_FONT, DT_CENTER, DT_END_ELLIPSIS,
    DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, HDC, HGDIOBJ, LOGFONTW, PS_SOLID, TRANSPARENT,
};

pub const CONTROL: COLORREF = COLORREF(0x002B_2B2B);
pub const HOVER: COLORREF = COLORREF(0x0039_3939);
pub const PRESSED: COLORREF = COLORREF(0x0045_4545);

pub fn font() -> HGDIOBJ {
    thread_local! { static FONT: OnceCell<HGDIOBJ> = const { OnceCell::new() }; }
    FONT.with(|font| {
        *font.get_or_init(|| {
            let mut spec = LOGFONTW {
                lfHeight: -15,
                lfWeight: 400,
                ..Default::default()
            };
            let face = crate::w32::wide("Segoe UI");
            spec.lfFaceName[..face.len()].copy_from_slice(&face);
            let font = unsafe { CreateFontIndirectW(&spec) };
            if font.is_invalid() {
                unsafe { GetStockObject(DEFAULT_GUI_FONT) }
            } else {
                font.into()
            }
        })
    })
}

pub fn accent() -> COLORREF {
    thread_local! { static ACCENT: OnceCell<COLORREF> = const { OnceCell::new() }; }
    ACCENT.with(|accent| {
        *accent.get_or_init(|| {
            let mut argb = 0;
            let mut opaque = Default::default();
            if unsafe { DwmGetColorizationColor(&mut argb, &mut opaque) }.is_ok() {
                COLORREF(((argb & 0xff) << 16) | (argb & 0xff00) | ((argb >> 16) & 0xff))
            } else {
                COLORREF(0x00FF_C078)
            }
        })
    })
}

pub fn surface(dc: HDC, (left, top, right, bottom): (i32, i32, i32, i32), fill: COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(fill);
        let pen = CreatePen(PS_SOLID, 1, COLORREF(0x0044_4444));
        let old_brush = SelectObject(dc, brush.into());
        let old_pen = SelectObject(dc, pen.into());
        let _ = RoundRect(dc, left, top, right, bottom, 10, 10);
        let _ = SelectObject(dc, old_pen);
        let _ = SelectObject(dc, old_brush);
        let _ = DeleteObject(pen.into());
        let _ = DeleteObject(brush.into());
    }
}

pub fn caption(dc: HDC, bounds: (i32, i32, i32, i32), text: &str) {
    label(dc, bounds, text, COLORREF(0x00F3_F3F3));
}

pub fn label(
    dc: HDC,
    (left, top, right, bottom): (i32, i32, i32, i32),
    text: &str,
    color: COLORREF,
) {
    let mut text: Vec<u16> = text.encode_utf16().collect();
    let mut bounds = RECT {
        left,
        top,
        right,
        bottom,
    };
    unsafe {
        let old_mode = SetBkMode(dc, TRANSPARENT);
        let old_color = SetTextColor(dc, color);
        let _ = DrawTextW(
            dc,
            &mut text,
            &mut bounds,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        let _ = SetTextColor(dc, old_color);
        let _ = SetBkMode(
            dc,
            windows::Win32::Graphics::Gdi::BACKGROUND_MODE(old_mode as u32),
        );
    }
}
