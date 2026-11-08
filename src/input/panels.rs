use super::*;

pub(super) fn refresh_import(hwnd: HWND) {
    if state::with_state(|state| state.notice.is_some()) == Some(true) {
        view::expire_notice(hwnd);
    }
    view::refresh(hwnd);
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
/// Opens the prompt for a rename, copy or move, pre-filled with what the operation starts from.
pub(super) fn ask(hwnd: HWND, operation: Pending) {
    state::with_state(|state| {
        let Some(path) = state.photos.current() else {
            return;
        };
        state.pending = Some((operation, path.clone()));
        state.dialog = Dialog::Text(String::new());
        dialog::open(
            &state.dialog,
            &state.config,
            hwnd,
            state.pending.as_ref().map(|(operation, _)| operation),
            Some(path),
        );
    });
    view::refresh(hwnd);
}

/// Opens a settings or shortcuts panel: its child controls are built by `dialog`.
pub(super) fn open_panel(hwnd: HWND, kind: Dialog) {
    state::with_state(|state| {
        dialog::close();
        if kind == Dialog::Shortcuts && state.dialog == Dialog::Shortcuts {
            state.dialog = Dialog::None;
        } else {
            state.dialog = kind;
            dialog::open(&state.dialog, &state.config, hwnd, None, None);
        }
    });
    view::refresh(hwnd);
}

/// Make the new selection visible and require a fresh command before touching it.
pub(super) fn settle_file_selection(hwnd: HWND, action: Action) -> bool {
    if matches!(
        action,
        Action::RemoveItem
            | Action::DeleteFile
            | Action::MoveToTarget
            | Action::Rename
            | Action::CopyToFolder
            | Action::ShowInExplorer
            | Action::RemoveSelected
            | Action::DeleteSelected
            | Action::ShowFolder
    ) && state::with_state(|state| state.navigation_pending) == Some(true)
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
