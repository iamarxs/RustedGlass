use super::*;
/// The wheel: Ctrl zooms, otherwise it steps through the list. A `MouseWheelActions` key
/// could offer more actions; supporting them means reading that key here.
pub fn on_wheel(hwnd: HWND, wparam: WPARAM) {
    let delta = w32::wheel_delta(wparam);
    let zoom = unsafe { ctrl_down() };
    let screen = unsafe { w32::client_size(hwnd) };

    if !zoom {
        view::navigate(hwnd, if delta < 0 { 1 } else { -1 });
        return;
    }
    view::zoom_overlay::step(hwnd, screen, delta);
}

/// A drag moves the image within the canvas at the configured pan speed.
pub fn on_drag(hwnd: HWND, lparam: LPARAM) {
    let screen = unsafe { w32::client_size(hwnd) };
    let pointer = (w32::pointer_x(lparam), w32::pointer_y(lparam));
    state::with_state(|state| {
        if !state.knobs.free_pan {
            return;
        }
        let (x, y) = state.drag.unwrap_or(pointer);
        if let Some(bitmap) = state.frame() {
            let step = state.knobs.pan_speed as f64 / 20.0;
            let delta = (
                ((pointer.0 - x) as f64 * step).round() as i32,
                ((pointer.1 - y) as f64 * step).round() as i32,
            );
            state.offset = crate::render::pan_offset(
                (bitmap.width, bitmap.height),
                screen,
                state.factor(screen),
                state.offset,
                delta,
            );
        }
        state.drag = Some(pointer);
    });
    unsafe { w32::invalidate(hwnd) };
}

pub fn on_drag_end() {
    state::with_state(|state| state.drag = None);
}

/// The right-click menu at the pointer.
pub fn show_menu(hwnd: HWND) {
    let chosen = tools::show_menu(hwnd);
    if let Some(action) = chosen {
        apply_action(hwnd, action);
    }
}

/// File, folder and playlist drops append in the playlist view and replace the list in the
/// image view; dropped `.rgpl` files load their rows like any other dropped content.
pub unsafe fn on_drop(hwnd: HWND, drop: HDROP) {
    let count = unsafe { DragQueryFileW(drop, u32::MAX, None) } as usize;
    let mut paths = Vec::new();
    for index in 0..count {
        let capacity = unsafe { DragQueryFileW(drop, index as u32, None) } as usize + 1;
        let mut buffer = vec![0u16; capacity];
        let length = unsafe { DragQueryFileW(drop, index as u32, Some(&mut buffer)) } as usize;
        buffer.truncate(length);
        paths.push(PathBuf::from(OsString::from_wide(&buffer)));
    }

    if playlist::is_visible() {
        state::with_state(|state| state.append_drop(paths));
    } else if paths.len() == 1 && paths[0].is_dir() {
        state::with_state(|state| state.open_folder(&paths[0]));
    } else {
        state::with_state(|state| state.open_files(&paths));
    }
    panels::refresh_import(hwnd);
}

/// The rows an operation on "the selected files" applies to: the rows the list has selected, or
/// the row of the picture on screen when none is. Asked for outside a `with_state` borrow, so a
/// caller can borrow the state afterwards.
pub(super) fn targets() -> Vec<usize> {
    let rows = playlist::selected();
    if rows.is_empty() {
        state::with_state(|state| vec![state.photos.index]).unwrap_or_default()
    } else {
        rows
    }
}
