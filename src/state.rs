//! The state the viewer shows: the current file and frame, the zoom and pan position, the
//! settings, and the display order. Windows, painting, the tool bar and input live in `view.rs`,
//! `tools.rs`, `dialog.rs` and `input.rs`.
use std::cell::RefCell;
use std::path::{Path, PathBuf};

use crate::config::{target_path, Config};
use crate::decode;
use crate::dialog::Dialog;
use crate::fileops::{self, Pending};
use crate::photo::{self, PhotoList};
use crate::render::Bitmap;
use crate::rgpl;
use crate::zoom::{self, ZoomMode};
use serde_json::Value;

thread_local! {
    /// The viewer and its window procedure share this UI thread.
    static WINDOW_STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Returns `None` if a nested Win32 message arrives while state is already borrowed.
pub fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    WINDOW_STATE.with(|cell| {
        let mut borrowed = cell.try_borrow_mut().ok()?;
        borrowed.as_mut().map(f)
    })
}

pub fn install(state: State) {
    WINDOW_STATE.with(|cell| *cell.borrow_mut() = Some(state));
}

/// The settings the viewer behaves by, read from the config.
///
/// The key names and defaults are those of the C# `Config.cs`, except the ones this port adds:
/// `EnableToolBar`, `EnableRandomDisplayOrder` and `MoveToFolder`.
pub struct Knobs {
    pub zoom_mode: ZoomMode,
    pub loop_back: bool,
    pub auto_sibling_dir: bool,
    pub free_pan: bool,
    pub pan_speed: i32,
    pub zoom_speed: f64,
    pub start_fullscreen: bool,
    pub start_maximized: bool,
    pub topmost: bool,
    pub slideshow: bool,
    pub slideshow_seconds: u32,
    pub slideshow_seconds_to: u32,
    pub loop_slideshow: bool,
    pub play_gif_once: bool,
    pub include_hidden: bool,
    pub recurse: bool,
    pub show_toolbar: bool,
    /// Show the files in a random order instead of by name.
    pub random_order: bool,
    /// Where `M` moves the shown file to. Empty means `M` does nothing.
    pub move_to: PathBuf,
}

impl Knobs {
    /// Reads the settings, using the key names and defaults of the C# `ImageGlassConfig`.
    pub fn from(config: &Config) -> Knobs {
        Knobs {
            // The mode decides the factor on load and on resize; only a locked zoom keeps one.
            zoom_mode: ZoomMode::parse(&config.get_str("ZoomMode", "AutoZoom"))
                .unwrap_or(ZoomMode::AutoZoom),
            loop_back: config.get_bool("EnableLoopBackNavigation", true),
            auto_sibling_dir: config.get_bool("EnableAutoSwitchSiblingDir", false),
            free_pan: config.get_bool("EnableFreePan", false),
            pan_speed: config.get_f64("PanSpeed", 20.0).max(1.0) as i32,
            // `0` is "auto" in C#; `zoom::zoom_step` turns it into a factor per press.
            zoom_speed: config.get_f64("ZoomSpeed", 0.0),
            start_fullscreen: config.get_bool("EnableFullScreen", false),
            start_maximized: config.get_bool("EnableMainWindowMaximized", false),
            topmost: config.get_bool("EnableWindowTopMost", false),
            slideshow: config.get_bool("EnableSlideshow", false),
            slideshow_seconds: config.get_f64("SlideshowInterval", 5.0).max(1.0) as u32,
            slideshow_seconds_to: config
                .get_f64(
                    "SlideshowIntervalTo",
                    config.get_f64("SlideshowInterval", 5.0),
                )
                .max(1.0) as u32,
            loop_slideshow: config.get_bool("EnableLoopSlideshow", true),
            play_gif_once: config.get_bool("SlideshowPlayGifOnce", true),
            include_hidden: config.get_bool("EnableHiddenImagesLoading", false),
            recurse: config.get_bool("EnableSubfoldersLoading", false),
            // The tool bar is the app's main control, so it is shown until the settings say otherwise.
            show_toolbar: config.get_bool("EnableToolBar", true),
            random_order: config.get_bool("EnableRandomDisplayOrder", false),
            move_to: PathBuf::from(config.get_str("MoveToFolder", "")),
        }
    }
}

