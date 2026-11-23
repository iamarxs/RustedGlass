//! What the tool bar and the right-click menu can do: one list of actions, two ways to reach it.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, TrackPopupMenuEx, HMENU, MF_CHECKED,
    MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, TPM_RETURNCMD, TPM_RIGHTBUTTON,
};

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
    StopSlideshowAndArmQuit,
    ToggleSlideshowPause,
    OpenFile,
    /// Replaces the list with the images of a folder.
    OpenFolder,
    AddFiles,
    AddFolder,
    RemoveItem,
    DeleteFile,
    MoveToTarget,
    Rename,
    SaveAs,
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
    /// Whatever panel is open. `Esc` asks for this instead of picking a panel itself.
    ClosePanel,
    Quit,
    About,
    CopyImage,
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
    (Action::Previous, "Previous image (← / ↑)"),
    (Action::Next, "Next image (→ / ↓)"),
    (Action::RandomOrder, "Random image order (Ctrl+A)"),
    (Action::ToggleRepeat, "Loop slideshow"),
    (Action::ZoomIn, "Zoom in (+)"),
    (Action::ZoomOut, "Zoom out (−)"),
    (Action::FitOriginal, "Fit image to window (0)"),
];

mod menu;
pub use menu::show_menu;

mod layout;
pub use layout::bar_layout;
