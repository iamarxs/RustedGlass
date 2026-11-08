use super::*;
type Shortcut = (&'static str, &'static str);

/// Keyboard shortcuts and mouse gestures grouped by task for the F1 panel.
pub const SHORTCUT_GROUPS: &[(&str, &[Shortcut])] = &[
    ("Browse images", &[
        ("← / ↑", "Previous image"),
        ("→ / ↓ / Space", "Next image"),
        ("Home / End", "First, last image"),
        ("Wheel", "Previous, next image"),
        ("O", "Open files"),
        ("F", "Open a folder"),
        ("I", "Show image dimensions and EXIF details"),
    ]),
    ("Zoom and rotate", &[
        ("0", "Show the whole picture"),
        ("1 - 6", "Zoom modes 1 to 6"),
        ("+ / -", "Zoom in, zoom out"),
        ("Ctrl + Wheel", "Zoom in, zoom out"),
        ("Drag", "Move the image"),
        ("R", "Turn it 90 degrees"),
    ]),
    ("Slideshow", &[
        ("F11", "Slideshow on, off"),
    ]),
    ("Playlists", &[
        ("L", "Playlist on, off"),
        ("Ctrl + O / S", "Open / save a playlist"),
        ("Ctrl + A", "Random display order"),
        ("A", "Add files to the list"),
        ("Shift + A", "Add a folder to the list"),
        ("Del", "Remove this item from the list"),
    ]),
    ("File actions", &[
        ("Ctrl + Del", "Delete the file (recycle bin)"),
        ("M", "Move the file to the set folder"),
        ("F2", "Rename the file"),
        ("C", "Copy the file to a folder"),
        ("E", "Show the file in Explorer"),
    ]),
    ("Window and help", &[
        ("F12", "Full screen on, off"),
        ("F10", "Window frame on, off"),
        ("Drag from top", "Move the borderless window. Start just below the top edge, clear of the toolbar buttons."),
        ("T", "Tool bar on, off"),
        ("S / Ctrl + P", "Open the settings panel"),
        ("Right-click", "Menu of everything"),
        ("F1", "Shortcut list on, off"),
        ("Esc", "Close panel, image, window"),
        ("Ctrl + Q", "Quit"),
    ]),
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
        VK_RIGHT | VK_DOWN | VK_SPACE => Action::Next,
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
            'c' => Action::CopyToFolder,
            'e' => Action::ShowInExplorer,
            'i' if !ctrl => Action::ImageInfo,
            'q' if ctrl => Action::Quit,
            _ => return None,
        },
    };
    Some(action)
}

/// `Esc`: close the panel, else the image, else the window.
fn escape() -> Action {
    if state::with_state(|state| state.dialog != Dialog::None).unwrap_or(false) {
        Action::ClosePanel
    } else if state::with_state(|s| s.playlist_visible) == Some(true) {
        Action::ShowPlaylist
    } else if state::with_state(|state| state.frame().is_some()).unwrap_or(false) {
        Action::CloseImage
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
