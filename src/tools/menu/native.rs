use super::*;

struct Menu(HMENU);

impl Drop for Menu {
    fn drop(&mut self) {
        let _ = unsafe { DestroyMenu(self.0) };
    }
}

pub(super) struct ContextMenu {
    root: Menu,
    actions: Vec<Action>,
}

impl ContextMenu {
    pub(super) fn new() -> Option<Self> {
        Some(Self {
            root: Menu(unsafe { CreatePopupMenu() }.ok()?),
            actions: Vec::new(),
        })
    }

    pub(super) fn root(&self) -> HMENU {
        self.root.0
    }

    pub(super) fn group(&self, label: &str) -> Option<HMENU> {
        let child = Menu(unsafe { CreatePopupMenu() }.ok()?);
        let handle = child.0;
        let label = w32::wide(label);
        unsafe {
            AppendMenuW(
                self.root.0,
                MF_POPUP,
                handle.0 as usize,
                PCWSTR(label.as_ptr()),
            )
        }
        .ok()?;
        // DestroyMenu on the root now owns this submenu.
        std::mem::forget(child);
        Some(handle)
    }

    pub(super) fn add(
        &mut self,
        menu: HMENU,
        action: Action,
        label: &str,
        enabled: bool,
        checked: bool,
    ) {
        let mut flags = MF_STRING;
        if !enabled {
            flags |= MF_GRAYED;
        }
        if checked {
            flags |= MF_CHECKED;
        }
        let label = w32::wide(label);
        let id = MENU_BASE + self.actions.len();
        if unsafe { AppendMenuW(menu, flags, id, PCWSTR(label.as_ptr())) }.is_ok() {
            self.actions.push(action);
        }
    }

    pub(super) fn line(&self, menu: HMENU) {
        let _ = unsafe { AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null()) };
    }

    pub(super) fn track(&self, hwnd: HWND) -> Option<Action> {
        let mut point = POINT::default();
        unsafe { GetCursorPos(&mut point) }.ok()?;
        let chosen = unsafe {
            TrackPopupMenuEx(
                self.root.0,
                (TPM_RETURNCMD.0 | TPM_RIGHTBUTTON.0) as u32,
                point.x,
                point.y,
                hwnd,
                None,
            )
        };
        let index = (chosen.0 as usize).checked_sub(MENU_BASE)?;
        self.actions.get(index).copied()
    }
}
