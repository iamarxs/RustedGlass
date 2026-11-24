use super::*;
use windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;

/// Finished background results applied per wake-up, so a burst of completed work cannot
/// monopolize one UI turn. What does not fit waits for the next wake-up or the fallback
/// timer, both of which keep coming while results are pending.
const RESULTS_PER_TURN: usize = 16;

pub(super) fn attach(hwnd: HWND) {
    state::with_state(|state| {
        if let Some(worker) = &state.worker {
            worker.attach_window(hwnd);
        }
    });
    unsafe { arm(hwnd, EVENT_TIMER, 250) };
}

pub(super) fn drain(hwnd: HWND) -> LRESULT {
    let anchor = playlist::capture_background_scroll();
    let update = state::with_state(|state| {
        let before = state.notice.clone();
        // One budgeted slice of the finished results per turn; the wake-up is
        // acknowledged before draining, so anything arriving meanwhile posts again.
        let changed = state.poll_background_budget(RESULTS_PER_TURN);
        // Results that did not fit this turn's budget get a fresh wake-up, so the next
        // turn handles them instead of waiting for the fallback timer.
        if let Some(worker) = &state.worker {
            worker.wake_pending_results();
        }
        if !changed {
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
        refresh_with_scroll(hwnd, anchor.as_ref());
        if let Some(details) = details.filter(|_| unsafe { IsWindowEnabled(hwnd).as_bool() }) {
            crate::metadata::show(hwnd, &details);
        }
    } else if playlist::request_metadata() {
        // The fallback timer can also finish a sort whose last rows were cached.
        refresh_with_scroll(hwnd, anchor.as_ref());
    }
    if unsafe { IsWindowEnabled(hwnd).as_bool() } {
        if let Some(respond) = state::with_state(|state| state.take_remote_import()).flatten() {
            use windows::core::w;
            use windows::Win32::UI::WindowsAndMessaging::{
                MessageBoxW, IDYES, MB_DEFBUTTON2, MB_ICONQUESTION, MB_YESNO,
            };

            let action = unsafe {
                MessageBoxW(
                Some(hwnd),
                w!("This playlist contains paths on network shares. Opening them can connect to those servers with your Windows account. Allow network paths for this import?"),
                w!("RustedGlass playlist"),
                MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2,
            )
            };
            let _ = respond.try_send(action == IDYES);
        }
    }
    LRESULT(1)
}

#[cfg(test)]
mod tests;
