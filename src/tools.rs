//! What the tool bar and the right-click menu can do: one list of actions, two ways to reach it.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, TrackPopupMenuEx, HMENU, MF_CHECKED,
    MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, TPM_RETURNCMD, TPM_RIGHTBUTTON,
};

use crate::config::Config;
use crate::state::State;
use crate::w32;
use crate::zoom;

/// Something the viewer can be told to do. `ZoomMode` carries the index into `zoom::MODES`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Settings,
    /// The list of keys, so the panel and the menu reach the same thing.
    Shortcuts,
    ToggleToolBar,
    ToggleFullscreen,
    /// The window frame off, without covering the screen.
    ToggleBorderless,
    /// The list of file names over the picture.
    ShowPlaylist,
    ZoomMode(usize),
    /// Fit the window and forget a zoom the user typed.
    FitOriginal,
    ZoomIn,
    ZoomOut,
    Previous,
    Next,
    First,
    Last,
    Rotate,
    RandomOrder,
    /// Repeat the slideshow when it reaches the last image.
    ToggleRepeat,
    StartSlideshow,
    StopSlideshow,
    OpenFile,
    /// Replaces the list with the images of a folder.
    OpenFolder,
    AddFiles,
    AddFolder,
    RemoveItem,
    DeleteFile,
    MoveToTarget,
    Rename,
    CopyToFolder,
    ShowInExplorer,
    /// Reads a `.rgpl` file into the list.
    OpenPlaylist,
    /// Writes the rows, in the order they are shown, to a `.rgpl` file.
    SavePlaylist,
    /// Takes the selected rows out of the list, leaving the files on disk.
    RemoveSelected,
    /// Sends the files of the selected rows to the recycle bin.
    DeleteSelected,
    /// Opens the folder a selected file sits in.
    ShowFolder,
    /// Rebuilds the rows in name order. `descending` reverses them.
    SortByName(bool),
    ImageInfo,
    ShufflePlaylist,
    CloseImage,
    /// Whatever panel is open. `Esc` asks for this instead of picking a panel itself.
    ClosePanel,
    Quit,
    About,
    CopyImage,
    /// A blank line in the menu.
    Separator,
}

pub struct Item {
    pub action: Action,
    pub label: String,
    pub checked: bool,
    pub enabled: bool,
}

/// The band the tool bar is painted in, and the gap between its buttons.
pub const BAND_HEIGHT: i32 = 48;
pub const EDGE: i32 = 8;
/// Menu command ids start here, so `WM_COMMAND` values cannot be confused with control ids.
const MENU_BASE: usize = 1000;

/// The buttons in the order they are drawn, with their tooltip and accessible labels. The keys do the same things,
/// so the buttons stay few: navigation, zoom, the zoom modes.
const BAR: &[(Action, &str)] = &[
    (Action::Settings, "Settings (S)"),
    (Action::ToggleToolBar, "Hide toolbar (T)"),
    (Action::ShowPlaylist, "Playlist (L)"),
    (Action::Previous, "Previous image (Left arrow)"),
    (Action::Next, "Next image (Right arrow)"),
    (Action::RandomOrder, "Shuffle (Ctrl+A)"),
    (Action::ToggleRepeat, "Repeat slideshow"),
    (Action::ZoomIn, "Zoom in (+)"),
    (Action::ZoomOut, "Zoom out (-)"),
    (Action::FitOriginal, "Show whole picture (0)"),
];

