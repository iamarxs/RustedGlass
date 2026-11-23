use super::*;
type Shortcut = (&'static str, &'static str);

/// Keyboard shortcuts and mouse gestures grouped by task for the F1 panel.
pub const SHORTCUT_GROUPS: &[(&str, &[Shortcut])] = &[
    (
        "Navigation",
        &[
            ("← / ↑", "Previous image"),
            ("→ / ↓", "Next image"),
            ("Home / End", "Go to first / last image"),
            ("Wheel", "Browse images"),
            ("O", "Open images"),
            ("F", "Open folder"),
            ("Ctrl + A", "Toggle random image order"),
        ],
    ),
    (
        "Zoom and image",
        &[
            ("+ / −", "Zoom in / out"),
            ("Ctrl + Wheel", "Zoom in / out"),
            ("0 / 5", "Fit image to window"),
            ("1 / 2", "Auto zoom / lock zoom ratio"),
            ("3 / 4", "Scale to width / height"),
            ("6", "Scale to fill"),
            ("Drag", "Pan image when ‘Pan image by dragging’ is enabled"),
            ("R", "Rotate image 90°"),
            ("I", "Show image information"),
        ],
    ),
    (
        "Slideshow",
        &[
            ("F11", "Start or stop slideshow"),
            ("Space", "Pause / resume slideshow or view next image"),
        ],
    ),
    (
        "Playlist",
        &[
            ("L", "Show or hide playlist"),
            ("Ctrl + O", "Open playlist"),
            ("Ctrl + S", "Save playlist"),
            ("A", "Add images"),
            ("Shift + A", "Add folder"),
            ("Del", "Remove image or selected rows from playlist"),
        ],
    ),
    (
        "Files",
        &[
            ("Ctrl + Del", "Move image or selected files to Recycle Bin"),
            ("M", "Quick-move image"),
            ("F2", "Rename image"),
            ("C", "Save as"),
            ("E", "Show image in Explorer"),
        ],
    ),
    (
        "Window and help",
        &[
            ("F12", "Toggle full screen"),
            ("F10", "Show or hide window frame"),
            (
                "Drag from top",
                "Move borderless window from below the top edge, clear of toolbar buttons",
            ),
            ("T", "Show or hide toolbar"),
            ("S / Ctrl + P", "Open Settings"),
            (
                "Right-click",
                "Open context menu. In the playlist, use Sort by to reorder rows",
            ),
            ("F1", "Show or hide keyboard shortcuts"),
            (
                "Esc",
                "Close the panel or playlist. Stop an active slideshow. Quit if neither applies.",
            ),
            ("Ctrl + Q", "Quit"),
        ],
    ),
];

/// Whether Ctrl is down. `MK_CONTROL` has no constant in this windows-rs version, and Ctrl+wheel
/// would not report it either.
pub(super) unsafe fn ctrl_down() -> bool {
    GetKeyState(VK_CONTROL.0 as i32) < 0
}

/// The character a virtual key stands for, so the letters need no constant each. Layout-aware,
/// so a non-US keyboard still works.
fn letter(vk: u32) -> Option<char> {
    let code = unsafe { MapVirtualKeyW(vk, MAPVK_VK_TO_CHAR) } & 0xFFFF;
    let text = char::from_u32(code)?;
    if text.is_control() {
        None
    } else {
        Some(text.to_ascii_lowercase())
    }
}

/// What a key press asks for. `None` means the viewer has no action for it.
pub fn on_key(vk: u32, ctrl: bool, shift: bool) -> Option<Action> {
    // The virtual keys are constants of `VIRTUAL_KEY`, so the match runs on that type.
    let key = VIRTUAL_KEY(vk as u16);
    let action = match key {
        VK_LEFT | VK_UP => Action::Previous,
        VK_RIGHT | VK_DOWN => Action::Next,
        VK_SPACE => {
            if state::with_state(|s| s.slideshow_running || s.slideshow_paused).unwrap_or(false) {
                Action::ToggleSlideshowPause
            } else {
                Action::Next
            }
        }
        VK_HOME => Action::First,
        VK_END => Action::Last,
        VK_ESCAPE => return Some(escape()),
        VK_F1 => Action::Shortcuts,
        VK_F2 => Action::Rename,
        VK_F10 => Action::ToggleBorderless,
        VK_F11 => slideshow_toggle(),
        VK_F12 => Action::ToggleFullscreen,
        VK_DELETE if state::with_state(|s| s.playlist_visible) == Some(true) => {
            if ctrl {
                Action::DeleteSelected
            } else {
                Action::RemoveSelected
            }
        }
        VK_DELETE if ctrl => Action::DeleteFile,
        VK_DELETE => Action::RemoveItem,
        _ => match letter(vk)? {
            '0' => Action::FitOriginal,
            '+' | '=' => Action::ZoomIn,
            '-' | '_' => Action::ZoomOut,
            digit @ '1'..='6' => Action::ZoomMode(digit.to_digit(10)? as usize - 1),
            'r' => Action::Rotate,
            't' => Action::ToggleToolBar,
            'l' if !ctrl => Action::ShowPlaylist,
            's' if !ctrl => Action::Settings,
            'p' if ctrl => Action::Settings,
            'f' => Action::OpenFolder,
            'o' if ctrl => Action::OpenPlaylist,
            's' if ctrl => Action::SavePlaylist,
            'o' => Action::OpenFile,
            'a' if shift => Action::AddFolder,
            'a' if ctrl => Action::RandomOrder,
            'a' => Action::AddFiles,
            'm' => Action::MoveToTarget,
            'c' => Action::SaveAs,
            'e' => Action::ShowInExplorer,
            'i' if !ctrl => Action::ImageInfo,
            'q' if ctrl => Action::Quit,
            _ => return None,
        },
    };
    Some(action)
}

