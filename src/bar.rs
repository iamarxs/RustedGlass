//! The tool bar: real `BUTTON` child windows, one per action.
//!
//! They are children of the viewer window, so Windows draws them and they take focus like any
//! other control. The list of buttons comes from
//! `tools`, so the bar, the menu and the keys all offer the same actions.

use std::cell::{Cell, RefCell};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::HGDIOBJ;
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, ODS_DISABLED, ODS_FOCUS, ODS_SELECTED};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, SendMessageW, BS_OWNERDRAW, HMENU, WINDOW_STYLE, WM_MOUSEMOVE,
    WM_SETFONT, WS_CHILD, WS_DISABLED, WS_TABSTOP, WS_VISIBLE,
};

use crate::config::Config;
use crate::dialog::Dialog;
use crate::state::State;
use crate::tools::{self, Action, Item};
use crate::w32;

/// Control ids of the bar's buttons, well away from the panel's ids so a `WM_COMMAND` says which
/// of the two it came from.
pub const TOOL_BASE: usize = 2000;
/// The buttons are as high as they can be inside the band.
const WM_MOUSELEAVE: u32 = 0x02A3;
const BUTTON_HEIGHT: i32 = 30;

thread_local! {
    /// `(control id, action, checked, enabled, window)` for the buttons on the bar, in the order
    /// they were made.
    static BUTTONS: RefCell<Vec<(usize, Action, bool, bool, HWND)>> = const { RefCell::new(Vec::new()) };
    /// Where the buttons are and what is on them: the layout they were made from.
    static HOVERED: Cell<HWND> = const { Cell::new(HWND(std::ptr::null_mut())) };
    static LAYOUT: RefCell<Vec<(i32, i32, String)>> = const { RefCell::new(Vec::new()) };
}

/// Make the bar match the state. Repaints call this, so a layout that has not changed is left
/// alone: only the checked state and the enable of an existing button are updated, and only when
/// they differ. That is what keeps the bar from flickering while the pointer moves over it.
pub fn sync(hwnd: HWND, state: &State, config: &Config, width: i32) {
    let wanted = if state.toolbar_visible && state.dialog == Dialog::None {
        tools::bar_layout(state, config, width)
    } else {
        Vec::new()
    };
    let places: Vec<(i32, i32, String)> = wanted
        .iter()
        .map(|(x, buttons, item)| (*x, *buttons, item.label.clone()))
        .collect();
    if LAYOUT.with(|stored| *stored.borrow() != places) {
        rebuild(hwnd, &wanted);
        LAYOUT.with(|stored| *stored.borrow_mut() = places);
    }
    let updates: Vec<_> = BUTTONS.with(|stored| {
        stored
            .borrow_mut()
            .iter_mut()
            .filter_map(|(id, _, checked, enabled, handle)| {
                let item = &wanted.get(*id - TOOL_BASE)?.2;
                let enable_changed = *enabled != item.enabled;
                if !enable_changed && *checked == item.checked {
                    return None;
                }
                *checked = item.checked;
                *enabled = item.enabled;
                Some((*handle, enable_changed.then_some(item.enabled)))
            })
            .collect()
    });
    // EnableWindow can redraw an owner-drawn button before returning.
    for (handle, enabled) in updates {
        unsafe {
            if let Some(enabled) = enabled {
                let _ = EnableWindow(handle, enabled);
            }
            w32::invalidate(handle);
        }
    }
}

/// Which action a bar button carries, from the id a `WM_COMMAND` reported.
pub fn action(id: usize) -> Option<Action> {
    BUTTONS.with(|stored| {
        stored
            .borrow()
            .iter()
            .find(|(control, ..)| *control == id)
            .map(|(_, action, ..)| *action)
    })
}

/// Take the bar's buttons away. Done before a panel is opened so that its controls are the only
/// children on the window, and whenever the bar is hidden.
pub fn destroy() {
    tooltip::destroy();
    let handles = BUTTONS.with(|stored| stored.replace(Vec::new()));
    for (_, _, _, _, handle) in handles {
        unsafe {
            let _ = DestroyWindow(handle);
        }
    }
    LAYOUT.with(|stored| stored.borrow_mut().clear());
}