/// Everything the menu offers. The keys are listed here too so a reader finds them in one place.
pub fn items(state: &State, config: &Config) -> Vec<Item> {
    let shown = state.photos.current().is_some();
    let zoomable = state.frame().is_some();
    let mut items: Vec<Item> = Vec::new();
    {
        // A local helper: every entry differs only in these four values.
        let mut push = |action: Action, label: &str, enabled: bool, checked: bool| {
            items.push(Item {
                action,
                label: label.to_string(),
                checked,
                enabled,
            });
        };

        push(Action::Settings, "Settings…  S", true, false);
        push(Action::Shortcuts, "Keyboard shortcuts…  F1", true, false);
        push(Action::ImageInfo, "Image information  I", shown, false);
        push(
            Action::ToggleToolBar,
            "Show / hide the tool bar  T",
            true,
            state.toolbar_visible,
        );
        push(
            Action::ToggleFullscreen,
            "Full screen  F12",
            true,
            state.is_fullscreen,
        );
        let slideshow_active = state.slideshow_running || state.slideshow_paused;
        push(
            if slideshow_active {
                Action::StopSlideshow
            } else {
                Action::StartSlideshow
            },
            "Slideshow  F11",
            shown || slideshow_active,
            state.slideshow_running,
        );
        push(
            Action::ToggleBorderless,
            "Window frame off  F10",
            true,
            state.is_borderless && !state.is_fullscreen,
        );
        push(
            Action::ToggleRepeat,
            "Repeat the slideshow",
            shown,
            config.get_bool("EnableLoopSlideshow", true),
        );
        push(Action::Separator, "", false, false);

        push(
            Action::FitOriginal,
            "Show the whole picture  0",
            zoomable,
            state.active_zoom_mode() == zoom::ZoomMode::ScaleToFit && state.scale <= 0.0,
        );
        for (index, mode) in zoom::MODES.iter().enumerate() {
            let label = format!("{}  {}", mode.label(), char::from(mode.hotkey() as u8));
            push(
                Action::ZoomMode(index),
                &label,
                zoomable,
                state.active_zoom_mode() == *mode,
            );
        }
        push(Action::ZoomIn, "Zoom in  +", zoomable, false);
        push(Action::ZoomOut, "Zoom out  -", zoomable, false);
        push(Action::Separator, "", false, false);

        push(Action::First, "First image  Home", shown, false);
        push(Action::Last, "Last image  End", shown, false);
        push(
            Action::ShowPlaylist,
            "Show / hide the list of files  L",
            true,
            state.playlist_visible,
        );
        push(Action::Rotate, "Turn it 90 degrees  R", zoomable, false);
        push(
            Action::RandomOrder,
            "Shuffle the list  Ctrl+A",
            shown,
            config.get_bool("EnableRandomDisplayOrder", false),
        );
        push(Action::Separator, "", false, false);

        push(Action::OpenFile, "Open files…  O", true, false);
        push(Action::OpenFolder, "Open a folder…  F", true, false);
        push(Action::AddFiles, "Add files to the list…  A", true, false);
        push(
            Action::AddFolder,
            "Add a folder to the list…  Shift+A",
            true,
            false,
        );
        push(
            Action::RemoveItem,
            "Remove this item from the list  Ctrl+Del",
            shown,
            false,
        );
        push(
            Action::DeleteFile,
            "Send this file to the recycle bin  Del",
            shown,
            false,
        );
        push(
            Action::MoveToTarget,
            "Move it to the set folder  M",
            shown && !state.knobs.move_to.as_os_str().is_empty(),
            false,
        );
        push(Action::Rename, "Rename it…  F2", shown, false);
        push(
            Action::CopyImage,
            "Copy image to clipboard",
            zoomable,
            false,
        );
        push(
            Action::CopyToFolder,
            "Copy it to a folder…  C",
            shown,
            false,
        );
        push(Action::ShowInExplorer, "Show it in Explorer", shown, false);
        push(Action::Separator, "", false, false);

        push(Action::CloseImage, "Close this image  Escape", shown, false);
        push(Action::About, "&About…", true, false);
        push(Action::Quit, "Quit", true, false);
    }
    items
}

/// The buttons to draw: the actions of `BAR` with their short text, plus the zoom modes, which
/// are the reason the bar exists.
mod menu;
pub use menu::show_menu;

mod layout;
pub use layout::bar_layout;
