//! Keyboard routing and accessible names for the Settings panel's native controls.

use super::*;
use windows::core::GUID;
use windows::Win32::System::Com::{CoUninitialize, CLSCTX_SERVER};
use windows::Win32::UI::Accessibility::{IAccPropServices, Name_Property_GUID, PROPID_ACC_NAME};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VIRTUAL_KEY, VK_ESCAPE, VK_F1, VK_SHIFT, VK_TAB,
};
use windows::Win32::UI::Shell::RemoveWindowSubclass;
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetWindowLongPtrW, IsWindowVisible, CHILDID_SELF, GA_ROOT, GWL_STYLE,
    OBJID_CLIENT, WM_CHAR, WM_NCDESTROY, WS_TABSTOP,
};

const ACC_PROP_SERVICES: GUID = GUID::from_u128(0xb5f8350b_0548_48b1_a6ee_88bd00b4a5e7);
const NAME_PROPERTIES: [GUID; 2] = [Name_Property_GUID, PROPID_ACC_NAME];

struct AnnotationService {
    properties: Option<IAccPropServices>,
    initialized: bool,
}

impl Drop for AnnotationService {
    fn drop(&mut self) {
        self.properties.take();
        if self.initialized {
            unsafe { CoUninitialize() };
        }
    }
}

thread_local! {
    static ANNOTATIONS: AnnotationService = unsafe {
        AnnotationService {
            initialized: CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok(),
            properties: CoCreateInstance(&ACC_PROP_SERVICES, None, CLSCTX_SERVER).ok(),
        }
    };
}

fn accessible_label(id: usize) -> Option<&'static str> {
    ROWS.get(id.checked_sub(BASE_ID)?).map(|row| row.label)
}

pub(super) fn focus_shortcuts_close(handle: HWND, parent: HWND) {
    unsafe {
        if SetWindowSubclass(handle, Some(shortcuts_key), APPLY_ID, parent.0 as usize).as_bool() {
            let _ = SetFocus(Some(handle));
        }
    }
}

unsafe extern "system" fn shortcuts_key(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    parent: usize,
) -> LRESULT {
    match msg {
        WM_KEYDOWN
            if [VK_ESCAPE, VK_F1]
                .iter()
                .any(|key| key.0 as usize == wparam.0) =>
        unsafe {
            let owner = HWND(parent as *mut _);
            let _ = SetFocus(Some(owner));
            let _ = PostMessageW(Some(owner), msg, wparam, lparam);
            LRESULT(0)
        },
        WM_CHAR if wparam.0 == VK_ESCAPE.0 as usize => LRESULT(0),
        WM_NCDESTROY => unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(shortcuts_key), id);
            DefSubclassProc(hwnd, msg, wparam, lparam)
        },
        _ => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
    }
}

pub(super) fn attach_controls() {
    let controls = CONTROLS.with_borrow(Clone::clone);
    ANNOTATIONS.with(|annotations| {
        let Some(properties) = &annotations.properties else {
            return;
        };
        for &(id, handle) in &controls {
            if let Some(label) = accessible_label(id) {
                let label = w32::wide(label);
                // Expose the painted label through both native accessibility APIs.
                for property in NAME_PROPERTIES {
                    let _ = unsafe {
                        properties.SetHwndPropStr(
                            handle,
                            OBJID_CLIENT.0 as u32,
                            CHILDID_SELF,
                            property,
                            PCWSTR(label.as_ptr()),
                        )
                    };
                }
            }
        }
    });

    unsafe {
        for &(id, handle) in &controls {
            let _ = SetWindowSubclass(handle, Some(settings_key), id, 0);
        }
        if let Some((_, first)) = controls.into_iter().find(|(_, handle)| eligible(*handle)) {
            let _ = SetFocus(Some(first));
        }
    }
}

