use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use super::{Event, Lane};

pub(crate) const BACKGROUND_READY: u32 = WM_APP + 3;

/// The worker only posts a message to this window. State and drawing resources stay on the UI thread.
/// Wake-ups are coalesced: while one `BACKGROUND_READY` message is queued, further finished results
/// only fill the lanes — handling the queued message drains them all.
#[derive(Default)]
pub(super) struct Wake {
    window: AtomicPtr<std::ffi::c_void>,
    /// A wake-up message is queued but the UI has not started draining yet.
    pending: AtomicBool,
}

impl Wake {
    pub(super) fn set_window(&self, hwnd: HWND) {
        self.window.store(hwnd.0, Ordering::Release);
        // Results can already be queued when startup attaches the window.
        self.post();
    }

    pub(super) fn deliver(&self, lane: &Lane<Event>, event: Event) {
        if lane.deliver(event).is_ok() {
            self.post();
        }
    }

    /// The UI thread calls this when it starts draining results. It runs before the
    /// drain, so a result delivered during the drain finds `pending` clear and posts
    /// a fresh wake-up: no wake-up is lost between the drain and the flag.
    pub(super) fn acknowledge(&self) {
        self.pending.store(false, Ordering::Release);
    }

    pub(super) fn post(&self) {
        // One outstanding wake-up at a time: the queued message drains the lanes, so
        // posting again before it is handled would only pile up identical messages.
        if self.pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let window = self.window.load(Ordering::Acquire);
        let posted = !window.is_null()
            && unsafe { PostMessageW(Some(HWND(window)), BACKGROUND_READY, WPARAM(0), LPARAM(0)) }
                .is_ok();
        if !posted {
            // No window yet or the queue refused the post: clear the flag so the next
            // result tries again; the fallback timer covers the gap meanwhile.
            self.pending.store(false, Ordering::Release);
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

    fn message_window() -> HWND {
        unsafe {
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
        .unwrap()
    }

    /// Drains the queued wake-ups, the way the UI's message loop would.
    fn take_wake_ups(hwnd: HWND) -> usize {
        let mut count = 0;
        let mut message = MSG::default();
        while unsafe {
            PeekMessageW(
                &mut message,
                Some(hwnd),
                BACKGROUND_READY,
                BACKGROUND_READY,
                PM_REMOVE,
            )
        }
        .as_bool()
        {
            count += 1;
        }
        count
    }

    #[test]
    fn a_burst_of_results_coalesces_into_one_wake_up_and_an_acknowledged_drain_wakes_again() {
        let hwnd = message_window();
        let wake = Wake::default();
        let results = Lane::new(8);
        let event = || Event::PrepFailed {
            tag: 1,
            error: String::new(),
        };

        // Before a window is attached a delivery cannot post; the flag must not stay set.
        wake.deliver(&results, event());
        assert_eq!(take_wake_ups(hwnd), 0);
        wake.set_window(hwnd);
        assert_eq!(
            take_wake_ups(hwnd),
            1,
            "attaching posts the startup wake-up"
        );
        wake.acknowledge();

        for _ in 0..5 {
            wake.deliver(&results, event());
        }
        assert_eq!(
            take_wake_ups(hwnd),
            1,
            "five finished results share the one queued wake-up"
        );

        wake.acknowledge();
        wake.deliver(&results, event());
        assert_eq!(
            take_wake_ups(hwnd),
            1,
            "a result after the acknowledge posts again — no lost wake"
        );
        assert_eq!(
            (0..6).filter(|_| results.try_pop().is_some()).count(),
            6,
            "every delivered result is still in the lane"
        );
        let _ = unsafe { DestroyWindow(hwnd) };
    }

    #[test]
    fn a_delivery_from_another_thread_after_the_acknowledge_is_not_lost() {
        let hwnd = message_window();
        let wake = Wake::default();
        let results = Lane::new(2);
        let event = || Event::PrepFailed {
            tag: 1,
            error: String::new(),
        };
        // A result queued before the window attaches wakes the window once it is there.
        wake.deliver(&results, event());
        wake.set_window(hwnd);
        let had_startup = take_wake_ups(hwnd) == 1 && results.try_pop().is_some();

        wake.acknowledge();
        std::thread::scope(|thread| {
            thread.spawn(|| wake.deliver(&results, event()));
        });
        let had_delivery = take_wake_ups(hwnd) == 1 && results.try_pop().is_some();
        let _ = unsafe { DestroyWindow(hwnd) };
        assert!(had_startup && had_delivery);
    }
}
