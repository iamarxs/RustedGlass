use super::*;

pub(super) unsafe extern "system" fn wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match message {
            WM_CREATE => {
                let _ = DragAcceptFiles(hwnd, true);
                LRESULT(0)
            }
            WM_PAINT => {
                paint::paint(hwnd);
                LRESULT(0)
            }
            WM_ERASEBKGND => {
                // `paint` fills the client area itself, in one copy. Erasing first would show the
                // background colour between the frames, which is the flicker.
                LRESULT(1)
            }
            WM_WINDOWPOSCHANGED => {
                frame::remember_normal_rect(hwnd);
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            WM_ENTERSIZEMOVE => {
                paint::set_live_resize(true);
                LRESULT(0)
            }
            WM_EXITSIZEMOVE => {
                paint::set_live_resize(false);
                refresh(hwnd);
                LRESULT(0)
            }
            WM_SIZE => {
                if paint::is_live_resize() {
                    layout_controls(hwnd);
                    w32::invalidate(hwnd);
                } else {
                    refresh(hwnd);
                }
                LRESULT(0)
            }
            WM_NOTIFY => {
                if lparam.0 == 0 {
                    return LRESULT(0);
                }
                // A row of the playlist is the picture to show. The focus goes back to the window
                // afterwards, so the keys still work after a click in the list.
                let header = &*(lparam.0 as *const NMHDR);
                if playlist::owns(header.hwndFrom) {
                    if header.code == NM_CLICK || header.code == NM_DBLCLK {
                        let item = &*(lparam.0 as *const NMITEMACTIVATE);
                        if item.iItem >= 0 {
                            input::jump_to(hwnd, item.iItem as usize);
                        }
                        let _ = SetFocus(Some(hwnd));
                    } else if header.code == LVN_BEGINDRAG {
                        let item = &*(lparam.0 as *const NMLISTVIEW);
                        playlist::begin_drag(header.hwndFrom, item.iItem);
                    } else if header.code == NM_RCLICK {
                        let _ = PostMessageW(Some(hwnd), WM_RBUTTONUP, wparam, LPARAM(0));
                    } else if header.code == LVN_GETDISPINFOW {
                        playlist::display_info(lparam);
                    } else if header.code == LVN_ODFINDITEMW {
                        return playlist::find_item(lparam);
                    } else if header.code == NM_CUSTOMDRAW {
                        return playlist::custom_draw(lparam);
                    }
                }
                LRESULT(0)
            }
            WM_KEYDOWN => {
                let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
                if let Some(action) =
                    input::on_key(wparam.0 as u32, ctrl, input::shift_held(lparam))
                {
                    input::apply_action(hwnd, action);
                }
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                // While the shortcut list is open the wheel reads the list, not the picture.
                if state::with_state(|state| state.dialog == Dialog::Shortcuts) == Some(true) {
                    let _ = windows::Win32::Graphics::Gdi::UpdateWindow(hwnd);
                    dialog::scroll_by(w32::wheel_delta(wparam));
                    refresh(hwnd);
                } else {
                    input::on_wheel(hwnd, wparam);
                }
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                if state::with_state(|state| state.dialog == Dialog::Settings) == Some(true) {
                    if let Some(id) = dialog::click_checkbox_label(
                        hwnd,
                        w32::pointer_x(lparam),
                        w32::pointer_y(lparam),
                    ) {
                        toggle_setting(hwnd, id);
                        return LRESULT(0);
                    }
                }
                let _ = SetCapture(hwnd);
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                if wparam.0 & 1 != 0 {
                    input::on_drag(hwnd, lparam);
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let _ = ReleaseCapture();
                input::on_drag_end();
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                input::show_menu(hwnd);
                LRESULT(0)
            }
            WM_DROPFILES => {
                let drop = HDROP(wparam.0 as *mut std::ffi::c_void);
                input::on_drop(hwnd, drop);
                let _ = DragFinish(drop);
                LRESULT(0)
            }
            windows::Win32::UI::WindowsAndMessaging::WM_NCCALCSIZE
            | windows::Win32::UI::WindowsAndMessaging::WM_NCPAINT
                if GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & WS_CAPTION.0 == 0 =>
            {
                LRESULT(0)
            }
            windows::Win32::UI::WindowsAndMessaging::WM_NCACTIVATE
                if GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & WS_CAPTION.0 == 0
                    && !IsIconic(hwnd).as_bool() =>
            {
                // Let Windows change activation without repainting a non-client frame.
                DefWindowProcW(hwnd, message, wparam, LPARAM(-1))
            }
            WM_NCHITTEST => {
                let resize = state::with_state(|s| s.is_borderless && !s.is_fullscreen);
                if resize == Some(true) {
                    let mut point = POINT {
                        x: w32::pointer_x(lparam),
                        y: w32::pointer_y(lparam),
                    };
                    let _ = ScreenToClient(hwnd, &mut point);
                    let size = w32::client_size(hwnd);
                    return LRESULT(crate::frame_hit::hit(point, size) as isize);
                }
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            windows::Win32::UI::WindowsAndMessaging::WM_DRAWITEM
                if lparam.0 != 0 && bar::draw(lparam) =>
            {
                LRESULT(1)
            }
            WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORBTN => {
                LRESULT(dialog::color_control(HDC(wparam.0 as *mut std::ffi::c_void)).0 as isize)
            }
            WM_COMMAND => {
                let id = w32::command_id(wparam);
                if id == PROMPT_ID {
                    if lparam.0 == 0 {
                        if let Some(value) = dialog::prompt_text() {
                            confirm_prompt(hwnd, &value);
                        }
                    }
                } else if id == dialog::APPLY_ID {
                    press_apply(hwnd);
                } else if dialog::is_pick(id) {
                    pick_folder(hwnd, id - dialog::PICK_BASE);
                } else if let Some(action) = bar::action(id) {
                    // Return focus to the viewer so its shortcuts keep working.
                    input::apply_action(hwnd, action);
                    let _ = SetFocus(Some(hwnd));
                } else if state::with_state(|state| state.dialog == Dialog::Settings) == Some(true)
                {
                    toggle_setting(hwnd, id);
                }
                LRESULT(0)
            }
            WM_TIMER => {
                match wparam.0 {
                    SLIDESHOW_TIMER => {
                        if !timers::slide_due(hwnd) {
                            return LRESULT(1);
                        }
                        if state::with_state(|state| {
                            state.slideshow_elapsed = state.slideshow_running;
                            state.advance_slideshow_if_ready()
                        }) != Some(true)
                        {
                            return LRESULT(1);
                        }
                    }
                    NAV_TIMER => {
                        arm(hwnd, NAV_TIMER, 0);
                        if state::with_state(|state| state.navigation_pending) != Some(true) {
                            return LRESULT(1);
                        }
                        state::with_state(|state| state.finish_navigation());
                    }
                    NOTICE_TIMER => {
                        arm(hwnd, NOTICE_TIMER, 0);
                        state::with_state(|state| state.notice = None);
                        playlist::invalidate();
                        w32::invalidate(hwnd);
                        return LRESULT(1);
                    }
                    FRAME_TIMER => {
                        if !timers::frame_due(hwnd) {
                            return LRESULT(1);
                        }
                        let advanced = state::with_state(|state| {
                            if state.frame_delay_ms() != 0 {
                                state.advance_frame();
                            }
                            state.advance_slideshow_if_ready() || state.unreadable_notice_pending
                        }) == Some(true);
                        if !advanced {
                            timers::sync(hwnd);
                            w32::invalidate(hwnd);
                            return LRESULT(1);
                        }
                    }
                    _ => {}
                }
                refresh(hwnd);
                LRESULT(1)
            }
            WM_DESTROY => {
                paint::clear_buffer();
                timers::clear(hwnd);
                // Settings the user changed while the window was open survive the next start.
                state::with_state(|state| {
                    state
                        .config
                        .set("WindowBounds", serde_json::json!(state.normal_rect));
                    let _ = state.config.save_user();
                });
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}