unsafe extern "system" fn settings_key(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    _holder: usize,
) -> LRESULT {
    match msg {
        WM_KEYDOWN if matches!(VIRTUAL_KEY(wparam.0 as u16), VK_ESCAPE | VK_F1) => unsafe {
            let root = GetAncestor(hwnd, GA_ROOT);
            let _ = SetFocus(Some(root));
            let _ = PostMessageW(Some(root), WM_KEYDOWN, wparam, lparam);
            LRESULT(0)
        },
        WM_KEYDOWN if wparam.0 == VK_TAB.0 as usize => {
            tab_to_next(hwnd);
            LRESULT(0)
        }
        WM_CHAR if matches!(wparam.0, 9 | 27) => LRESULT(0),
        WM_NCDESTROY => unsafe {
            if accessible_label(id).is_some() {
                let _ = ANNOTATIONS.try_with(|annotations| {
                    if let Some(properties) = &annotations.properties {
                        let _ = properties.ClearHwndProps(
                            hwnd,
                            OBJID_CLIENT.0 as u32,
                            CHILDID_SELF,
                            &NAME_PROPERTIES,
                        );
                    }
                });
            }
            let _ = RemoveWindowSubclass(hwnd, Some(settings_key), id);
            DefSubclassProc(hwnd, msg, wparam, lparam)
        },
        _ => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
    }
}

/// Moves the focus to the next visible, enabled tab stop of the panel, wrapping around and
/// skipping the rest, in either direction; a disabled control (for example "Only when opening
/// files on launch") is never a stop.
fn tab_to_next(hwnd: HWND) {
    let stops = CONTROLS.with_borrow(Clone::clone);
    let Some(at) = stops.iter().position(|(_, handle)| *handle == hwnd) else {
        return;
    };
    let reverse = unsafe { GetKeyState(VK_SHIFT.0 as i32) < 0 };
    if let Some(next) = next_stop(stops.len(), at, reverse, |index| eligible(stops[index].1)) {
        unsafe {
            let _ = SetFocus(Some(stops[next].1));
        }
    }
}

fn eligible(handle: HWND) -> bool {
    unsafe {
        IsWindowVisible(handle).as_bool()
            && IsWindowEnabled(handle).as_bool()
            && GetWindowLongPtrW(handle, GWL_STYLE) as u32 & WS_TABSTOP.0 != 0
    }
}

/// The next position after `from` in a ring of `count` entries whose entries may be
/// ineligible, wrapping around in either direction. `None` when there is no other entry.
fn next_stop(
    count: usize,
    from: usize,
    reverse: bool,
    is_stop: impl Fn(usize) -> bool,
) -> Option<usize> {
    let delta: i32 = if reverse { -1 } else { 1 };
    (1..count)
        .map(|step| (from as i32 + step as i32 * delta).rem_euclid(count as i32) as usize)
        .find(|&probe| is_stop(probe))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_walks_both_directions_and_skips_ineligible_stops() {
        let stops = [true, false, false, true];
        let eligible = |index| stops[index];
        assert_eq!(next_stop(4, 0, false, eligible), Some(3));
        assert_eq!(next_stop(4, 3, true, eligible), Some(0));
        assert_eq!(next_stop(4, 0, true, eligible), Some(3));
        assert_eq!(next_stop(4, 3, false, eligible), Some(0));
        assert_eq!(next_stop(4, 1, false, eligible), Some(3));
        assert_eq!(next_stop(1, 0, false, |_| false), None);
    }

    #[test]
    fn settings_rows_expose_their_painted_labels_to_automation() {
        assert_eq!(accessible_label(BASE_ID), Some("Show toolbar"));
        assert_eq!(accessible_label(BASE_ID + 1), Some("Always open maximized"));
        assert_eq!(
            accessible_label(BASE_ID + ROWS.len() - 1),
            Some("Pan speed")
        );
        // Buttons already speak their own window text.
        assert_eq!(accessible_label(PICK_BASE), None);
        assert_eq!(accessible_label(APPLY_ID), None);
        assert!(ROWS.iter().all(|row| !row.label.is_empty()));
    }
}