fn rebuild(hwnd: HWND, layout: &[(i32, i32, Item)]) {
    destroy();
    let HGDIOBJ(font) = crate::style::font();
    let instance = w32::instance();
    if !layout.is_empty() {
        tooltip::create(hwnd);
    }
    for (index, (x, buttons, item)) in layout.iter().enumerate() {
        let text = w32::wide(&item.label);
        let mut style = WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32);
        if !item.enabled {
            style |= WS_DISABLED;
        }
        let Ok(handle) = (unsafe {
            CreateWindowExW(
                Default::default(),
                PCWSTR(w32::wide("BUTTON").as_ptr()),
                PCWSTR(text.as_ptr()),
                style,
                *x,
                (tools::BAND_HEIGHT - BUTTON_HEIGHT) / 2,
                *buttons,
                BUTTON_HEIGHT,
                Some(hwnd),
                Some(HMENU((TOOL_BASE + index) as *mut core::ffi::c_void)),
                Some(instance),
                None,
            )
        }) else {
            continue;
        };
        unsafe {
            let _ = SendMessageW(
                handle,
                WM_SETFONT,
                Some(WPARAM(font as usize)),
                Some(LPARAM(0)),
            );
            // Visual styles would paint the button in its own colours and ignore the panel brush
            // asked for in `WM_CTLCOLORBUTTON`.
            let _ = SetWindowSubclass(handle, Some(button_proc), 1, 0);
        }
        BUTTONS.with(|stored| {
            stored.borrow_mut().push((
                TOOL_BASE + index,
                item.action,
                item.checked,
                item.enabled,
                handle,
            ))
        });
        tooltip::add(hwnd, handle, text);
        unsafe { w32::invalidate(handle) };
    }
}

pub unsafe fn draw(lparam: LPARAM) -> bool {
    let item = unsafe { &*(lparam.0 as *const DRAWITEMSTRUCT) };
    let button = BUTTONS.with(|buttons| {
        buttons.try_borrow().ok().and_then(|buttons| {
            buttons
                .iter()
                .find(|entry| entry.4 == item.hwndItem)
                .map(|entry| (entry.1, entry.2))
        })
    });
    let Some((action, checked)) = button else {
        return false;
    };
    let rect = item.rcItem;
    let fill = if item.itemState.0 & ODS_SELECTED.0 != 0 {
        crate::style::PRESSED
    } else if HOVERED.get() == item.hwndItem {
        crate::style::HOVER
    } else {
        crate::style::CONTROL
    };
    unsafe {
        w32::fill_rect(
            item.hDC,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            crate::paint::BACKGROUND,
        );
        crate::style::surface(
            item.hDC,
            (rect.left, rect.top, rect.right, rect.bottom),
            fill,
        );
        if checked || item.itemState.0 & ODS_FOCUS.0 != 0 {
            w32::fill_rect(
                item.hDC,
                rect.left + 6,
                rect.bottom - 3,
                rect.right - rect.left - 12,
                2,
                crate::style::accent(),
            );
        }
        let ink = if item.itemState.0 & ODS_DISABLED.0 != 0 {
            COLORREF(0x0088_8888)
        } else {
            COLORREF(0x00F3_F3F3)
        };
        icons::draw(item.hDC, rect, action, fill, ink);
    }
    true
}

unsafe extern "system" fn button_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    unsafe {
        if msg == WM_MOUSEMOVE && HOVERED.get() != hwnd {
            HOVERED.set(hwnd);
            let _ = TrackMouseEvent(&mut TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            });
            w32::invalidate(hwnd);
        } else if msg == WM_MOUSELEAVE && HOVERED.get() == hwnd {
            HOVERED.set(HWND(std::ptr::null_mut()));
            w32::invalidate(hwnd);
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}

mod icons;
mod tooltip;
