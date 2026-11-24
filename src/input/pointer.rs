use std::cell::{Cell, RefCell};

use super::*;

thread_local! {
    /// The plain wheel's leftover units, so a high-resolution wheel still advances per 120.
    static WHEEL_REMAINDER: Cell<i32> = const { Cell::new(0) };
}

/// Adds one wheel message to the signed remainder and returns the image steps it completed.
/// Truncation toward zero keeps a partial turn for the next message, and opposite turns cancel.
fn completed_steps(remainder: &mut i32, delta: i32) -> i32 {
    *remainder += delta;
    let steps = *remainder / 120;
    *remainder -= steps * 120;
    // Wheel up (positive) goes to the previous image, wheel down to the next.
    -steps
}

/// Ctrl+wheel zooms around the pointer. The wheel alone steps through the list.
pub fn on_wheel(hwnd: HWND, wparam: WPARAM, position: LPARAM) {
    let delta = w32::wheel_delta(wparam);
    let zoom = unsafe { ctrl_down() };
    let screen = unsafe { w32::client_size(hwnd) };

    if !zoom {
        let steps = WHEEL_REMAINDER.with(|cell| {
            let mut remainder = cell.get();
            let steps = completed_steps(&mut remainder, delta);
            cell.set(remainder);
            steps
        });
        // Random order picks one image per `step`, so each completed turn navigates alone.
        for _ in 0..steps.unsigned_abs() {
            view::navigate(hwnd, steps.signum());
        }
        return;
    }
    let image = state::with_state(|state| view::image_area(screen, state)).unwrap_or(screen);
    view::zoom_overlay::wheel(
        hwnd,
        image,
        delta,
        (w32::pointer_x(position), w32::pointer_y(position)),
    );
}

