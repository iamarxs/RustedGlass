use std::sync::atomic::{AtomicPtr, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use super::{Event, Lane};

pub(crate) const BACKGROUND_READY: u32 = WM_APP + 3;

/// The worker only posts a message to this window. State and drawing resources stay on the UI thread.
#[derive(Default)]
pub(super) struct Wake(AtomicPtr<std::ffi::c_void>);

impl Wake {
    pub(super) fn set_window(&self, hwnd: HWND) {
        self.0.store(hwnd.0, Ordering::Release);
        // Results can already be queued when startup attaches the window.
        self.post();
    }

    pub(super) fn deliver(&self, lane: &Lane<Event>, event: Event) {
        if lane.deliver(event).is_ok() {
            self.post();
        }
    }

    fn post(&self) {
        let window = self.0.load(Ordering::Acquire);
        if !window.is_null() {
            let _ =
                unsafe { PostMessageW(Some(HWND(window)), BACKGROUND_READY, WPARAM(0), LPARAM(0)) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::w;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, PeekMessageW, HWND_MESSAGE, MSG, PM_REMOVE,
    };

    #[test]
    fn attaching_and_delivering_wake_the_window_without_a_timer() {
        let hwnd = unsafe {
            CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!(""),
                Default::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }
        .unwrap();
        let wake = Wake::default();
        let results = Lane::new(1);
        let event = || Event::PrepFailed {
            tag: 1,
            error: String::new(),
        };
        wake.deliver(&results, event());
        wake.set_window(hwnd);
        let mut message = MSG::default();
        let had_initial_event = results.try_pop().is_some();
        let had_startup = unsafe {
            PeekMessageW(
                &mut message,
                Some(hwnd),
                BACKGROUND_READY,
                BACKGROUND_READY,
                PM_REMOVE,
            )
        }
        .as_bool()
            && had_initial_event;
        std::thread::scope(|thread| {
            thread.spawn(|| wake.deliver(&results, event()));
        });
        let had_delivery = unsafe {
            PeekMessageW(
                &mut message,
                Some(hwnd),
                BACKGROUND_READY,
                BACKGROUND_READY,
                PM_REMOVE,
            )
        }
        .as_bool()
            && results.try_pop().is_some();
        let _ = unsafe { DestroyWindow(hwnd) };
        assert!(had_startup && had_delivery);
    }
}
