//! The state the viewer shows: the current file and frame, the zoom and pan position, the
//! settings, and the display order. Windows, painting, the tool bar and input live in `view.rs`,
//! `tools.rs`, `dialog.rs` and `input.rs`.
use std::cell::RefCell;
use std::path::{Path, PathBuf};

use crate::config::{target_path, Config};
use crate::decode;
use crate::dialog::Dialog;
use crate::fileops;
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

pub fn read_state<R>(f: impl FnOnce(&State) -> R) -> Option<R> {
    WINDOW_STATE.with(|cell| cell.try_borrow().ok()?.as_ref().map(f))
}

pub fn install(state: State) {
    WINDOW_STATE.with(|cell| *cell.borrow_mut() = Some(state));
}

/// The settings the viewer behaves by, read from the config.
///
/// Key names and defaults are those of the config format, except the app's own keys:
/// `EnableToolBar`, `EnableRandomDisplayOrder` and `MoveToFolder`.
pub struct Knobs {
    pub zoom_mode: ZoomMode,
    pub loop_back: bool,
    pub free_pan: bool,
    pub pan_speed: i32,
    pub zoom_step_percent: f64,
    pub start_fullscreen: bool,
    pub topmost: bool,
    pub slideshow: bool,
    pub slideshow_seconds: u32,
    pub slideshow_seconds_to: u32,
    pub loop_slideshow: bool,
    pub play_gif_once: bool,
    pub include_hidden: bool,
    pub recurse: bool,
    pub show_toolbar: bool,
    /// Pick images at random during navigation and slideshows.
    pub random_order: bool,
    /// Where `M` moves the shown file to. Empty means `M` does nothing.
    pub move_to: PathBuf,
}

