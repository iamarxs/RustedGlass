//! How the viewer is driven: keys, the mouse wheel, drags, the right-click menu and dropped
//! files. A key, a tool bar button and a menu entry all end up in `apply_action`, so the three
//! ways of asking for the same thing cannot drift apart. `SHORTCUT_GROUPS` is what the `F1` panel
//! and this module's own handling agree on.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, MapVirtualKeyW, MAPVK_VK_TO_CHAR, VIRTUAL_KEY, VK_CONTROL, VK_DELETE, VK_DOWN,
    VK_END, VK_ESCAPE, VK_F1, VK_F10, VK_F11, VK_F12, VK_F2, VK_HOME, VK_LEFT, VK_RIGHT, VK_SHIFT,
    VK_SPACE, VK_UP,
};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};

use crate::dialog::{self, Dialog};
use crate::fileops::Pending;
use crate::playlist;
use crate::state;
use crate::tools::{self, Action};
use crate::view;
use crate::w32;
use crate::zoom::{self, ZoomMode};

/// Show the image at this place in the list. A row of the playlist and `Home` / `End` all ask
/// for the same thing, so they share this.
pub fn jump_to(hwnd: HWND, position: usize) {
    state::with_state(|state| {
        if state.photos.jump_to(position).is_some() {
            state.navigation_pending = true;
        }
    });
    view::refresh_navigation(hwnd);
}

