//! The viewer window. It is created from the loaded state, hands every message to the module that
//! understands it, and keeps the two timers that drive the slideshow and animated pictures.

use std::path::PathBuf;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::Graphics::Gdi::{ScreenToClient, HBRUSH, HDC};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    LVN_BEGINDRAG, LVN_GETDISPINFOW, LVN_ODFINDITEMW, NMHDR, NMITEMACTIVATE, NMLISTVIEW, NM_CLICK,
    NM_CUSTOMDRAW, NM_DBLCLK, NM_RCLICK,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_CONTROL,
};
use windows::Win32::UI::Shell::{DragAcceptFiles, DragFinish, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetSystemMetrics,
    GetWindowLongPtrW, IsIconic, IsZoomed, KillTimer, LoadIconW, LoadImageW, PostMessageW,
    PostQuitMessage, RegisterClassExW, SetTimer, SetWindowLongPtrW, SetWindowPos, SetWindowTextW,
    ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, GWL_STYLE, HICON,
    IMAGE_ICON, LR_SHARED, MSG, SM_CXSMICON, SM_CYSMICON, SWP_FRAMECHANGED, SWP_NOZORDER,
    SW_MAXIMIZE, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_COMMAND, WM_CREATE, WM_CTLCOLORBTN,
    WM_CTLCOLOREDIT, WM_CTLCOLORSTATIC, WM_DESTROY, WM_DROPFILES, WM_ENTERSIZEMOVE, WM_ERASEBKGND,
    WM_EXITSIZEMOVE, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_NCHITTEST, WM_NOTIFY, WM_PAINT, WM_RBUTTONUP, WM_SIZE, WM_SYSKEYDOWN, WM_TIMER,
    WM_WINDOWPOSCHANGED, WNDCLASSEXW, WS_CAPTION, WS_CLIPCHILDREN, WS_EX_TOPMOST,
    WS_OVERLAPPEDWINDOW, WS_POPUP, WS_VISIBLE,
};

use crate::bar;
use crate::dialog::{self, Dialog, BASE_ID, PROMPT_ID};
use crate::playlist;
use crate::{input, paint, state, tools, w32};

const CLASS: &str = "RustedGlassWindow";
const TITLE: &str = "RustedGlass";
const SLIDESHOW_TIMER: usize = 1;
const FRAME_TIMER: usize = 2;
const NAV_TIMER: usize = 3;
const NOTICE_TIMER: usize = 4;

/// Opens the window on the given state and runs its message loop until the window is closed.
pub fn run(config_path: PathBuf, args: &[String]) -> Result<(), String> {
    let mut loaded = state::State::new(config_path, args);
    let startup_notice = loaded.notice.take();
    let old_config = loaded.config.incompatible_user_path.clone();
    let starts_fullscreen = loaded.knobs.start_fullscreen;
    let starts_slideshow = loaded.knobs.slideshow;

    state::install(loaded);

    let hwnd = unsafe { create_window()? };
    if starts_fullscreen {
        set_fullscreen(hwnd, true);
    }
    if starts_slideshow {
        // `EnableSlideshow` starts the loop with the window, as the C# viewer does.
        input::apply_action(hwnd, tools::Action::StartSlideshow);
    }
    if let Some(notice) = startup_notice {
        state::with_state(|state| state.notice = Some(notice));
        expire_notice(hwnd);
        unsafe { w32::invalidate(hwnd) };
    }
    if let Some(path) = old_config {
        use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONWARNING, MB_OK};
        let text = w32::wide(&format!(
            "{} uses an older settings format. RustedGlass is using defaults and will preserve this file. To save settings, choose another file with --config.",
            path.display()
        ));
        let _ = unsafe {
            MessageBoxW(
                Some(hwnd),
                PCWSTR(text.as_ptr()),
                windows::core::w!("Older settings file"),
                MB_OK | MB_ICONWARNING,
            )
        };
    }
    unsafe { pump() }
    Ok(())
}

/// Repaints, syncs playback timers and matches the tool bar's buttons to the state: the one place
/// that says "something changed, show it again".
pub fn refresh(hwnd: HWND) {
    unsafe {
        update_title(hwnd);
        timers::sync(hwnd);
        layout_controls(hwnd);
        let unreadable = state::with_state(|state| {
            if state.playlist_visible {
                playlist::refresh(&state.photos);
            }
            std::mem::take(&mut state.unreadable_notice_pending)
        });
        if unreadable == Some(true) {
            expire_notice(hwnd);
        }
        w32::invalidate(hwnd);
    }
}

fn layout_controls(hwnd: HWND) {
    let (width, height) = unsafe { w32::client_size(hwnd) };
    playlist::resize(width as i32, height as i32);
    state::with_state(|state| {
        if state.dialog == Dialog::Shortcuts {
            dialog::resize_shortcuts((width, height));
        }
        bar::sync(hwnd, state, &state.config, width as i32);
    });
}

