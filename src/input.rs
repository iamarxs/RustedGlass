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
    VK_END, VK_ESCAPE, VK_F1, VK_F10, VK_F11, VK_F12, VK_F2, VK_HOME, VK_LEFT, VK_NEXT, VK_PRIOR,
    VK_RIGHT, VK_SHIFT, VK_SPACE, VK_UP,
};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};

use crate::dialog::{self, Dialog};
use crate::playlist;
use crate::state;
use crate::tools::{self, Action};
use crate::view;
use crate::w32;

/// Posted to the viewer when a playlist row is right-clicked: `wparam` is the row plus one, so
/// zero means the click missed the rows (the header or an empty area).
pub(crate) const ROW_MENU: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 4;

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

/// Whether a panel is open over the picture: viewer keys, wheel and gestures must not act
/// behind it.
pub fn panel_open() -> bool {
    state::with_state(|state| state.dialog != Dialog::None).unwrap_or(false)
}

/// A failed file command becomes the timed notice (`State.notice`): the viewer and the playlist
/// view both paint it, and the standard notice timer clears it.
fn fail_to_notice(hwnd: HWND, failure: Option<String>) {
    if let Some(message) = failure {
        state::with_state(|state| state.notice = Some(message));
        view::expire_notice(hwnd);
    }
    view::refresh(hwnd);
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
        Action::PageUp | Action::PageDown => {
            playlist::browse_page(hwnd, action == Action::PageDown);
        }
        Action::ShowPlaylist => {
            let visible = state::with_state(|state| {
                state.playlist_visible = !state.playlist_visible;
                if state.playlist_visible {
                    state.pause_slideshow();
                } else if !state.slideshow_paused_by_space {
                    state.resume_slideshow();
                }
                state.playlist_visible
            });
            if visible == Some(true) {
                playlist::show(hwnd);
            } else if visible == Some(false) {
                playlist::hide();
            }
            view::refresh(hwnd);
        }
        Action::Rotate => {
            state::with_state(|state| state.rotate_frames(90));
            view::refresh(hwnd);
        }
        Action::ZoomMode(_) | Action::FitOriginal | Action::ZoomIn | Action::ZoomOut => {
            // The keys zoom the picture's pane, which is the left half while the playlist is open.
            let image =
                state::with_state(|state| view::image_area(screen, state)).unwrap_or(screen);
            view::zoom_overlay::change(hwnd, image, action);
        }
        Action::ExactZoom => view::zoom_overlay::open(hwnd),
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
        Action::ToggleSlideshowPause => {
            state::with_state(|state| state.toggle_slideshow_pause());
            view::expire_notice(hwnd);
            view::refresh(hwnd);
        }
        Action::ShufflePlaylist => {
            state::with_state(|state| state.shuffle_playlist_rows());
            view::refresh(hwnd);
        }
        Action::UndoPlaylist => {
            state::with_state(|state| state.undo_playlist_edit());
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
            let paths = match action {
                Action::OpenFile => dialog::pick_files(hwnd, "Open images"),
                _ => dialog::pick_files(hwnd, "Add images"),
            };
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
        Action::RetryFolderScan => {
            state::with_state(|state| state.retry_folder_scan());
            panels::refresh_import(hwnd);
        }
        Action::OpenFolder | Action::AddFolder => {
            let folder = match action {
                Action::OpenFolder => dialog::pick_folder(hwnd, "Open folder"),
                _ => dialog::pick_folder(hwnd, "Add folder"),
            };
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
            let failure = state::with_state(|state| state.delete_current().err()).flatten();
            fail_to_notice(hwnd, failure);
        }
        Action::MoveToTarget => {
            let failure = state::with_state(|state| state.move_current_to_target().err()).flatten();
            fail_to_notice(hwnd, failure);
        }
        Action::Rename => ask_rename(hwnd),
        Action::SaveAs => save_as(hwnd),
        Action::ShowInExplorer => {
            let failure = state::with_state(|state| {
                state
                    .photos
                    .current()
                    .and_then(|path| crate::fileops::show_in_explorer(path).err())
            })
            .flatten();
            fail_to_notice(hwnd, failure);
        }
        Action::OpenPlaylist => {
            let path = dialog::pick_playlist(hwnd);
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
            let path = dialog::pick_playlist_destination(hwnd);
            if let Some(path) = path {
                let failure = state::with_state(|state| state.save_playlist(&path).err()).flatten();
                fail_to_notice(hwnd, failure);
            }
        }
        Action::RemoveSelected => {
            let Some(rows) = chosen_rows(hwnd) else {
                return;
            };
            state::with_state(|state| state.remove_many(&rows));
            view::refresh(hwnd);
        }
        Action::DeleteSelected => {
            let Some(rows) = chosen_rows(hwnd) else {
                return;
            };
            let failure = state::with_state(|state| state.delete_many(&rows)).flatten();
            fail_to_notice(hwnd, failure);
        }
        Action::ShowFolder => {
            let failure = pointer::chosen_path(hwnd)
                .and_then(|path| crate::fileops::show_in_explorer(&path).err());
            fail_to_notice(hwnd, failure);
        }
        Action::CopyImage => panels::copy_image(hwnd),
        Action::CopyFilePath => panels::copy_file_path(hwnd),
        Action::RemoveDuplicates => panels::remove_duplicates(hwnd),
        Action::TogglePlaylistColumn(column) => {
            playlist::toggle_column(column);
            view::refresh(hwnd);
        }
        Action::About => crate::about::show(hwnd),
        Action::ImageInfo => {
            if state::with_state(|state| state.request_image_info()) == Some(true) {
                panels::refresh_import(hwnd);
            }
        }
        Action::Quit => {
            let _ = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
        }
    }
}

mod keys;
use keys::ctrl_down;
pub use keys::{on_key, shift_held, SHORTCUT_GROUPS};

mod pointer;
use pointer::{chosen_path, chosen_rows};
pub use pointer::{on_drag, on_drag_end, on_drop, on_wheel, show_menu};
mod panels;
use panels::{ask_rename, open_panel, save_as, settle_file_selection};
