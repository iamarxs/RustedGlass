use super::*;
/// Opens a panel: builds the child controls of `parent`. The caller stores the `Dialog` in its
/// own state; this only mirrors it into Win32 controls.
pub fn open(
    dialog: &Dialog,
    config: &Config,
    parent: HWND,
    pending: Option<&Pending>,
    file: Option<&Path>,
) {
    close();
    SCROLL.with(|cell| cell.set(0));
    SCROLL_MAX.with(|cell| cell.set(0));
    let instance = w32::instance();
    let HGDIOBJ(font) = crate::style::font();
    let all = rows(dialog);
    for (index, row) in all {
        let class = w32::wide(match row.kind {
            Kind::CheckBox => "BUTTON",
            Kind::TextField | Kind::PathField => "EDIT",
        });
        // `ES_AUTOHSCROLL` belongs to edit controls only: its value is `BS_BITMAP` for a button,
        // which would draw the label as a bitmap, that is, not at all.
        let extra = WINDOW_STYLE(if row.kind == Kind::CheckBox {
            0
        } else {
            ES_AUTOHSCROLL as u32
        });
        // A check box is drawn by the parent, not by its control: an unthemed button paints its
        // window text with the control's brush, and that came out unreadable on the dark panel.
        let label = match row.kind {
            Kind::CheckBox => String::new(),
            Kind::TextField | Kind::PathField => value_text(config, row),
        };
        let text = w32::wide(&label);
        let style = match row.kind {
            Kind::CheckBox => {
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32)
            }
            Kind::TextField | Kind::PathField => WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER,
        } | extra;
        let x = match row.kind {
            Kind::CheckBox => MARGIN,
            Kind::TextField => MARGIN + LABEL_WIDTH + 8,
            Kind::PathField => MARGIN + PATH_LABEL_WIDTH,
        };
        let width = match row.kind {
            Kind::CheckBox => CHECK_WIDTH,
            Kind::TextField => FIELD_WIDTH,
            Kind::PathField => PATH_FIELD_WIDTH,
        };
        // An edit control keeps its text; the value is the label of a number or path row.
        let initial = text.as_ptr();
        match create(
            class.as_ptr(),
            initial,
            style,
            x,
            row_y(index),
            width,
            ROW_HEIGHT - 4,
            parent,
            BASE_ID + index,
            instance,
        ) {
            Ok(handle) => {
                unsafe {
                    // A check box keeps its theme: Windows then draws the box in the system's
                    // palette with a tick that is visible on a dark panel. Everything else has to
                    // be unthemed to take the panel's brush.
                    if row.kind != Kind::CheckBox {
                        untheme(handle);
                    }
                    let _ = SendMessageW(
                        handle,
                        WM_SETFONT,
                        Some(WPARAM(font as usize)),
                        Some(LPARAM(0)),
                    );
                }
                if row.kind == Kind::CheckBox {
                    set_checked(handle, config.get_bool(row.key, row.default == "true"));
                }
                CONTROLS.with(|controls| controls.borrow_mut().push((BASE_ID + index, handle)));
            }
            Err(error) => eprintln!(
                "rustedglass: could not create the {} control ({})",
                row.key, error
            ),
        }
        if row.kind == Kind::PathField {
            // A real button: it takes focus, Windows draws it, and its click arrives as a
            // `WM_COMMAND` carrying `PICK_BASE + index`.
            let (x, y, width, height) = pick_rect(row_y(index));
            if let Ok(handle) = create(
                w32::wide("BUTTON").as_ptr(),
                w32::wide("Pick…").as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32),
                x,
                y,
                width,
                height,
                parent,
                PICK_BASE + index,
                instance,
            ) {
                untheme(handle);
                let _ = unsafe {
                    SendMessageW(
                        handle,
                        WM_SETFONT,
                        Some(WPARAM(font as usize)),
                        Some(LPARAM(0)),
                    )
                };
                CONTROLS.with(|controls| controls.borrow_mut().push((PICK_BASE + index, handle)));
            }
        }
    }
    if matches!(dialog, Dialog::Text(_)) {
        let class = w32::wide("EDIT");
        let text = w32::wide(&prefill(pending, file));
        let style =
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER | WINDOW_STYLE(ES_AUTOHSCROLL as u32);
        if let Ok(handle) = create(
            class.as_ptr(),
            text.as_ptr(),
            style,
            MARGIN,
            row_y(0),
            settings_width() - MARGIN * 2,
            22,
            parent,
            PROMPT_ID,
            instance,
        ) {
            unsafe {
                let _ = SendMessageW(
                    handle,
                    WM_SETFONT,
                    Some(WPARAM(font as usize)),
                    Some(LPARAM(0)),
                );
                // Enter belongs to the field, so forward it as a click on "Apply".
                let _ = SetWindowSubclass(handle, Some(prompt_key), PROMPT_ID, parent.0 as usize);
                let _ = SetFocus(Some(handle));
            }
            CONTROLS.with(|controls| controls.borrow_mut().push((PROMPT_ID, handle)));
        }
    }

    // The panel's own button, again a real control: it takes focus, it is drawn by Windows, and
    // a click on it arrives as a `WM_COMMAND` carrying `APPLY_ID`.
    let (x, y, width, height) = apply_rect(dialog, parent);
    let label = w32::wide(button_label(dialog));
    if let Ok(handle) = create(
        w32::wide("BUTTON").as_ptr(),
        label.as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32),
        x,
        y,
        width,
        height,
        parent,
        APPLY_ID,
        instance,
    ) {
        untheme(handle);
        let _ = unsafe {
            SendMessageW(
                handle,
                WM_SETFONT,
                Some(WPARAM(font as usize)),
                Some(LPARAM(0)),
            )
        };
        CONTROLS.with(|controls| controls.borrow_mut().push((APPLY_ID, handle)));
    }
}

pub fn resize_shortcuts(size: (u32, u32)) {
    let handle = CONTROLS.with_borrow(|controls| {
        controls
            .iter()
            .find(|(id, _)| *id == APPLY_ID)
            .map(|(_, handle)| *handle)
    });
    if let Some(handle) = handle {
        let (x, y, width, height) = shortcut_button_rect(size);
        use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER};
        let _ = unsafe {
            SetWindowPos(
                handle,
                None,
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
        };
    }
}
