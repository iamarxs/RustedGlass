//! Paint the native header while leaving its sizing, hit testing, and text model intact.

use windows::core::PWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    DrawTextW, FillRect, GetStockObject, SetBkMode, SetDCBrushColor, SetTextColor, DC_BRUSH,
    DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, HBRUSH, HDC, TRANSPARENT,
};
use windows::Win32::UI::Controls::{
    CDDS_ITEMPREPAINT, CDDS_POSTPAINT, CDDS_PREPAINT, CDIS_HOT, CDIS_SELECTED, CDRF_NOTIFYITEMDRAW,
    CDRF_NOTIFYPOSTPAINT, CDRF_SKIPDEFAULT, HDF_SORTDOWN, HDF_SORTUP, HDITEMW, HDI_FORMAT,
    HDI_TEXT, HDM_GETITEMCOUNT, HDM_GETITEMRECT, HDM_GETITEMW, LVM_GETHEADER, NMCUSTOMDRAW, NMHDR,
    NM_CUSTOMDRAW,
};
use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, SendMessageW};

const BACKGROUND: COLORREF = COLORREF(0x0025_2525);
const DIVIDER: COLORREF = COLORREF(0x0044_4444);
const TEXT: COLORREF = COLORREF(0x00F3_F3F3);

pub(crate) unsafe fn header_notify(list: HWND, lparam: LPARAM) -> Option<LRESULT> {
    let notification = unsafe { (lparam.0 as *const NMHDR).as_ref() }?;
    if notification.code != NM_CUSTOMDRAW {
        return None;
    }
    let header = HWND(unsafe { SendMessageW(list, LVM_GETHEADER, None, None) }.0 as *mut _);
    if header.0.is_null() || notification.hwndFrom != header {
        return None;
    }
    let draw = unsafe { &*(lparam.0 as *const NMCUSTOMDRAW) };
    Some(match draw.dwDrawStage {
        CDDS_PREPAINT => LRESULT((CDRF_NOTIFYITEMDRAW | CDRF_NOTIFYPOSTPAINT) as isize),
        CDDS_ITEMPREPAINT => {
            if draw_item(header, draw) {
                LRESULT(CDRF_SKIPDEFAULT as isize)
            } else {
                LRESULT(0)
            }
        }
        CDDS_POSTPAINT => {
            paint_gap(header, draw.hdc);
            LRESULT(0)
        }
        _ => LRESULT(0),
    })
}

fn draw_item(header: HWND, draw: &NMCUSTOMDRAW) -> bool {
    let mut label = [0u16; 64];
    let mut item = HDITEMW {
        mask: HDI_TEXT | HDI_FORMAT,
        pszText: PWSTR(label.as_mut_ptr()),
        cchTextMax: label.len() as i32,
        ..Default::default()
    };
    if unsafe {
        SendMessageW(
            header,
            HDM_GETITEMW,
            Some(WPARAM(draw.dwItemSpec)),
            Some(LPARAM(&mut item as *mut _ as isize)),
        )
    }
    .0 == 0
    {
        return false;
    }

    let background = if (draw.uItemState & CDIS_SELECTED).0 != 0 {
        crate::style::PRESSED
    } else if (draw.uItemState & CDIS_HOT).0 != 0 {
        crate::style::HOVER
    } else {
        BACKGROUND
    };
    fill(draw.hdc, &draw.rc, background);
    let mut divider = draw.rc;
    divider.left = divider.right - 1;
    fill(draw.hdc, &divider, DIVIDER);

    let mut text = draw.rc;
    let height = (text.bottom - text.top).max(1);
    text.left += height / 3;
    text.right -= height / 3;
    let mut arrow = if item.fmt.0 & HDF_SORTDOWN.0 != 0 {
        ['▼' as u16]
    } else {
        ['▲' as u16]
    };
    let has_arrow = item.fmt.0 & (HDF_SORTUP.0 | HDF_SORTDOWN.0) != 0;
    let color = if has_arrow {
        crate::dialog::TEXT_HEADING
    } else {
        TEXT
    };
    let format = DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX;
    let length = label
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(label.len());
    unsafe {
        let mode = SetBkMode(draw.hdc, TRANSPARENT);
        let previous = SetTextColor(draw.hdc, color);
        if has_arrow && text.right - text.left >= height {
            let mut glyph = text;
            glyph.left = glyph.right - height / 2;
            text.right = glyph.left - height / 4;
            let _ = DrawTextW(draw.hdc, &mut arrow, &mut glyph, format);
        }
        if text.right > text.left {
            let _ = DrawTextW(draw.hdc, &mut label[..length], &mut text, format);
        }
        let _ = SetTextColor(draw.hdc, previous);
        let _ = SetBkMode(
            draw.hdc,
            windows::Win32::Graphics::Gdi::BACKGROUND_MODE(mode as u32),
        );
    }
    true
}

fn paint_gap(header: HWND, dc: HDC) {
    let mut gap = RECT::default();
    if unsafe { GetClientRect(header, &mut gap) }.is_err() {
        return;
    }
    let count = unsafe { SendMessageW(header, HDM_GETITEMCOUNT, None, None) }.0;
    if count > 0 {
        let mut last = RECT::default();
        let done = unsafe {
            SendMessageW(
                header,
                HDM_GETITEMRECT,
                Some(WPARAM(count as usize - 1)),
                Some(LPARAM(&mut last as *mut _ as isize)),
            )
        };
        if done.0 != 0 {
            gap.left = last.right.max(gap.left);
        }
    }
    if gap.left < gap.right {
        fill(dc, &gap, BACKGROUND);
    }
}

fn fill(dc: HDC, bounds: &RECT, color: COLORREF) {
    unsafe {
        let brush = HBRUSH(GetStockObject(DC_BRUSH).0);
        let previous = SetDCBrushColor(dc, color);
        let _ = FillRect(dc, bounds, brush);
        let _ = SetDCBrushColor(dc, previous);
    }
}
