//! The two things the list view control cannot do on its own: paint its rows (it would paint
//! black text on the dark background the playlist is given) and answer a drag of one row onto
//! another, which is how the order of the playlist gets changed by hand.

use std::cell::Cell;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, FillRect, HBRUSH};
use windows::Win32::UI::Controls::{
    CDDS_ITEMPREPAINT, CDDS_POSTPAINT, CDDS_PREPAINT, CDRF_DODEFAULT, CDRF_NEWFONT,
    CDRF_NOTIFYITEMDRAW, CDRF_NOTIFYPOSTPAINT, LIST_VIEW_ITEM_STATE_FLAGS, LVHITTESTINFO,
    LVIS_FOCUSED, LVIS_SELECTED, LVITEMW, LVM_HITTEST, LVM_SETITEMSTATE,
    NMCUSTOMDRAW_DRAW_STATE_FLAGS, NMLVCUSTOMDRAW, NM_CUSTOMDRAW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetCapture, GetKeyState, ReleaseCapture, SetCapture, SetFocus, VIRTUAL_KEY, VK_CONTROL,
    VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_LEFT, VK_RIGHT, VK_UP,
};
use windows::Win32::UI::Shell::DefSubclassProc;
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetParent, PostMessageW, SendMessageW, WM_APP, WM_CANCELMODE, WM_CAPTURECHANGED,
    WM_DROPFILES, WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCDESTROY,
};

use crate::dialog::PANEL_COLOR;
use crate::{input, playlist, state, w32};

thread_local! {
    // Capture owner, source row, and the optional preview window.
    static DRAG: Cell<Option<(HWND, i32, Option<HWND>)>> = const { Cell::new(None) };
    /// Brushes for the two row colours, made once each. The Rust runtime does not free GDI
    /// objects, and there are never more than two.
    static ROW_BRUSHES: [Cell<HBRUSH>; 2] =
        const { [const { Cell::new(HBRUSH(std::ptr::null_mut())) }; 2] };
}

/// Takes the list's mouse messages over. `SetWindowSubclass` keeps the procedure for the life of
/// the window, and the playlist window is never destroyed while the app runs.
pub fn install(list: HWND) {
    unsafe {
        let _ = windows::Win32::UI::Shell::SetWindowSubclass(list, Some(list_proc), 1, 0);
    }
}

pub fn end_drag() {
    let Some((hwnd, _, preview)) = DRAG.take() else {
        return;
    };
    unsafe {
        if let Some(preview) = preview {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(preview);
        }
        if GetCapture() == hwnd {
            let _ = ReleaseCapture();
        }
    }
}

const START_DRAG: u32 = WM_APP + 2;

pub fn begin_drag(hwnd: HWND, row: i32) {
    if state::read_state(|state| state.browsing_import_preview()) == Some(true) {
        return;
    }
    if row >= 0 {
        // The list can release its own capture after returning from LVN_BEGINDRAG.
        let _ = unsafe { PostMessageW(Some(hwnd), START_DRAG, WPARAM(row as usize), LPARAM(0)) };
    }
}

/// The custom draw of the list. Every row is painted here. A selected row takes the system
/// highlight colours, so it stays readable whatever the system theme is.
pub fn custom_draw(lparam: LPARAM) -> LRESULT {
    unsafe {
        let draw = &mut *(lparam.0 as *mut NMLVCUSTOMDRAW);
        if draw.nmcd.hdr.code != NM_CUSTOMDRAW {
            return LRESULT(0);
        }
        match draw.nmcd.dwDrawStage {
            CDDS_PREPAINT => LRESULT((CDRF_NOTIFYITEMDRAW | CDRF_NOTIFYPOSTPAINT) as isize),
            CDDS_POSTPAINT => {
                state::with_state(|state| {
                    if let Some(notice) = &state.notice {
                        crate::paint::status(
                            draw.nmcd.hdc,
                            w32::client_size(draw.nmcd.hdr.hwndFrom),
                            notice,
                        );
                    }
                });
                LRESULT(CDRF_DODEFAULT as isize)
            }
            CDDS_ITEMPREPAINT => {
                let list = draw.nmcd.hdr.hwndFrom;
                let selected = draw
                    .nmcd
                    .uItemState
                    .contains(NMCUSTOMDRAW_DRAW_STATE_FLAGS(LVIS_SELECTED.0));
                let background = if selected {
                    crate::style::accent()
                } else if draw.nmcd.dwItemSpec % 2 == 0 {
                    PANEL_COLOR
                } else {
                    COLORREF(0x0021_2121)
                };
                let rgb = background.0;
                let light = ((rgb & 255) * 3 + ((rgb >> 8) & 255) * 6 + ((rgb >> 16) & 255)) > 1400;
                draw.clrText = if selected && light {
                    COLORREF(0)
                } else {
                    COLORREF(0x00F3_F3F3)
                };
                draw.clrTextBk = background;
                // `nmcd.rc` is only the label of the row, so the row is filled from edge to edge
                // of the control instead.
                let mut bounds = RECT::default();
                let _ = GetClientRect(list, &mut bounds);
                let row = RECT {
                    left: bounds.left,
                    top: draw.nmcd.rc.top,
                    right: bounds.right,
                    bottom: draw.nmcd.rc.bottom,
                };
                let _ = FillRect(draw.nmcd.hdc, &row, row_brush(background));
                LRESULT(CDRF_NEWFONT as isize)
            }
            _ => LRESULT(CDRF_DODEFAULT as isize),
        }
    }
}