pub fn apply_action(hwnd: HWND, action: Action) {
    if settle_file_selection(hwnd, action) {
        return;
    }
    let screen = unsafe { w32::client_size(hwnd) };

    match action {
        Action::Settings | Action::Shortcuts => open_panel(
            hwnd,
            if action == Action::Settings {
                Dialog::Settings
            } else {
                Dialog::Shortcuts
            },
        ),
        Action::ClosePanel => {
            state::with_state(|state| {
                dialog::close();
                state.dialog = Dialog::None;
                state.pending = None;
            });
            view::refresh(hwnd);
        }
        Action::CloseImage => {
            state::with_state(|state| {
                state.bitmap = None;
                state.gif = None;
                state.navigation_pending = false;
                state.message = Some(String::from(
                    "Nothing shown: press F for a folder, or drop one here",
                ));
            });
            view::refresh(hwnd);
        }
        Action::ToggleToolBar => {
            state::with_state(|state| state.toolbar_visible = !state.toolbar_visible);
            view::refresh(hwnd);
        }
        Action::ToggleFullscreen => {
            let on = !state::with_state(|state| state.is_fullscreen).unwrap_or(false);
            view::set_fullscreen(hwnd, on);
        }
        Action::ToggleBorderless => {
            let on = !state::with_state(|state| state.is_borderless).unwrap_or(false);
            view::set_borderless(hwnd, on);
        }
        Action::Previous | Action::Next => {
            view::navigate(hwnd, if action == Action::Previous { -1 } else { 1 });
        }
        Action::First | Action::Last => {
            let last = state::with_state(|state| state.photos.len().saturating_sub(1)).unwrap_or(0);
            jump_to(hwnd, if action == Action::First { 0 } else { last });
        }
        Action::ShowPlaylist => {
            let (width, height) = (screen.0 as i32, screen.1 as i32);
            state::with_state(|state| {
                state.playlist_visible = !state.playlist_visible;
                if state.playlist_visible {
                    state.pause_slideshow();
                    playlist::show(hwnd, &state.photos, width, height);
                } else {
                    playlist::hide();
                    state.resume_slideshow();
                }
            });
            view::refresh(hwnd);
        }
        Action::Rotate => {
            state::with_state(|state| state.rotate_frames(90));
            view::refresh(hwnd);
        }
        Action::ZoomMode(index) => {
            state::with_state(|state| {
                if let Some(mode) = zoom::MODES.get(index) {
                    state.select_zoom_mode(*mode, screen);
                }
            });
            view::refresh(hwnd);
        }
        Action::FitOriginal => {
            state::with_state(|state| {
                state.select_zoom_mode(ZoomMode::ScaleToFit, screen);
                state.clear_manual_zoom();
            });
            view::refresh(hwnd);
        }
        Action::ZoomIn | Action::ZoomOut => {
            state::with_state(|state| {
                let step = zoom::zoom_step(state.knobs.zoom_speed, 120);
                state.zoom_by(
                    screen,
                    if action == Action::ZoomIn {
                        step
                    } else {
                        1.0 / step
                    },
                );
            });
            view::refresh(hwnd);
        }
        Action::StartSlideshow | Action::StopSlideshow => {
            state::with_state(|state| {
                if action == Action::StartSlideshow {
                    state.start_slideshow();
                } else {
                    state.stop_slideshow();
                }
            });
            view::expire_notice(hwnd);
            view::refresh(hwnd);
        }
        Action::ShufflePlaylist => {
            state::with_state(|state| {
                let shown = state.photos.file_index(state.photos.index);
                state.photos.shuffle(crate::state::random_seed());
                if let Some(index) = shown {
                    state.photos.index = state.photos.position_of_file(index).unwrap_or(0);
                }
            });
            view::refresh(hwnd);
        }
        Action::RandomOrder => {
            state::with_state(|state| state.toggle_random_order());
            view::refresh(hwnd);
        }
        Action::ToggleRepeat => {
            state::with_state(|state| {
                state.knobs.loop_slideshow = !state.knobs.loop_slideshow;
                state.config.set(
                    "EnableLoopSlideshow",
                    serde_json::Value::Bool(state.knobs.loop_slideshow),
                );
            });
            view::refresh(hwnd);
        }
        Action::OpenFile | Action::AddFiles => {
            let paths = dialog::pick_files(hwnd);
            if !paths.is_empty() {
                state::with_state(|state| match action {
                    Action::OpenFile => state.open_files(&paths),
                    _ => {
                        let _ = state.add_files(&paths);
                    }
                });
                panels::refresh_import(hwnd);
            }
        }
        Action::OpenFolder | Action::AddFolder => {
            let folder = dialog::pick_folder(hwnd);
            if let Some(dir) = folder {
                state::with_state(|state| match action {
                    Action::OpenFolder => state.open_folder(&dir),
                    _ => {
                        let _ = state.add_folder(&dir);
                    }
                });
                panels::refresh_import(hwnd);
            }
        }
        Action::RemoveItem => {
            state::with_state(|state| state.remove_current());
            view::refresh(hwnd);
        }
        Action::DeleteFile => {
            state::with_state(|state| state.message = state.delete_current().err());
            view::refresh(hwnd);
        }
        Action::MoveToTarget => {
            state::with_state(|state| {
                if let Err(error) = state.move_current_to_target() {
                    state.message = Some(error);
                }
            });
            view::refresh(hwnd);
        }
        Action::Rename => ask(hwnd, Pending::Rename),
        Action::CopyToFolder => ask(hwnd, Pending::CopyTo(PathBuf::new())),
        Action::ShowInExplorer => {
            state::with_state(|state| {
                if let Some(path) = state.photos.current() {
                    state.message = crate::fileops::show_in_explorer(path).err();
                }
            });
            view::refresh(hwnd);
        }
        Action::OpenPlaylist => {
            let path =
                dialog::pick_one_file(hwnd, "Open a playlist", "Playlists (.rgpl)", "*.rgpl");
            if let Some(path) = path {
                state::with_state(|state| {
                    if let Err(message) = state.open_playlist(&path) {
                        state.notice = Some(message);
                    }
                });
                panels::refresh_import(hwnd);
            }
        }
        Action::SavePlaylist => {
            let path = dialog::pick_save_file(
                hwnd,
                "Save the list of files",
                "Playlists (.rgpl)",
                "*.rgpl",
                "playlist",
            );
            if let Some(path) = path {
                state::with_state(|state| {
                    if let Err(message) = state.save_playlist(&path) {
                        state.message = Some(message);
                    }
                });
                view::refresh(hwnd);
            }
        }
        Action::RemoveSelected => {
            let rows = targets();
            state::with_state(|state| state.remove_many(&rows));
            view::refresh(hwnd);
        }
        Action::DeleteSelected => {
            let rows = targets();
            state::with_state(|state| state.message = state.delete_many(&rows));
            view::refresh(hwnd);
        }
        Action::ShowFolder => {
            let rows = targets();
            state::with_state(|state| {
                let row = rows.first().copied().unwrap_or(state.photos.index);
                if let Some(path) = state.photos.path_at(row) {
                    state.message = crate::fileops::show_in_explorer(path).err();
                }
            });
            view::refresh(hwnd);
        }
        Action::SortByName(descending) => {
            state::with_state(|state| state.sort_by_name(descending));
            view::refresh(hwnd);
        }
        Action::CopyImage => panels::copy_image(hwnd),
        Action::About => crate::about::show(hwnd),
        Action::ImageInfo => crate::metadata::show(hwnd),
        Action::Quit => {
            let _ = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
        }
        Action::Separator => {}
    }
}

mod keys;
use keys::ctrl_down;
pub use keys::{on_key, shift_held, SHORTCUT_GROUPS};

mod pointer;
use pointer::targets;
pub use pointer::{on_drag, on_drag_end, on_drop, on_wheel, show_menu};
mod panels;
use panels::{ask, open_panel, settle_file_selection};
