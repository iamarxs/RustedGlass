use std::cell::Cell;
use std::time::{Duration, Instant};

use windows::Win32::Graphics::Gdi::HDC;
use windows::Win32::UI::Controls::{EM_SETLIMITTEXT, EM_SETSEL};
use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows::Win32::UI::Shell::SetWindowSubclass;
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, MoveWindow, SendMessageW, ES_CENTER, WINDOW_STYLE, WM_SETFONT, WS_BORDER,
    WS_CHILD, WS_TABSTOP, WS_VISIBLE,
};

use super::*;
use crate::dialog::controls::{create, prompt_key};
use crate::style;
use crate::tools::Action;
use crate::zoom::{self, ZoomMode};

pub(super) const TIMER: usize = 5;
pub(super) const EDIT_ID: usize = 901;
const DISPLAY_MS: u32 = 2_000;
const RANGE: std::ops::RangeInclusive<f64> = 1.0..=10_000.0;
const EDIT_HEIGHT: i32 = 24;

thread_local! {
    static UNTIL: Cell<Option<Instant>> = const { Cell::new(None) };
    static REVISION: Cell<u64> = const { Cell::new(0) };
    static FIELD: Cell<Option<HWND>> = const { Cell::new(None) };
    static INVALID: Cell<bool> = const { Cell::new(false) };
}

fn available(state: &state::State) -> bool {
    state.frame().is_some()
        && state.dialog == Dialog::None
        && !state.playlist_visible
        && !state.navigation_pending
}

fn visible() -> bool {
    UNTIL.get().is_some_and(|until| until > Instant::now())
}

fn bounds((width, height): (u32, u32), editing: bool) -> (i32, i32, i32, i32) {
    let (width, height) = (width as i32, height as i32);
    let w = width.min(if editing { 232 } else { 112 });
    let h = height.min(if editing { 94 } else { 42 });
    ((width - w) / 2, (height - h) / 2, w, h)
}

fn field_bounds(area: (u32, u32)) -> (i32, i32, i32) {
    let (x, y, w, h) = bounds(area, true);
    let field_width = w.min(120);
    (
        x + (w - field_width) / 2,
        y + (h - EDIT_HEIGHT) / 2,
        field_width,
    )
}

