use super::*;
/// Paints the panel's background, its heading and the labels. Fields, buttons and the box of a
/// check box are child controls, so Windows draws those.
pub fn draw(dialog: &Dialog, hdc: HDC, width: i32, height: i32) {
    unsafe {
        let panel = if *dialog == Dialog::Shortcuts {
            width
        } else {
            settings_width()
        };
        crate::style::surface(hdc, (4, 4, panel, height - 4), PANEL_COLOR);
        w32::draw_text(hdc, MARGIN, 8, title(dialog), TEXT_HEADING);
        if *dialog == Dialog::Shortcuts {
            draw_shortcuts(hdc, width, height);
            return;
        }
        if let Dialog::Text(error) = dialog {
            w32::draw_wrapped(
                hdc,
                MARGIN,
                row_y(1) + BUTTON_HEIGHT + 14,
                width.min(panel) - 2 * MARGIN,
                error,
                COLORREF(0x0099_B5FF),
            );
        }
        for (index, row) in rows(dialog) {
            // A check box row has its box in the control and its text here, right of the box.
            let x = match row.kind {
                Kind::CheckBox => CHECK_LABEL_X,
                Kind::TextField | Kind::PathField => MARGIN,
            };
            w32::draw_text(hdc, x, row_y(index) + ROW_LABEL_TOP, row.label, TEXT_COLOR);
        }
    }
}

/// The shortcut table: key in blue, what it does in white. A description is wrapped into its
/// column instead of cut, a full column continues in the next, and what still does not fit is
/// reached with the wheel, which `view::wndproc` sends to `scroll_by`.
fn draw_shortcuts(hdc: HDC, width: i32, height: i32) {
    use windows::Win32::Graphics::Gdi::{IntersectClipRect, RestoreDC, SaveDC};

    let (_, button_top, _, _) = shortcut_button_rect((width as u32, height as u32));
    let bottom = (button_top - MARGIN / 2).max(HEADER);
    let usable = bottom - HEADER;
    let columns = (width / COLUMN_WIDTH).max(1);
    let text_width = (width.min(COLUMN_WIDTH) - KEY_COLUMN - MARGIN).max(1);
    let mut column = 0;
    let mut y = HEADER;
    let mut tallest = 0;
    let mut layout = Vec::new();
    let row_height =
        |description| unsafe { w32::text_height(hdc, text_width, description) }.max(ROW_HEIGHT) + 4;
    for &(heading, rows) in crate::input::SHORTCUT_GROUPS {
        let Some((_, first)) = rows.first() else {
            continue;
        };
        for (index, (key, what)) in std::iter::once(("", heading))
            .chain(rows.iter().copied())
            .enumerate()
        {
            let is_heading = index == 0;
            let needed = if is_heading {
                ROW_HEIGHT + 8
            } else {
                row_height(what)
            };
            let block = if is_heading {
                needed + row_height(first)
            } else {
                needed
            };
            if index != 1 && y > HEADER && y + block > bottom && column + 1 < columns {
                tallest = tallest.max(y - HEADER);
                column += 1;
                y = HEADER;
            }
            layout.push((column, y, needed, key, what));
            y += needed;
        }
    }
    let limit = (tallest.max(y - HEADER) - usable).max(0);
    SCROLL_MAX.set(limit);
    let scroll = SCROLL.get().min(limit);
    SCROLL.set(scroll);

    let saved = unsafe { SaveDC(hdc) };
    if saved == 0 {
        return;
    }
    let _ = unsafe { IntersectClipRect(hdc, 0, HEADER, width, bottom) };
    for (column, y, needed, key, what) in layout {
        let top = y - scroll;
        if top + needed > HEADER && top < bottom {
            unsafe {
                let x = column * COLUMN_WIDTH;
                if key.is_empty() {
                    w32::draw_text(hdc, x + MARGIN, top + 6, what, TEXT_HEADING);
                } else {
                    w32::draw_text(hdc, x + MARGIN, top, key, TEXT_HEADING);
                    w32::draw_wrapped(hdc, x + KEY_COLUMN, top, text_width, what, TEXT_COLOR);
                }
            }
        }
    }
    let _ = unsafe { RestoreDC(hdc, saved) };
}

/// Text colour and background for the panel's controls. Without a brush in the panel's own colour
/// a control repaints over whatever was left under it, which leaves its label unreadable.
pub fn color_control(hdc: HDC) -> HBRUSH {
    let _ = unsafe { w32::static_colour(hdc, TEXT_COLOR) };
    PANEL_BRUSH.with(|brush| {
        let brush = &mut *brush.borrow_mut();
        if brush.is_none() {
            *brush = Some(unsafe { CreateSolidBrush(PANEL_COLOR) });
        }
        // A failed brush creation would leave the control with no background at all.
        brush.unwrap_or_else(|| unsafe { GetSysColorBrush(COLOR_WINDOW) })
    })
}