impl Knobs {
    /// Reads the settings, using the documented key names and defaults.
    pub fn from(config: &Config) -> Knobs {
        Knobs {
            // The mode decides the factor on load and on resize; only a locked zoom keeps one.
            zoom_mode: ZoomMode::parse(&config.get_str("ZoomMode", "AutoZoom"))
                .unwrap_or(ZoomMode::AutoZoom),
            loop_back: config.get_bool("EnableLoopBackNavigation", true),
            free_pan: config.get_bool("EnableFreePan", false),
            // clamped so a huge config value cannot overflow the pan offset in the window proc
            pan_speed: config.get_f64("PanSpeed", 20.0).clamp(1.0, 1000.0) as i32,
            zoom_step_percent: config.zoom_step_percent(),
            start_fullscreen: config.get_bool("EnableFullScreen", false),
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
    playlist_undo: Option<undo::PlaylistUndo>,
    folder_retry: Option<background::FolderRetry>,
    pub bitmap: Option<Bitmap>,
    pub file_size: Option<u64>,
    pub animation: Option<decode::Animation>,
    /// The background lanes decoding and preparing imports off the UI thread; tests build
    /// the `State` without one and run the same paths synchronously.
    pub worker: Option<worker::Worker>,
    /// The generation of the decode the current selection is waiting for.
    selection: u64,
    /// A decode of the current selection is outstanding; replies of older generations
    /// are discarded against this generation.
    pub load_pending: bool,
    /// A frame or rewind of the current selection is outstanding.
    frame_pending: bool,
    /// The current selection is an animation playing on the decode thread.
    animated_selection: bool,
    /// The replacement to keep: results of older replacements are dropped.
    replace_tag: u64,
    pending_import: Option<u64>,
    import_notice_until: Option<std::time::Instant>,
    /// Import tag and the first provisional row.
    import_preview: Option<(u64, usize)>,
    import_rows: Vec<PathBuf>,
    remote_import: Option<std::sync::mpsc::SyncSender<bool>>,
    /// The next import preparation's caller tag.
    prep_tag: u64,
    /// The command-line image to select once its folder's rows are prepared.
    startup_select: Option<PathBuf>,
    /// A directly opened image already shown as the provisional first row of its folder scan.
    direct_seed: Option<PathBuf>,
    startup_slideshow: bool,
    /// The command-line or picked playlist whose rows are being loaded, for the report.
    open_playlist_path: Option<PathBuf>,
    pub frame_revision: u64,
    frame_ms: u32,
    animation_played_once: bool,
    /// A zoom the user chose; `0.0` means the zoom mode decides.
    pub scale: f64,
    pub offset: (i32, i32),
    /// How many quarter turns the shown image was rotated by.
    pub rotation_turns: usize,
    /// Where the window sat before full screen took over the monitor, so leaving it goes back.
    pub normal_rect: (i32, i32, i32, i32),
    pub start_maximized: bool,
    /// Whether the window was maximized when the frame went off, so it is put back that way.
    pub was_maximized: bool,
    pub is_fullscreen: bool,
    pub fullscreen_restore: (bool, bool),
    /// The window frame is off, without the window covering the screen. `F10`.
    pub is_borderless: bool,
    /// Where the left button went down, while it is held down to pan the image.
    pub drag: Option<(i32, i32)>,
    pub toolbar_visible: bool,
    /// The playlist is shown over the picture. It is a view, so nothing of it is saved.
    pub playlist_visible: bool,
    pub zoom_mode: ZoomMode,
    pub slide_revision: u64,
    inspection: Option<inspection::Inspection>,
    pub slideshow_running: bool,
    /// A slideshow the playlist put on hold: it is started again when the list is closed.
    pub slideshow_paused: bool,
    pub slideshow_paused_by_space: bool,
    pub slideshow_elapsed: bool,
    pub pending: Option<PathBuf>,
    /// Which child-control panel is open right now.
    pub dialog: Dialog,
    pub message: Option<String>,
    pub notice: Option<String>,
    pub unreadable_notice_pending: bool,
    pub navigation_pending: bool,
}

impl State {
    /// The synchronous viewer: every load decodes in place, the way the direct unit
    /// tests exercise it.
    #[cfg(test)]
    pub fn new(config_path: PathBuf, args: &[String]) -> State {
        State::build(config_path, args, None)
    }

    /// The app entry: decoding and folder/playlist enumeration run on the background
    /// lanes instead of the UI thread, and the first picture appears when its `Opened`
    /// event is drained.
    pub fn interactive(config_path: PathBuf, args: &[String]) -> State {
        let worker = worker::Worker::new(&photo::image_formats());
        State::build(config_path, args, Some(worker))
    }

    fn build(config_path: PathBuf, args: &[String], worker: Option<worker::Worker>) -> State {
        let config = Config::load(config_path, args);
        let knobs = Knobs::from(&config);
        let toolbar_visible = knobs.show_toolbar;
        let zoom_mode = knobs.zoom_mode;
        let start_maximized = config.get_bool(crate::config::ALWAYS_MAXIMIZED, false);
        let mut state = State {
            config,
            knobs,
            photos: PhotoList::empty(),
            playlist_undo: None,
            folder_retry: None,
            bitmap: None,
            file_size: None,
            animation: None,
            worker,
            selection: 0,
            load_pending: false,
            frame_pending: false,
            animated_selection: false,
            replace_tag: 0,
            pending_import: None,
            import_notice_until: None,
            import_preview: None,
            import_rows: Vec::new(),
            remote_import: None,
            prep_tag: 0,
            startup_select: None,
            direct_seed: None,
            startup_slideshow: false,
            open_playlist_path: None,
            frame_revision: 0,
            frame_ms: 0,
            animation_played_once: false,
            scale: 0.0,
            offset: (0, 0),
            rotation_turns: 0,
            normal_rect: (0, 0, 1100, 750),
            start_maximized,
            was_maximized: false,
            is_fullscreen: false,
            fullscreen_restore: (false, false),
            is_borderless: false,
            drag: None,
            toolbar_visible,
            playlist_visible: false,
            zoom_mode,
            slide_revision: 0,
            inspection: None,
            slideshow_running: false,
            slideshow_paused: false,
            slideshow_paused_by_space: false,
            slideshow_elapsed: false,
            pending: None,
            dialog: Dialog::None,
            message: None,
            notice: None,
            unreadable_notice_pending: false,
            navigation_pending: false,
        };

        // A path on the command line, or dropped on the window, becomes the playlist. A `.rgpl`
        // file is a list of paths, so it is read as one. With the worker, the preparation
        // runs on its lane and the resulting rows replace the list once they arrive.
        let mut prepared_on_lane = false;
        if let Some(arg) = target_path(args) {
            let absolute = std::path::absolute(arg).ok();
            let path = absolute.as_deref().unwrap_or_else(|| Path::new(arg));
            let playlist = rgpl::is_playlist_file(path);
            let image = photo::is_image_file(path, &state.photos.formats);
            let maximize = state.config.get_bool(crate::config::FILES_MAXIMIZED, false);
            let metadata = if !playlist || maximize {
                path.metadata().ok()
            } else {
                None
            };
            let file = metadata.as_ref().is_some_and(std::fs::Metadata::is_file);
            let directory = metadata.as_ref().is_some_and(std::fs::Metadata::is_dir);
            state.start_maximized |= maximize && file && (playlist || image);
            if state.worker.is_some() {
                if playlist {
                    if state.submit_import(
                        Source::PlaylistFile {
                            path: path.to_path_buf(),
                        },
                        true,
                    ) {
                        state.open_playlist_path = Some(path.to_path_buf());
                        prepared_on_lane = true;
                    }
                } else if file && image {
                    prepared_on_lane = state.open_direct_image(path);
                } else if file || directory {
                    let folder = if directory {
                        path.to_path_buf()
                    } else {
                        path.parent().map(Path::to_path_buf).unwrap_or_default()
                    };
                    if state.submit_import(
                        Source::Folder {
                            path: folder,
                            include_hidden: state.knobs.include_hidden,
                            recurse: state.knobs.recurse && directory,
                            seed: None,
                        },
                        true,
                    ) {
                        state.startup_select = file.then(|| path.to_path_buf());
                        prepared_on_lane = true;
                    }
                }
            } else if playlist {
                if let Err(error) = state.open_playlist(path) {
                    state.message = Some(error);
                }
                return state;
            } else if file || directory {
                state.photos = PhotoList::from_path(
                    path,
                    state.knobs.include_hidden,
                    state.knobs.recurse,
                    photo::image_formats(),
                );
            }
        }

        if prepared_on_lane {
            state.startup_slideshow = state.knobs.slideshow;
        } else {
            state.load_current();
        }
        state
    }

    /// Keeps the settings panel's knobs in step with the config after it was saved.
    pub fn reload_knobs(&mut self) {
        let was_random = self.knobs.random_order;
        self.knobs = Knobs::from(&self.config);
        if was_random != self.knobs.random_order {
            self.photos.reset_random_pass();
        }
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

mod background;
mod image;
mod inspection;
mod library;
mod slideshow;
mod undo;
mod worker;
use worker::Source;
pub(crate) use worker::BACKGROUND_READY;
