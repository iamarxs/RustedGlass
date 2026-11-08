use super::*;
pub fn bar_items(state: &State, config: &Config) -> Vec<Item> {
    let items = items(state, config);
    let mut bar: Vec<Item> = BAR
        .iter()
        .map(|&(action, label)| {
            let (checked, enabled) = match action {
                Action::Previous | Action::Next => (false, !state.photos.is_empty()),
                _ => items
                    .iter()
                    .find(|item| item.action == action)
                    .map_or((false, false), |item| (item.checked, item.enabled)),
            };
            Item {
                action,
                label: label.into(),
                checked,
                enabled,
            }
        })
        .collect();
    if let Some(slideshow) = items
        .iter()
        .find(|item| matches!(item.action, Action::StartSlideshow | Action::StopSlideshow))
    {
        bar.insert(
            3,
            Item {
                action: slideshow.action,
                label: if slideshow.action == Action::StartSlideshow {
                    "Play"
                } else {
                    "Stop"
                }
                .into(),
                enabled: slideshow.enabled,
                checked: slideshow.action == Action::StopSlideshow,
            },
        );
    }
    for (index, mode) in zoom::MODES.iter().enumerate() {
        bar.push(Item {
            action: Action::ZoomMode(index),
            label: format!("{} {}", char::from(mode.hotkey() as u8), mode.label()),
            enabled: state.frame().is_some(),
            checked: state.active_zoom_mode() == *mode,
        });
    }
    bar
}

/// Where each button is, and the action it triggers. Buttons past the window's right edge are
/// left out: `shortcut: no scrolling in the bar, add one if the window is made narrow.`
pub fn bar_layout(state: &State, config: &Config, width: i32) -> Vec<(i32, i32, Item)> {
    let mut layout = Vec::new();
    let mut x = EDGE;
    for item in bar_items(state, config) {
        let buttons = width_of(&item.label);
        if x + buttons > width - EDGE {
            break;
        }
        layout.push((x, buttons, item));
        x += buttons + EDGE;
    }
    layout
}

/// A button is as wide as its text, in the 8 pixel average of a 9 point UI font.
/// `shortcut: measured text would need GetTextExtentPoint32W per label.`
fn width_of(label: &str) -> i32 {
    (label.chars().count() as i32) * 8 + 14
}
