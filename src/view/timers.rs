use super::*;
use std::cell::Cell;
use std::time::{Duration, Instant};

type SlideKey = (u64, (u32, u32));
type FrameKey = (u64, u32);

thread_local! {
    static SLIDE: Cell<Option<(SlideKey, Instant)>> = const { Cell::new(None) };
    static FRAME: Cell<Option<(FrameKey, Instant)>> = const { Cell::new(None) };
}

pub(super) fn sync(hwnd: HWND) {
    state::with_state(|state| {
        let slide =
            (state.slideshow_running && !state.slideshow_elapsed && !state.navigation_pending)
                .then_some((state.slide_revision, state.knobs.slideshow_bounds()));
        let delay = state.frame_delay_ms();
        let frame = (delay != 0).then_some((state.frame_revision, delay));

        SLIDE.with(|timer| {
            schedule(hwnd, SLIDESHOW_TIMER, timer, slide, || {
                state.slideshow_delay_ms()
            })
        });
        FRAME.with(|timer| schedule(hwnd, FRAME_TIMER, timer, frame, || delay));
    });
}

fn schedule<K: Copy + Eq>(
    hwnd: HWND,
    id: usize,
    timer: &Cell<Option<(K, Instant)>>,
    key: Option<K>,
    delay: impl FnOnce() -> u32,
) {
    if timer.get().map(|(key, _)| key) == key {
        return;
    }
    timer.set(None);
    unsafe {
        let _ = KillTimer(Some(hwnd), id);
        if let Some(key) = key {
            // Match the limits applied by SetTimer.
            let delay = delay().clamp(10, 0x7fff_ffff);
            let due = Instant::now() + Duration::from_millis(u64::from(delay));
            if SetTimer(Some(hwnd), id, delay, None) != 0 {
                timer.set(Some((key, due)));
            }
        }
    }
}

fn take_due<K: Copy>(hwnd: HWND, id: usize, timer: &Cell<Option<(K, Instant)>>) -> bool {
    let Some((_, deadline)) = timer.get() else {
        return false;
    };
    let remaining = deadline.saturating_duration_since(Instant::now());
    unsafe {
        // A message from a replaced timer can still be queued.
        if !remaining.is_zero() {
            let _ = SetTimer(Some(hwnd), id, remaining.as_millis() as u32 + 1, None);
            return false;
        }
        let _ = KillTimer(Some(hwnd), id);
    }
    timer.set(None);
    true
}

pub(super) fn slide_due(hwnd: HWND) -> bool {
    SLIDE.with(|timer| take_due(hwnd, SLIDESHOW_TIMER, timer))
}

pub(super) fn frame_due(hwnd: HWND) -> bool {
    FRAME.with(|timer| take_due(hwnd, FRAME_TIMER, timer))
}

pub(super) fn clear(hwnd: HWND) {
    SLIDE.with(|timer| schedule(hwnd, SLIDESHOW_TIMER, timer, None, || 0));
    FRAME.with(|timer| schedule(hwnd, FRAME_TIMER, timer, None, || 0));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refreshing_an_armed_timer_keeps_its_deadline_and_delay_sample() {
        let key = (3, (1, 5));
        let due = Instant::now() + Duration::from_secs(2);
        let timer = Cell::new(Some((key, due)));
        schedule(HWND::default(), SLIDESHOW_TIMER, &timer, Some(key), || {
            panic!("refresh should not draw a new interval")
        });
        assert_eq!(timer.get(), Some((key, due)));
    }
}
