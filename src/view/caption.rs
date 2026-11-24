use super::*;
use std::cell::RefCell;
use std::fmt::Write;

thread_local! {
    static LAST_TITLE: RefCell<String> = const { RefCell::new(String::new()) };
}

pub(super) fn update(hwnd: HWND) {
    let area = unsafe { w32::client_size(hwnd) };
    let title = state::with_state(|state| {
        if state.navigation_pending || state.load_pending {
            return None;
        }
        let Some(path) = state.photos.current() else {
            return Some(TITLE.to_string());
        };
        let area = crate::view::image_area(area, state);
        let mut title = format!(
            "{TITLE} | {} ({}/{})",
            w32::file_name(path),
            state.photos.index + 1,
            state.photos.len()
        );
        if let Some(frame) = state.frame() {
            let _ = write!(
                title,
                " | {:.2}% | {} x {}",
                state.factor(area) * 100.0,
                frame.width,
                frame.height
            );
        }
        if let Some(size) = state.file_size {
            let _ = write!(title, " | {}", file_size(size));
        }
        Some(title)
    })
    .flatten();
    if let Some(title) = title {
        let changed = LAST_TITLE.with_borrow_mut(|last| {
            if *last == title {
                return false;
            }
            last.clone_from(&title);
            true
        });
        if changed {
            let _ = unsafe { SetWindowTextW(hwnd, PCWSTR(w32::wide(&title).as_ptr())) };
        }
    }
}

pub(super) fn clear() {
    LAST_TITLE.with_borrow_mut(String::clear);
}

fn file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    if bytes < MB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    }
}

#[cfg(test)]
mod tests {
    use crate::dialog::Dialog;

    #[test]
    fn visible_playlist_limits_picture_to_left_half() {
        let mut state = crate::state::State::new(crate::test_path("unused-config.json"), &[]);
        let area = (1000, 500);
        state.dialog = Dialog::None;
        state.playlist_visible = true;
        assert_eq!(crate::view::image_area(area, &state), (500, 500));
        state.playlist_visible = false;
        assert_eq!(crate::view::image_area(area, &state), area);
        state.playlist_visible = true;
        state.dialog = Dialog::Shortcuts;
        assert_eq!(crate::view::image_area(area, &state), area);
    }

    #[test]
    fn file_sizes_switch_to_megabytes_at_one_megabyte() {
        assert_eq!(super::file_size(305_439), "298.28 KB");
        assert_eq!(super::file_size(1024 * 1024), "1.00 MB");
    }
}