fn percent_text(factor: f64) -> String {
    format!("{:.2}", factor * 100.0)
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

pub(crate) fn change(hwnd: HWND, area: (u32, u32), action: Action) {
    match action {
        Action::ZoomMode(index) => {
            if let Some(&mode) = zoom::MODES.get(index) {
                state::with_state(|state| state.select_zoom_mode(mode, area));
            }
        }
        Action::FitOriginal => {
            state::with_state(|state| state.select_zoom_mode(ZoomMode::ScaleToFit, area));
        }
        Action::ZoomIn => return step(hwnd, area, 120),
        Action::ZoomOut => return step(hwnd, area, -120),
        _ => return,
    }
    show(hwnd);
    refresh(hwnd);
}

pub(crate) fn step(hwnd: HWND, area: (u32, u32), delta: i32) {
    if delta == 0 || FIELD.get().is_some() {
        return;
    }
    state::with_state(|state| {
        let step = zoom::zoom_step(state.knobs.zoom_step_percent, delta);
        state.zoom_by(area, if delta > 0 { step } else { 1.0 / step });
    });
    show(hwnd);
    refresh(hwnd);
}

fn show(hwnd: HWND) {
    if let Some(Some(revision)) =
        state::with_state(|state| available(state).then_some(state.slide_revision))
    {
        REVISION.set(revision);
        UNTIL.set(Some(
            Instant::now() + Duration::from_millis(DISPLAY_MS.into()),
        ));
        unsafe { arm(hwnd, TIMER, DISPLAY_MS) };
    }
}

pub(super) fn expire(hwnd: HWND) {
    if !visible() {
        UNTIL.set(None);
        unsafe {
            arm(hwnd, TIMER, 0);
            w32::invalidate(hwnd);
        }
    }
}

pub(crate) fn open(hwnd: HWND) {
    if let Some(field) = FIELD.get() {
        let _ = unsafe { SetFocus(Some(field)) };
        return;
    }
    let area = unsafe { w32::client_size(hwnd) };
    let Some(Some((factor, revision))) = state::with_state(|state| {
        available(state).then(|| (state.factor(area), state.slide_revision))
    }) else {
        return;
    };
    let (x, y, w) = field_bounds(area);
    let text = w32::wide(&percent_text(factor));
    let Ok(field) = create(
        windows::core::w!("EDIT").as_ptr(),
        text.as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER | WINDOW_STYLE(ES_CENTER as u32),
        x,
        y - 2,
        w,
        EDIT_HEIGHT,
        hwnd,
        EDIT_ID,
        w32::instance(),
    ) else {
        return;
    };
    unsafe {
        if !SetWindowSubclass(field, Some(prompt_key), EDIT_ID, hwnd.0 as usize).as_bool() {
            let _ = DestroyWindow(field);
            return;
        }
        REVISION.set(revision);
        FIELD.set(Some(field));
        UNTIL.set(None);
        arm(hwnd, TIMER, 0);
        let _ = SendMessageW(
            field,
            WM_SETFONT,
            Some(WPARAM(style::font().0 as usize)),
            None,
        );
        let _ = SendMessageW(field, EM_SETLIMITTEXT, Some(WPARAM(24)), None);
        let _ = SetFocus(Some(field));
        let _ = SendMessageW(field, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
        w32::invalidate(hwnd);
    }
}

fn parse_percent(text: &str) -> Option<f64> {
    let text = text.trim().trim_end_matches('%').trim_end();
    let percent = text.replace(',', ".").parse::<f64>().ok()?;
    RANGE.contains(&percent).then_some(percent)
}

pub(super) fn apply(hwnd: HWND) {
    let Some(field) = FIELD.get() else { return };
    let text = unsafe { w32::control_text(field) };
    let Some(percent) = parse_percent(&text) else {
        INVALID.set(true);
        unsafe { w32::invalidate(hwnd) };
        return;
    };
    clear(hwnd);
    let applied = state::with_state(|state| {
        if available(state) && REVISION.get() == state.slide_revision {
            state.set_zoom_factor(percent / 100.0);
            return true;
        }
        false
    });
    if applied == Some(true) {
        show(hwnd);
    }
    refresh(hwnd);
}

pub(super) fn click(hwnd: HWND, point: LPARAM) -> bool {
    let editing = FIELD.get().is_some();
    if !editing && !visible() {
        return false;
    }
    let (x, y, w, h) = bounds(unsafe { w32::client_size(hwnd) }, editing);
    if (x..x + w).contains(&w32::pointer_x(point)) && (y..y + h).contains(&w32::pointer_y(point)) {
        open(hwnd);
        return FIELD.get().is_some();
    }
    editing && cancel(hwnd)
}

pub(super) fn cancel(hwnd: HWND) -> bool {
    if FIELD.get().is_none() {
        return false;
    }
    clear(hwnd);
    unsafe { w32::invalidate(hwnd) };
    true
}

pub(super) fn clear(hwnd: HWND) {
    UNTIL.set(None);
    INVALID.set(false);
    unsafe {
        arm(hwnd, TIMER, 0);
        if let Some(field) = FIELD.take() {
            let focused = GetFocus() == field;
            let _ = DestroyWindow(field);
            if focused {
                let _ = SetFocus(Some(hwnd));
            }
        }
    }
}

pub(super) fn layout(hwnd: HWND, area: (u32, u32)) {
    let field = FIELD.get();
    if field.is_none() && UNTIL.get().is_none() {
        return;
    }
    if state::with_state(|state| available(state) && REVISION.get() == state.slide_revision)
        == Some(false)
    {
        clear(hwnd);
    } else if let Some(field) = field {
        let (x, y, w) = field_bounds(area);
        let _ = unsafe { MoveWindow(field, x, y, w, EDIT_HEIGHT, true) };
    }
}

pub(crate) fn draw(dc: HDC, area: (u32, u32), state: &state::State) {
    let editing = FIELD.get().is_some();
    if (!editing && !visible()) || !available(state) || REVISION.get() != state.slide_revision {
        return;
    }
    let (x, y, w, h) = bounds(area, editing);
    paint::shade(dc, (x, y, w, h));
    if editing {
        style::caption(dc, (x, y, x + w, y + 32), "Exact zoom (%)");
        let hint = if INVALID.get() {
            "Enter a value from 1 to 10,000"
        } else {
            "Enter to apply · Esc to cancel"
        };
        style::caption(dc, (x, y + 62, x + w, y + h), hint);
    } else {
        let label = format!("{}%", percent_text(state.factor(area)));
        style::caption(dc, (x, y, x + w, y + h), &label);
    }
}

#[cfg(test)]
mod tests {
    use super::parse_percent;

    #[test]
    fn exact_zoom_accepts_decimal_percentages_within_the_manual_range() {
        assert_eq!(parse_percent("137.5"), Some(137.5));
        assert_eq!(parse_percent("137,5 %"), Some(137.5));
        assert_eq!(parse_percent("10000"), Some(10_000.0));
        for invalid in ["", "NaN", "inf", "0", "10000.1", "-25", "1,2.3"] {
            assert_eq!(parse_percent(invalid), None);
        }
    }
}
