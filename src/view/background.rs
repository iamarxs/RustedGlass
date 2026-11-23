use super::*;
use windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;

pub(super) fn attach(hwnd: HWND) {
    state::with_state(|state| {
        if let Some(worker) = &state.worker {
            worker.attach_window(hwnd);
        }
    });
    unsafe { arm(hwnd, EVENT_TIMER, 250) };
}

pub(super) fn drain(hwnd: HWND) -> LRESULT {
    let update = state::with_state(|state| {
        let before = state.notice.clone();
        if !state.poll_background() {
            return None;
        }
        state.advance_slideshow_if_ready();
        let new_notice = state.notice.is_some() && state.notice != before;
        Some((new_notice, state.take_image_info()))
    })
    .flatten();
    if let Some((new_notice, details)) = update {
        if new_notice {
            expire_notice(hwnd);
        }
        refresh(hwnd);
        if let Some(details) = details.filter(|_| unsafe { IsWindowEnabled(hwnd).as_bool() }) {
            crate::metadata::show(hwnd, &details);
        }
    }
    LRESULT(1)
}
