use super::*;
pub fn show_menu(hwnd: HWND) -> Option<Action> {
    let playlist =
        crate::state::with_state(|s| s.playlist_visible.then_some(s.knobs.loop_slideshow))?;
    // While the list of files is on screen, a right-click is about the rows: the menu of the list
    // replaces the menu of the picture, which is one `Esc` away.
    if let Some(repeat) = playlist {
        return playlist_menu(hwnd, repeat);
    }
    let items = crate::state::with_state(|s| items(s, &s.config))?;
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        for (index, item) in items.iter().enumerate() {
            let mut flags = if item.action == Action::Separator {
                MF_SEPARATOR
            } else {
                MF_STRING
            };
            if item.checked {
                flags |= MF_CHECKED;
            }
            if !item.enabled {
                flags |= MF_GRAYED;
            }
            let label = w32::wide(&item.label);
            let _ = AppendMenuW(menu, flags, MENU_BASE + index, PCWSTR(label.as_ptr()));
        }
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        let chosen = TrackPopupMenuEx(
            menu,
            (TPM_RETURNCMD.0 | TPM_RIGHTBUTTON.0) as u32,
            point.x,
            point.y,
            hwnd,
            None,
        );
        let _ = DestroyMenu(menu);
        if chosen.0 <= 0 {
            return None;
        }
        items
            .get((chosen.0 as usize) - MENU_BASE)
            .map(|item| item.action)
    }
}

/// The menu of the list of files: what to do with the rows, all of them or the selected ones.
/// The picture's own menu is one `Esc` away, so this one leaves the zoom out.
fn playlist_menu(hwnd: HWND, repeat: bool) -> Option<Action> {
    // One id list for the menu and its sub-menu: a leaf id is only ever asked for after it was
    // handed out, so both can share the running counter.
    let mut by_id: Vec<(usize, Action)> = Vec::new();
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        let sort = CreatePopupMenu().ok()?;
        let mut add = |target: HMENU, label: &str, action: Action, checked: bool| {
            let id = MENU_BASE + by_id.len();
            by_id.push((id, action));
            let mut flags = MF_STRING;
            if checked {
                flags |= MF_CHECKED;
            }
            let text = w32::wide(label);
            let _ = AppendMenuW(target, flags, id, PCWSTR(text.as_ptr()));
        };
        let line = |target: HMENU| {
            let _ = AppendMenuW(target, MF_SEPARATOR, 0, PCWSTR(std::ptr::null()));
        };

        add(menu, "Add a file to the list…  A", Action::AddFiles, false);
        add(
            menu,
            "Add a folder to the list…  Shift+A",
            Action::AddFolder,
            false,
        );
        add(menu, "Open playlist…  Ctrl+O", Action::OpenPlaylist, false);
        add(menu, "Save playlist…  Ctrl+S", Action::SavePlaylist, false);
        line(menu);
        add(
            menu,
            "Remove selected from playlist  Del",
            Action::RemoveSelected,
            false,
        );
        add(
            menu,
            "Send selected to Recycle Bin  Ctrl+Del",
            Action::DeleteSelected,
            false,
        );
        add(
            menu,
            "Show the containing folder in Explorer",
            Action::ShowFolder,
            false,
        );
        line(menu);
        add(sort, "Name ascending", Action::SortByName(false), false);
        add(sort, "Name descending", Action::SortByName(true), false);
        add(sort, "Shuffle", Action::ShufflePlaylist, false);
        let _ = AppendMenuW(
            menu,
            MF_POPUP,
            sort.0 as usize,
            PCWSTR(w32::wide("Sort by").as_ptr()),
        );
        line(menu);
        add(menu, "Repeat the slideshow", Action::ToggleRepeat, repeat);
        add(
            menu,
            "Show / hide the list of files  L",
            Action::ShowPlaylist,
            true,
        );
        line(menu);
        add(menu, "&About…", Action::About, false);

        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        let chosen = TrackPopupMenuEx(
            menu,
            (TPM_RETURNCMD.0 | TPM_RIGHTBUTTON.0) as u32,
            point.x,
            point.y,
            hwnd,
            None,
        );
        let _ = DestroyMenu(menu);
        by_id
            .iter()
            .find(|(id, _)| *id == chosen.0 as usize)
            .map(|(_, action)| *action)
    }
}
