use std::cell::RefCell;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::Controls::{
    TOOLTIPS_CLASSW, TTF_IDISHWND, TTF_SUBCLASS, TTM_ADDTOOLW, TTS_NOPREFIX, TTTOOLINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, SendMessageW, WINDOW_STYLE, WS_EX_TOPMOST, WS_POPUP,
};

thread_local! {
    static TOOLTIP: RefCell<Option<(HWND, Vec<Vec<u16>>)>> = const { RefCell::new(None) };
}

pub(super) fn create(parent: HWND) {
    let tooltip = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST,
            TOOLTIPS_CLASSW,
            PCWSTR::null(),
            WS_POPUP | WINDOW_STYLE(TTS_NOPREFIX),
            0,
            0,
            0,
            0,
            Some(parent),
            None,
            Some(crate::w32::instance()),
            None,
        )
    };
    if let Ok(tooltip) = tooltip {
        TOOLTIP.replace(Some((tooltip, Vec::new())));
    }
}

pub(super) fn add(parent: HWND, button: HWND, mut label: Vec<u16>) {
    let Some(tooltip) = TOOLTIP.with(|stored| stored.borrow().as_ref().map(|(handle, _)| *handle))
    else {
        return;
    };
    let info = TTTOOLINFOW {
        cbSize: size_of::<TTTOOLINFOW>() as u32,
        uFlags: TTF_IDISHWND | TTF_SUBCLASS,
        hwnd: parent,
        uId: button.0 as usize,
        lpszText: PWSTR(label.as_mut_ptr()),
        ..Default::default()
    };
    let added = unsafe {
        SendMessageW(
            tooltip,
            TTM_ADDTOOLW,
            None,
            Some(LPARAM(&info as *const _ as isize)),
        )
    };
    if added.0 != 0 {
        // Keep the native tool's text alive until its tooltip window is destroyed.
        TOOLTIP.with(|stored| {
            if let Some((_, labels)) = stored.borrow_mut().as_mut() {
                labels.push(label);
            }
        });
    }
}

pub(super) fn destroy() {
    if let Some((tooltip, _labels)) = TOOLTIP.take() {
        let _ = unsafe { DestroyWindow(tooltip) };
    }
}