unsafe fn arm(hwnd: HWND, id: usize, delay: u32) {
    if delay == 0 {
        let _ = unsafe { KillTimer(Some(hwnd), id) };
    } else {
        let _ = unsafe { SetTimer(Some(hwnd), id, delay, None) };
    }
}

/// The title bar names the shown file and where it sits in the list.
unsafe fn update_title(hwnd: HWND) {
    state::with_state(|state| {
        let title = match state.photos.current() {
            Some(path) => format!(
                "{TITLE} — {} ({}/{})",
                w32::file_name(path),
                state.photos.index + 1,
                state.photos.len()
            ),
            None => TITLE.to_string(),
        };
        let _ = unsafe { SetWindowTextW(hwnd, PCWSTR(w32::wide(&title).as_ptr())) };
    });
}

unsafe fn create_window() -> Result<HWND, String> {
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }
        .map_err(|error| error.to_string())?
        .into();
    let name = w32::wide(CLASS);
    let cursor = unsafe {
        windows::Win32::UI::WindowsAndMessaging::LoadCursorW(
            None,
            windows::Win32::UI::WindowsAndMessaging::IDC_ARROW,
        )
    }
    .map_err(|error| format!("could not load the default cursor: {error}"))?;
    let icon_resource = PCWSTR(101usize as *const u16);
    let icon = unsafe { LoadIconW(Some(instance), icon_resource) }
        .map_err(|error| format!("could not load the application icon: {error}"))?;
    let small_icon = unsafe {
        LoadImageW(
            Some(instance),
            icon_resource,
            IMAGE_ICON,
            GetSystemMetrics(SM_CXSMICON),
            GetSystemMetrics(SM_CYSMICON),
            LR_SHARED,
        )
    }
    .map(|image| HICON(image.0))
    .unwrap_or(icon);
    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        hIcon: icon,
        hIconSm: small_icon,
        hCursor: cursor,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wndproc),
        hInstance: instance,
        hbrBackground: HBRUSH::default(),
        lpszClassName: PCWSTR(name.as_ptr()),
        ..Default::default()
    };
    let _ = unsafe { RegisterClassExW(&class) };

    let start = state::with_state(|state| {
        (
            frame_style(state.is_borderless),
            state.knobs.start_maximized,
            state.knobs.topmost,
            frame::startup_bounds(state.config.window_bounds()),
        )
    });
    let (style, maximized, topmost, (x, y, width, height)) =
        start.ok_or("viewer state disappeared")?;
    let hwnd = unsafe {
        CreateWindowExW(
            if topmost {
                WS_EX_TOPMOST
            } else {
                WINDOW_EX_STYLE::default()
            },
            PCWSTR(name.as_ptr()),
            PCWSTR(w32::wide(TITLE).as_ptr()),
            style | WS_VISIBLE,
            x,
            y,
            width,
            height,
            None,
            None,
            Some(instance),
            None,
        )
    }
    .map_err(|error| format!("could not create the window: {error}"))?;
    state::with_state(|state| state.normal_rect = unsafe { w32::window_rect(hwnd) });
    let _ = unsafe { ShowWindow(hwnd, if maximized { SW_MAXIMIZE } else { SW_SHOW }) };
    // The window is dark, so its title bar is asked to be dark too.
    let dark = 1i32;
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark as *const i32 as *const core::ffi::c_void,
            4,
        )
    };
    Ok(hwnd)
}

unsafe fn pump() {
    let mut message = MSG::default();
    unsafe {
        while GetMessageW(&mut message, None, 0, 0).0 != 0 {
            if matches!(message.message, WM_KEYDOWN | WM_SYSKEYDOWN)
                && message.wParam.0 == 0x79
                && message.lParam.0 & (1 << 29) == 0
            {
                if message.lParam.0 & (1 << 30) == 0 {
                    let root = windows::Win32::UI::WindowsAndMessaging::GetAncestor(
                        message.hwnd,
                        windows::Win32::UI::WindowsAndMessaging::GA_ROOT,
                    );
                    input::apply_action(root, tools::Action::ToggleBorderless);
                }
                continue;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

pub fn navigate(hwnd: HWND, delta: i32) {
    state::with_state(|state| {
        if state.step(delta) {
            state.navigation_pending = true;
        }
    });
    refresh_navigation(hwnd);
}

pub fn refresh_navigation(hwnd: HWND) {
    if state::with_state(|state| state.navigation_pending) == Some(true) {
        unsafe {
            arm(hwnd, NAV_TIMER, 85);
        }
    }
    refresh(hwnd);
}

pub fn expire_notice(hwnd: HWND) {
    unsafe {
        arm(hwnd, NOTICE_TIMER, 2800);
    }
    playlist::invalidate();
}

mod messages;
use messages::wndproc;

mod settings;
use settings::{confirm_prompt, pick_folder, press_apply, toggle_setting};

mod frame;
use frame::frame_style;
pub use frame::{set_borderless, set_fullscreen};

mod timers;

#[cfg(test)]
mod drop_tests;