/// Everything the viewer shows and remembers.
pub struct State {
    pub config: Config,
    pub knobs: Knobs,
    pub photos: PhotoList,
    pub bitmap: Option<Bitmap>,
    pub gif: Option<decode::Gif>,
    pub frame_revision: u64,
    gif_frame_ms: u32,
    gif_played_once: bool,
    /// A zoom the user chose; `0.0` means the zoom mode decides.
    pub scale: f64,
    pub offset: (i32, i32),
    /// How many quarter turns the shown image was rotated by.
    pub rotation_turns: usize,
    /// Where the window sat before full screen took over the monitor, so leaving it goes back.
    pub normal_rect: (i32, i32, i32, i32),
    /// Whether the window was maximized when the frame went off, so it is put back that way.
    pub was_maximized: bool,
    pub is_fullscreen: bool,
    pub fullscreen_restore: (bool, bool),
    /// The window frame is off, without the window covering the screen. `F10`.
    pub is_borderless: bool,
    /// Where the left button went down, while it is held down to pan the image.
    pub drag: Option<(i32, i32)>,
    pub is_dragging: bool,
    pub toolbar_visible: bool,
    /// The playlist is shown over the picture. It is a view, so nothing of it is saved.
    pub playlist_visible: bool,
    pub zoom_mode: ZoomMode,
    pub slide_revision: u64,
    pub slideshow_running: bool,
    /// A slideshow the playlist put on hold: it is started again when the list is closed.
    pub slideshow_paused: bool,
    pub slideshow_elapsed: bool,
    pub pending: Option<(Pending, PathBuf)>,
    /// Which child-control panel is open right now.
    pub dialog: Dialog,
    pub message: Option<String>,
    pub notice: Option<String>,
    pub unreadable_notice_pending: bool,
    pub navigation_pending: bool,
}

impl State {
    pub fn new(config_path: PathBuf, args: &[String]) -> State {
        let config = Config::load(config_path, args);
        let knobs = Knobs::from(&config);
        let toolbar_visible = knobs.show_toolbar;
        let zoom_mode = knobs.zoom_mode;
        let mut state = State {
            config,
            knobs,
            photos: PhotoList::empty(),
            bitmap: None,
            gif: None,
            frame_revision: 0,
            gif_frame_ms: 0,
            gif_played_once: false,
            scale: 0.0,
            offset: (0, 0),
            rotation_turns: 0,
            normal_rect: (0, 0, 1100, 750),
            was_maximized: false,
            is_fullscreen: false,
            fullscreen_restore: (false, false),
            is_borderless: false,
            drag: None,
            is_dragging: false,
            toolbar_visible,
            playlist_visible: false,
            zoom_mode,
            slide_revision: 0,
            slideshow_running: false,
            slideshow_paused: false,
            slideshow_elapsed: false,
            pending: None,
            dialog: Dialog::None,
            message: None,
            notice: None,
            unreadable_notice_pending: false,
            navigation_pending: false,
        };

        // A path on the command line, or dropped on the window, becomes the playlist. A `.rgpl`
        // file is a list of paths, so it is read as one.
        if let Some(arg) = target_path(args) {
            let absolute = std::path::absolute(arg).ok();
            let path = absolute.as_deref().unwrap_or_else(|| Path::new(arg));
            if rgpl::is_playlist_file(path) {
                if let Err(error) = state.open_playlist(path) {
                    state.message = Some(error);
                }
                return state;
            } else if path.is_file() || path.is_dir() {
                state.photos = PhotoList::from_path(
                    path,
                    state.knobs.include_hidden,
                    state.knobs.recurse,
                    photo::image_formats(),
                );
                if path.is_dir() {
                    state.load_new_order();
                    return state;
                }
            }
        }

        state.apply_order();
        state.load_current();
        state
    }

    /// Keeps the settings panel's knobs in step with the config after it was saved.
    pub fn reload_knobs(&mut self) {
        self.knobs = Knobs::from(&self.config);
        self.toolbar_visible = self.knobs.show_toolbar;
        self.zoom_mode = self.knobs.zoom_mode;
    }
}

/// Rows to take out of a list from the back upwards, so removing one row does not move the
/// rows after it. Repeated rows are dropped: two actions on one row would delete twice.
fn sorted_desc(rows: &[usize]) -> Vec<usize> {
    let mut rows: Vec<usize> = rows.to_vec();
    rows.sort_unstable_by(|a, b| b.cmp(a));
    rows.dedup();
    rows
}

/// A seed that changes per call, so every shuffle differs.
pub fn random_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.subsec_nanos() as u64 + since.as_secs() * 1_000_000_019)
        .unwrap_or(0x9E37_79B9_7F4A_7C15)
}

#[cfg(test)]
mod tests;

mod image;

mod library;

mod slideshow;
