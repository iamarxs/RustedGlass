use super::*;
/// A row of the settings panel changed: copy it to the configuration and update what the viewer
/// reads from it. Reached by a click on the checkbox control or a change in a field.
pub(super) unsafe fn toggle_setting(hwnd: HWND, id: usize) {
    state::with_state(|state| {
        dialog::apply_to_config(&state.dialog, &mut state.config, Some(id));
        state.reload_knobs();
    });
    refresh(hwnd);
}

/// What the panel's button does. It is the same thing the painted button used to be, chosen by
/// which panel is open.
pub(super) fn press_apply(hwnd: HWND) {
    match state::with_state(|state| state.dialog.clone()) {
        Some(Dialog::Settings) => save_settings(hwnd),
        Some(Dialog::Text(_)) => confirm_prompt(hwnd, &dialog::prompt_text().unwrap_or_default()),
        _ => input::apply_action(hwnd, tools::Action::ClosePanel),
    }
}

/// "Save & Close": write the panel into the config, keep it in memory, and drop it on disk.
fn save_settings(hwnd: HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    let result = dialog::slideshow_intervals()
        .map_err(str::to_owned)
        .and_then(|(minimum, maximum)| {
            state::with_state(|state| {
                dialog::apply_to_config(&state.dialog, &mut state.config, None);
                state.config.set("SlideshowInterval", minimum.into());
                state.config.set("SlideshowIntervalTo", maximum.into());
                state.reload_knobs();
                state.config.save_user()?;
                state.message = None;
                state.dialog = Dialog::None;
                Ok(())
            })
            .unwrap_or_else(|| Err("Settings are busy. Please try again.".into()))
        });
    if let Err(error) = result {
        let text = w32::wide(&error);
        let _ = unsafe {
            MessageBoxW(
                Some(hwnd),
                PCWSTR(text.as_ptr()),
                windows::core::w!("Settings could not be saved"),
                MB_OK | MB_ICONERROR,
            )
        };
        return;
    }
    dialog::close();
    refresh(hwnd);
}

/// Hands the picked folder to the path field that asked for it.
pub(super) fn pick_folder(hwnd: HWND, index: usize) {
    if let Some(path) = dialog::pick_folder(hwnd) {
        dialog::set_field_text(BASE_ID + index, &path.display().to_string());
    }
}

/// Enter in the rename / copy / move field.
pub(super) fn confirm_prompt(hwnd: HWND, value: &str) {
    use windows::Win32::UI::WindowsAndMessaging::GetDlgItem;

    match state::with_state(|state| state.apply_pending(value)) {
        Some(Ok(_)) => {
            dialog::close();
            state::with_state(|state| state.dialog = Dialog::None);
            refresh(hwnd);
        }
        Some(Err(error)) => {
            state::with_state(|state| state.dialog = Dialog::Text(error));
            unsafe {
                if let Ok(field) = GetDlgItem(Some(hwnd), PROMPT_ID as i32) {
                    let _ = SetFocus(Some(field));
                }
                w32::invalidate(hwnd);
            }
        }
        None => {}
    }
}
