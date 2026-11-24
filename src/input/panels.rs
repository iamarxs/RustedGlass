use super::*;

pub(super) fn refresh_import(hwnd: HWND) {
    if state::with_state(|state| state.notice.is_some()) == Some(true) {
        view::expire_notice(hwnd);
    }
    view::refresh(hwnd);
}

pub(super) fn remove_duplicates(hwnd: HWND) {
    state::with_state(|state| state.request_remove_duplicates());
    refresh_import(hwnd);
}

pub(super) fn copy_file_path(hwnd: HWND) {
    let Some(path) = super::chosen_path(hwnd) else {
        return;
    };
    let message = match crate::fileops::copy_path(hwnd, &path) {
        Ok(()) => "File path copied to clipboard.".to_owned(),
        Err(error) => error,
    };
    fail_to_notice(hwnd, Some(message));
}

pub(super) fn copy_image(hwnd: HWND) {
    state::with_state(|state| {
        if let Some(bitmap) = state.frame() {
            state.notice = Some(match bitmap.copy_to_clipboard(hwnd) {
                Ok(()) => "Image copied to clipboard.".into(),
                Err(error) => format!("Could not copy image: {error}"),
            });
        }
    });
    view::expire_notice(hwnd);
    unsafe { w32::invalidate(hwnd) };
}
pub(super) fn ask_rename(hwnd: HWND) {
    state::with_state(|state| {
        let Some(path) = state.photos.current() else {
            return;
        };
        state.pending = Some(path.clone());
        state.dialog = Dialog::Text(String::new());
        dialog::open(&state.dialog, &state.config, hwnd, Some(path));
    });
    view::refresh(hwnd);
}

pub(super) fn save_as(hwnd: HWND) {
    let Some(source) = state::with_state(|state| state.photos.current().cloned()).flatten() else {
        return;
    };
    let Some(target) = dialog::pick_image_destination(hwnd, &source) else {
        return;
    };
    if source
        .canonicalize()
        .is_ok_and(|path| target.canonicalize().is_ok_and(|target| target == path))
    {
        return;
    }
    let result = crate::atomic_file::copy(&source, &target);
    state::with_state(|state| {
        state.notice = Some(match result {
            Ok(()) => {
                if !state.photos.files.contains(&target) {
                    state.add_files(std::slice::from_ref(&target));
                }
                format!("Saved {}", target.display())
            }
            Err(error) => format!("Could not save image: {error}"),
        });
    });
    refresh_import(hwnd);
}

/// Opens a settings or shortcuts panel: its child controls are built by `dialog`.
pub(super) fn open_panel(hwnd: HWND, kind: Dialog) {
    state::with_state(|state| {
        dialog::close();
        if kind == Dialog::Shortcuts && state.dialog == Dialog::Shortcuts {
            state.dialog = Dialog::None;
        } else {
            state.dialog = kind;
            dialog::open(&state.dialog, &state.config, hwnd, None);
        }
    });
    view::refresh(hwnd);
}

/// Make the new selection visible and require a fresh command before touching it.
pub(super) fn settle_file_selection(hwnd: HWND, action: Action) -> bool {
    if matches!(
        action,
        Action::RemoveItem
            | Action::RemoveSelected
            | Action::RemoveDuplicates
            | Action::UndoPlaylist
            | Action::DeleteFile
            | Action::DeleteSelected
            | Action::MoveToTarget
            | Action::Rename
            | Action::ShufflePlaylist
            | Action::SavePlaylist
    ) && state::read_state(|state| state.browsing_import_preview()) == Some(true)
    {
        fail_to_notice(
            hwnd,
            Some("Wait for the scan to finish before changing the playlist.".into()),
        );
        return true;
    }
    if matches!(
        action,
        Action::RemoveItem
            | Action::DeleteFile
            | Action::MoveToTarget
            | Action::Rename
            | Action::SaveAs
            | Action::ShowInExplorer
            | Action::CopyFilePath
            | Action::RemoveSelected
            | Action::DeleteSelected
            | Action::ShowFolder
    ) && state::with_state(|state| state.navigation_pending || state.load_pending) == Some(true)
    {
        state::with_state(|state| {
            state.finish_navigation();
            state.notice = Some("Image selection changed. Repeat the file command.".into());
        });
        view::expire_notice(hwnd);
        view::refresh(hwnd);
        let _ = unsafe { windows::Win32::Graphics::Gdi::UpdateWindow(hwnd) };
        return true;
    }
    false
}
