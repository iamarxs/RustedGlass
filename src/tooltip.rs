use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::UI::Controls::{
    TOOLTIPS_CLASSW, TOOLTIP_FLAGS, TTF_IDISHWND, TTF_SUBCLASS, TTM_ADDTOOLW, TTM_SETMAXTIPWIDTH,
    TTS_NOPREFIX, TTTOOLINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, SendMessageW, WINDOW_STYLE, WS_EX_TOPMOST, WS_POPUP,
};

pub struct Tooltip {
    window: HWND,
    labels: Vec<Vec<u16>>,
}

impl Tooltip {
    pub fn new(parent: HWND, max_width: Option<i32>) -> Option<Self> {
        let window = unsafe {
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
        }
        .ok()?;
        if let Some(width) = max_width {
            let _ = unsafe {
                SendMessageW(
                    window,
                    TTM_SETMAXTIPWIDTH,
                    None,
                    Some(LPARAM(width as isize)),
                )
            };
        }
        Some(Self {
            window,
            labels: Vec::new(),
        })
    }

    pub fn add_control(&mut self, parent: HWND, control: HWND, label: &str) {
        self.add(
            parent,
            control.0 as usize,
            TTF_IDISHWND,
            RECT::default(),
            label,
        );
    }

    pub fn add_area(&mut self, parent: HWND, id: usize, rect: RECT, label: &str) {
        self.add(parent, id, TOOLTIP_FLAGS(0), rect, label);
    }

    fn add(&mut self, parent: HWND, id: usize, flags: TOOLTIP_FLAGS, rect: RECT, label: &str) {
        let mut label = crate::w32::wide(label);
        let info = TTTOOLINFOW {
            cbSize: size_of::<TTTOOLINFOW>() as u32,
            uFlags: TTF_SUBCLASS | flags,
            hwnd: parent,
            uId: id,
            rect,
            lpszText: PWSTR(label.as_mut_ptr()),
            ..Default::default()
        };
        let added = unsafe {
            SendMessageW(
                self.window,
                TTM_ADDTOOLW,
                None,
                Some(LPARAM(&info as *const _ as isize)),
            )
        };
        if added.0 != 0 {
            self.labels.push(label);
        }
    }
}

impl Drop for Tooltip {
    fn drop(&mut self) {
        let _ = unsafe { DestroyWindow(self.window) };
    }
}
