use super::*;
pub fn bar_items(state: &State) -> Vec<Item> {
    let shown = state.photos.current().is_some();
    let zoomable = state.frame().is_some();
    let mut bar: Vec<Item> = BAR
        .iter()
        .map(|&(action, label)| {
            let (checked, enabled) = match action {
                Action::Previous | Action::Next => (false, shown),
                Action::RandomOrder => (state.knobs.random_order, shown),
                Action::ToggleRepeat => (state.knobs.loop_slideshow, shown),
                Action::ZoomIn | Action::ZoomOut => (false, zoomable),
                Action::FitOriginal => (
                    state.active_zoom_mode() == zoom::ZoomMode::ScaleToFit && state.scale <= 0.0,
                    zoomable,
                ),
                Action::ShowPlaylist => (state.playlist_visible, true),
                Action::ToggleToolBar => (state.toolbar_visible, true),
                _ => (false, true),
            };
            let label = match action {
                Action::ShowPlaylist if state.playlist_visible => "Hide playlist (L)",
                Action::ShowPlaylist => "Show playlist (L)",
                _ => label,
            };
            Item {
                action,
                label: label.into(),
                checked,
                enabled,
            }
        })
        .collect();
    let active = state.slideshow_running || state.slideshow_paused;
    bar.insert(
        3,
        Item {
            action: if active {
                Action::StopSlideshow
            } else {
                Action::StartSlideshow
            },
            label: if active {
                "Stop slideshow (F11)"
            } else {
                "Start slideshow (F11)"
            }
            .into(),
            enabled: shown || active,
            checked: active,
        },
    );
    for (index, mode) in zoom::MODES.iter().enumerate() {
        bar.push(Item {
            action: Action::ZoomMode(index),
            label: format!("{} ({})", mode.label(), char::from(mode.hotkey() as u8)),
            enabled: state.frame().is_some(),
            checked: state.active_zoom_mode() == *mode,
        });
    }
    bar
}

/// Where each button is, and the action it triggers. Buttons past the window's right edge are
/// left out: `shortcut: no scrolling in the bar, add one if the window is made narrow.`
pub fn bar_layout(state: &State, width: i32) -> Vec<(i32, i32, Item)> {
    const BUTTON_WIDTH: i32 = 34;
    let mut layout = Vec::new();
    let mut x = EDGE;
    for item in bar_items(state) {
        let buttons = BUTTON_WIDTH;
        if x + buttons > width - EDGE {
            break;
        }
        layout.push((x, buttons, item));
        x += buttons + EDGE;
    }
    layout
}