/// A drag moves the image within the canvas at the configured pan speed.
pub fn on_drag(hwnd: HWND, lparam: LPARAM) {
    let client = unsafe { w32::client_size(hwnd) };
    let pointer = (w32::pointer_x(lparam), w32::pointer_y(lparam));
    state::with_state(|state| {
        if !state.knobs.free_pan {
            return;
        }
        let screen = view::image_area(client, state);
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

/// What a context menu command acts on: the rows the menu was opened on, or for a key command
/// the list selection / shown image.
pub(super) enum MenuRows {
    /// The rows the menu was opened on, still valid for the current playlist. `row` is the row
    /// the menu was opened over, when it was opened on a row rather than the selection.
    Explicit {
        rows: Vec<usize>,
        row: Option<usize>,
    },
    /// The playlist was rebuilt after the menu opened, so its row numbers mean something else.
    Stale,
    /// No menu is open; the caller uses the list selection or the shown image.
    Keyboard,
}

struct MenuChoice {
    rows: Vec<usize>,
    row: Option<usize>,
}

struct MenuTarget {
    token: u64,
    choice: MenuChoice,
}

thread_local! {
    static MENU_TARGET: RefCell<Option<MenuTarget>> = const { RefCell::new(None) };
}

/// The right-click menu at the pointer. `row` is the row that was right-clicked, `None` when the
/// click missed the rows. The menu keeps the rows it was opened on, so a file command never acts
/// on a selection the user changed while the menu was up.
pub fn show_menu(hwnd: HWND, row: Option<usize>) {
    let row_commands = open_menu_target(row);
    if let Some(action) = tools::show_menu(hwnd, row_commands) {
        apply_action(hwnd, action);
    }
    MENU_TARGET.with(|cell| cell.borrow_mut().take());
    if !panel_open() {
        // A panel the menu opened keeps the focus it chose.
        let _ = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(hwnd)) };
    }
}

/// The rows a menu opened on `clicked` acts on: the selection when it contains the clicked row,
/// otherwise the clicked row, otherwise the existing selection. `None` disables row commands.
fn menu_target(clicked: Option<usize>, selection: &[usize]) -> Option<MenuChoice> {
    match clicked {
        Some(row) if selection.contains(&row) => Some(MenuChoice {
            rows: selection.to_vec(),
            row: Some(row),
        }),
        Some(row) => Some(MenuChoice {
            rows: vec![row],
            row: Some(row),
        }),
        None if !selection.is_empty() => Some(MenuChoice {
            rows: selection.to_vec(),
            row: None,
        }),
        None => None,
    }
}

/// Keeps the rows the menu is opened on, so the commands it returns can find them. `false`
/// means the playlist has no row to act on, which disables the file commands.
fn open_menu_target(clicked: Option<usize>) -> bool {
    if !playlist::is_visible() {
        return false;
    }
    let Some(choice) = menu_target(clicked, &playlist::selected()) else {
        return false;
    };
    let Some(token) = state::with_state(|state| state.photos.rows_token()) else {
        return false;
    };
    MENU_TARGET.with(|cell| *cell.borrow_mut() = Some(MenuTarget { token, choice }));
    true
}

/// Takes the rows the menu was opened on. A playlist rebuild since then makes them stale, so the
/// command does nothing rather than acting on whatever now sits at those row numbers.
pub(super) fn take_menu_rows() -> MenuRows {
    let Some(target) = MENU_TARGET.with(|cell| cell.borrow_mut().take()) else {
        return MenuRows::Keyboard;
    };
    if state::with_state(|state| state.photos.rows_token() == target.token) == Some(true) {
        MenuRows::Explicit {
            rows: target.choice.rows,
            row: target.choice.row,
        }
    } else {
        MenuRows::Stale
    }
}

/// The rows a file command applies to: the rows its menu was opened on, or for a key command the
/// list selection / shown image. A menu whose rows are stale does nothing.
pub(super) fn chosen_rows(hwnd: HWND) -> Option<Vec<usize>> {
    match take_menu_rows() {
        MenuRows::Explicit { rows, .. } => Some(rows),
        MenuRows::Stale => {
            fail_to_notice(hwnd, Some("Playlist changed. Open the menu again.".into()));
            None
        }
        MenuRows::Keyboard => Some(targets()),
    }
}

/// The path a file command acts on: the row its menu was opened on, or the shown image for a key
/// command.
pub(super) fn chosen_path(hwnd: HWND) -> Option<PathBuf> {
    match take_menu_rows() {
        MenuRows::Explicit { rows, row } => state::with_state(|state| {
            row.or_else(|| rows.first().copied())
                .and_then(|row| state.photos.path_at(row).cloned())
        })
        .flatten(),
        MenuRows::Stale => {
            fail_to_notice(hwnd, Some("Playlist changed. Open the menu again.".into()));
            None
        }
        MenuRows::Keyboard => state::with_state(|state| state.photos.current().cloned()).flatten(),
    }
}

#[cfg(test)]
mod tests {
    use super::{completed_steps, menu_target};

    #[test]
    fn a_menu_targets_the_selection_containing_the_clicked_row() {
        let rows =
            |choice: Option<super::MenuChoice>| choice.map(|choice| (choice.rows, choice.row));
        assert_eq!(
            rows(menu_target(Some(2), &[1, 2, 5])),
            Some((vec![1, 2, 5], Some(2)))
        );
        assert_eq!(
            rows(menu_target(Some(2), &[1, 5])),
            Some((vec![2], Some(2)))
        );
        assert_eq!(rows(menu_target(Some(2), &[])), Some((vec![2], Some(2))));
        assert_eq!(rows(menu_target(None, &[1, 5])), Some((vec![1, 5], None)));
        assert_eq!(rows(menu_target(None, &[])), None);
    }

    #[test]
    fn plain_wheel_deltas_accumulate_into_whole_steps() {
        let steps = |deltas: &[i32]| {
            let mut remainder = 0;
            deltas
                .iter()
                .map(|&delta| completed_steps(&mut remainder, delta))
                .sum::<i32>()
        };
        assert_eq!(steps(&[-30, -30, -30, -30]), 1);
        assert_eq!(steps(&[-240]), 2);
        assert_eq!(steps(&[-60, 60]), 0, "opposite partial turns cancel");
        assert_eq!(steps(&[120]), -1, "wheel up goes to the previous image");
    }
}

/// A drop on the playlist pane appends. A drop on the image opens its content.
pub unsafe fn on_drop(hwnd: HWND, drop: HDROP, from_playlist: bool) {
    let count = unsafe { DragQueryFileW(drop, u32::MAX, None) } as usize;
    let mut paths = Vec::new();
    for index in 0..count {
        let capacity = unsafe { DragQueryFileW(drop, index as u32, None) } as usize + 1;
        let mut buffer = vec![0u16; capacity];
        let length = unsafe { DragQueryFileW(drop, index as u32, Some(&mut buffer)) } as usize;
        buffer.truncate(length);
        paths.push(PathBuf::from(OsString::from_wide(&buffer)));
    }

    if from_playlist {
        state::with_state(|state| state.append_drop(paths));
    } else if paths.len() == 1 && crate::rgpl::is_playlist_file(&paths[0]) {
        // A single playlist opens like the playlist command: its rows keep file order.
        let failure = state::with_state(|state| state.open_playlist(&paths[0]).err()).flatten();
        fail_to_notice(hwnd, failure);
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
