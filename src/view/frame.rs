use super::*;
/// Full screen: the frame goes away and the window covers the screen, the way the C# viewer does
/// `ToggleWindowStyle` does it. Turning it off goes back to the rectangle remembered on the way in.
pub fn set_fullscreen(hwnd: HWND, on: bool) {
    unsafe {
        state::with_state(|state| {
            if on == state.is_fullscreen {
                return;
            }
            if on {
                use windows::Win32::Graphics::Gdi::{
                    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
                };
                let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                let mut info = MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                    return;
                }
                state.fullscreen_restore = (state.is_borderless, IsZoomed(hwnd).as_bool());
                state.is_fullscreen = true;
                state.is_borderless = true;
                let rect = info.rcMonitor;
                apply_frame(
                    hwnd,
                    true,
                    (
                        rect.left,
                        rect.top,
                        rect.right - rect.left,
                        rect.bottom - rect.top,
                    ),
                );
            } else {
                let (borderless, maximized) = state.fullscreen_restore;
                state.is_fullscreen = false;
                state.is_borderless = borderless;
                apply_frame(hwnd, borderless, state.normal_rect);
                if maximized {
                    let _ = ShowWindow(hwnd, SW_MAXIMIZE);
                }
            }
        });
    }
    refresh(hwnd);
}

/// Shows or hides the window frame on its own. Turning it off puts back the rectangle — and the
/// maximized state — remembered on the way in; leaving it on also leaves full screen, which is a
/// frameless window over the whole monitor.
pub fn set_borderless(hwnd: HWND, on: bool) {
    unsafe {
        state::with_state(|state| {
            if on {
                // A maximized window reports its maximized rectangle, which is not the size to go
                // back to, so only an ordinary window's rectangle is remembered.
                if IsZoomed(hwnd).as_bool() {
                    state.was_maximized = true;
                    let _ = ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_RESTORE);
                    state.normal_rect = w32::window_rect(hwnd);
                } else {
                    state.normal_rect = w32::window_rect(hwnd);
                    state.was_maximized = false;
                }
                state.is_fullscreen = false;
                state.is_borderless = true;
                apply_frame(hwnd, true, state.normal_rect);
            } else {
                let rect = if state.is_fullscreen {
                    state.normal_rect
                } else {
                    w32::window_rect(hwnd)
                };
                let maximize =
                    state.was_maximized || (state.is_fullscreen && state.fullscreen_restore.1);
                state.is_fullscreen = false;
                state.is_borderless = false;
                apply_frame(hwnd, false, rect);
                if maximize {
                    let _ = ShowWindow(hwnd, SW_MAXIMIZE);
                }
            }
        });
    }
    refresh(hwnd);
}

/// Puts the window at `rect` with or without its frame. The style has to change with it: a popup
/// window has no title bar to draw, and only `SetWindowPos` makes Windows redraw the frame.
unsafe fn apply_frame(hwnd: HWND, borderless: bool, rect: (i32, i32, i32, i32)) {
    let style = frame_style(borderless) | WS_VISIBLE;
    let _ = unsafe { SetWindowLongPtrW(hwnd, GWL_STYLE, style.0 as isize) };
    let _ = unsafe {
        SetWindowPos(
            hwnd,
            None,
            rect.0,
            rect.1,
            rect.2,
            rect.3,
            SWP_FRAMECHANGED | SWP_NOZORDER,
        )
    };
}

/// A frameless window is a popup: no title bar, no border, no taskbar entry.
pub(super) fn frame_style(borderless: bool) -> WINDOW_STYLE {
    if borderless {
        WS_POPUP | WS_CLIPCHILDREN | windows::Win32::UI::WindowsAndMessaging::WS_THICKFRAME
    } else {
        WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN
    }
}

/// Reject invalid rectangles and positions left behind by a disconnected display.
pub(super) fn startup_bounds(bounds: Option<(i32, i32, i32, i32)>) -> (i32, i32, i32, i32) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{MonitorFromRect, MONITOR_DEFAULTTONULL};

    let fallback = (CW_USEDEFAULT, CW_USEDEFAULT, 1100, 750);
    let Some((x, y, width, height)) = bounds else {
        return fallback;
    };
    let (Some(right), Some(bottom)) = (x.checked_add(width), y.checked_add(height)) else {
        return fallback;
    };
    let rect = RECT {
        left: x,
        top: y,
        right,
        bottom,
    };
    if width <= 0
        || height <= 0
        || unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONULL) }.is_invalid()
    {
        fallback
    } else {
        (x, y, width, height)
    }
}

pub(super) fn remember_normal_rect(hwnd: HWND) {
    state::with_state(|state| unsafe {
        if !state.is_fullscreen && !IsZoomed(hwnd).as_bool() && !IsIconic(hwnd).as_bool() {
            let rect = w32::window_rect(hwnd);
            if rect.2 > 0 && rect.3 > 0 {
                state.normal_rect = rect;
            }
        }
    });
}