fn row_brush(color: COLORREF) -> HBRUSH {
    ROW_BRUSHES.with(|brushes| {
        let brush = &brushes[usize::from(color != PANEL_COLOR)];
        if brush.get().0.is_null() {
            brush.set(unsafe { CreateSolidBrush(color) });
        }
        brush.get()
    })
}

/// The list answers its own mouse messages: a row pressed and moved past the threshold is put
/// where the pointer is over another row. Everything else is the control's own business.
unsafe extern "system" fn list_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    unsafe {
        match message {
            windows::Win32::UI::WindowsAndMessaging::WM_NOTIFY => {
                if let Some(result) = playlist::header_notify(hwnd, lparam) {
                    return result;
                }
            }
            WM_KEYDOWN if wparam.0 == VK_ESCAPE.0 as usize && DRAG.get().is_some() => {
                end_drag();
                return LRESULT(0);
            }
            WM_KEYDOWN => {
                let key = VIRTUAL_KEY(wparam.0 as u16);
                let list_navigation =
                    matches!(key, VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN | VK_HOME | VK_END);
                let ctrl = GetKeyState(i32::from(VK_CONTROL.0)) < 0;
                if !list_navigation
                    && input::on_key(wparam.0 as u32, ctrl, input::shift_held(lparam)).is_some()
                {
                    if let Ok(parent) = GetParent(hwnd) {
                        return SendMessageW(parent, message, Some(wparam), Some(lparam));
                    }
                }
            }
            START_DRAG => {
                end_drag();
                let _ = SetCapture(hwnd);
                if GetCapture() == hwnd {
                    let preview =
                        playlist::row_name(wparam.0).and_then(|name| preview::open(hwnd, &name));
                    DRAG.set(Some((hwnd, wparam.0 as i32, preview)));
                }
                return LRESULT(0);
            }
            WM_MOUSEMOVE if DRAG.get().is_some() => {
                if let Some((_, _, Some(preview))) = DRAG.get() {
                    preview::follow(preview);
                }
                return LRESULT(0);
            }
            WM_LBUTTONUP if DRAG.get().is_some() => {
                let row = DRAG.get().map_or(-1, |(_, row, _)| row);
                end_drag();
                let target = row_under(hwnd, lparam);
                if target >= 0 && target != row {
                    move_row(row, target);
                    if let Ok(parent) = GetParent(hwnd) {
                        crate::view::refresh(parent);
                    }
                }
                // A drag that moved nothing (or ended outside the rows) still selected the pressed
                // row natively; put the selection back on the picture so the two agree.
                select_current(hwnd);
                if let Ok(parent) = GetParent(hwnd) {
                    let _ = SetFocus(Some(parent));
                }
                return LRESULT(0);
            }
            WM_CAPTURECHANGED | WM_CANCELMODE | WM_NCDESTROY => end_drag(),
            WM_DROPFILES => {
                if let Ok(parent) = GetParent(hwnd) {
                    // The drop point belongs to the list's client area, so mark the
                    // forwarded message explicitly for the parent.
                    return SendMessageW(parent, message, Some(wparam), Some(LPARAM(1)));
                }
            }
            _ => {}
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }
}

unsafe fn row_under(hwnd: HWND, lparam: LPARAM) -> i32 {
    let mut hit = LVHITTESTINFO {
        pt: POINT {
            x: w32::pointer_x(lparam),
            y: w32::pointer_y(lparam),
        },
        ..Default::default()
    };
    SendMessageW(
        hwnd,
        LVM_HITTEST,
        None,
        Some(LPARAM(&mut hit as *mut _ as isize)),
    )
    .0 as i32
}

/// Puts the native selection back on the shown row, without changing the picture. The list
/// selects whatever a drag was pressed on, which may not be the row the viewer shows.
fn select_current(hwnd: HWND) {
    let Some(row) =
        state::with_state(|state| state.photos.current().map(|_| state.photos.index)).flatten()
    else {
        return;
    };
    let mask = LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0);
    let mut item = LVITEMW {
        stateMask: mask,
        ..Default::default()
    };
    unsafe {
        let _ = SendMessageW(
            hwnd,
            LVM_SETITEMSTATE,
            Some(WPARAM(usize::MAX)),
            Some(LPARAM(&item as *const _ as isize)),
        );
        item.state = mask;
        let _ = SendMessageW(
            hwnd,
            LVM_SETITEMSTATE,
            Some(WPARAM(row)),
            Some(LPARAM(&item as *const _ as isize)),
        );
    }
}

/// Puts the row `from` where the row `to` is, and shows the list in that order. The shown picture
/// keeps its row: only the order changes.
fn move_row(from: i32, to: i32) {
    state::with_state(|state| {
        state.photos.move_row(from as usize, to as usize);
    });
    playlist::refresh(None);
}

mod preview;