/// `Esc`: close the panel or playlist, stop a running slideshow (the next press quits),
/// otherwise quit immediately — even with an image open.
fn escape() -> Action {
    if state::with_state(|state| state.dialog != Dialog::None).unwrap_or(false) {
        Action::ClosePanel
    } else if state::with_state(|s| s.playlist_visible) == Some(true) {
        Action::ShowPlaylist
    } else if state::with_state(|s| s.slideshow_running || s.slideshow_paused) == Some(true) {
        Action::StopSlideshowAndArmQuit
    } else if state::with_state(|state| state.slideshow_stopped_by_escape) == Some(true) {
        state::with_state(|state| state.slideshow_stopped_by_escape = false);
        Action::Quit
    } else {
        Action::Quit
    }
}

/// `F11` starts a slideshow, or stops the one that is running.
fn slideshow_toggle() -> Action {
    if state::with_state(|state| state.slideshow_running || state.slideshow_paused).unwrap_or(false)
    {
        Action::StopSlideshow
    } else {
        Action::StartSlideshow
    }
}

/// Whether Shift was down. Asked of the keyboard state rather than the message, because the
/// message's own shift bit only says "was already held".
pub fn shift_held(_lparam: LPARAM) -> bool {
    unsafe { GetKeyState(VK_SHIFT.0 as i32) < 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlist_shortcuts_are_distinct_from_image_open_and_settings() {
        assert_eq!(on_key(b'O' as u32, true, false), Some(Action::OpenPlaylist));
        assert_eq!(on_key(b'S' as u32, true, false), Some(Action::SavePlaylist));
        assert_eq!(on_key(b'O' as u32, false, false), Some(Action::OpenFile));
        assert_eq!(on_key(b'S' as u32, false, false), Some(Action::Settings));
    }

    #[test]
    fn space_pauses_a_running_slideshow_but_still_advances_when_off() {
        let mut state = crate::state::State::new(crate::test_path("unused-config.json"), &[]);
        state.photos.files.push("image.png".into());
        assert_eq!(on_key(VK_SPACE.0 as u32, false, false), Some(Action::Next));
        state.start_slideshow();
        state::install(state);
        assert_eq!(
            on_key(VK_SPACE.0 as u32, false, false),
            Some(Action::ToggleSlideshowPause)
        );
    }

    #[test]
    fn escape_stops_a_slideshow_then_quits_on_the_next_press() {
        let mut state = crate::state::State::new(crate::test_path("unused-config.json"), &[]);
        state.photos.files.push("image.png".into());
        state.start_slideshow();
        state::install(state);
        assert_eq!(escape(), Action::StopSlideshowAndArmQuit);
        state::with_state(|state| {
            state.stop_slideshow();
            state.slideshow_stopped_by_escape = true;
        });
        assert_eq!(escape(), Action::Quit);
    }

    #[test]
    fn escape_quits_immediately_when_no_slideshow_runs_even_with_an_image_open() {
        let mut state = crate::state::State::new(crate::test_path("unused-config.json"), &[]);
        state.photos.files.push("image.png".into());
        state::install(state);
        assert_eq!(escape(), Action::Quit);
    }

    #[test]
    fn every_shortcut_is_listed_once() {
        let table: Vec<_> = SHORTCUT_GROUPS
            .iter()
            .flat_map(|(_, rows)| rows.iter().copied())
            .collect();
        assert!(table.len() > 20, "the list should cover the viewer");
        let mut keys: Vec<&str> = table.iter().map(|(key, _)| *key).collect();
        for (key, what) in &table {
            assert!(!key.is_empty() && !what.is_empty());
        }
        keys.sort_unstable();
        let all = keys.len();
        keys.dedup();
        assert_eq!(all, keys.len(), "a key is listed twice");
    }
}
