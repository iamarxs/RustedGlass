//! Exposes painted status and shortcut text to Windows accessibility clients.

use std::cell::RefCell;

use windows::core::{GUID, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Variant::{VARIANT, VT_I4};
use windows::Win32::UI::Accessibility::{
    HelpText_Property_GUID, IAccPropServices, LiveSetting_Property_GUID, NotifyWinEvent, Polite,
    PROPID_ACC_DESCRIPTION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CHILDID_SELF, EVENT_OBJECT_LIVEREGIONCHANGED, OBJID_CLIENT,
};

use crate::state;

const ACC_PROP_SERVICES: GUID = GUID::from_u128(0xb5f8350b_0548_48b1_a6ee_88bd00b4a5e7);
const HELP_PROPERTIES: [GUID; 2] = [HelpText_Property_GUID, PROPID_ACC_DESCRIPTION];

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
    /// The annotation service, made once. COM is initialized for the thread first.
    static ANNOTATIONS: AnnotationService = unsafe {
        AnnotationService {
            initialized: CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok(),
            properties: CoCreateInstance(&ACC_PROP_SERVICES, None, CLSCTX_INPROC_SERVER).ok(),
        }
    };
    /// The last text announced. Refreshes run on every state change, so remembering the
    /// previous text is what keeps a repaint or an unrelated update from firing the event.
    static LAST: RefCell<String> = const { RefCell::new(String::new()) };
}

pub(super) fn start(hwnd: HWND) {
    ANNOTATIONS.with(|service| {
        let Some(properties) = &service.properties else {
            return;
        };
        let mut value = VARIANT::default();
        unsafe {
            let integer = &mut *value.Anonymous.Anonymous;
            integer.vt = VT_I4;
            integer.Anonymous.lVal = Polite.0;
            let _ = properties.SetHwndProp(
                hwnd,
                OBJID_CLIENT.0 as u32,
                CHILDID_SELF,
                LiveSetting_Property_GUID,
                &value,
            );
        }
    });
}

pub(super) fn clear(hwnd: HWND) {
    LAST.with_borrow_mut(String::clear);
    set_help(hwnd, None);
    ANNOTATIONS.with(|service| {
        if let Some(properties) = &service.properties {
            let _ = unsafe {
                properties.ClearHwndProps(
                    hwnd,
                    OBJID_CLIENT.0 as u32,
                    CHILDID_SELF,
                    &[LiveSetting_Property_GUID],
                )
            };
        }
    });
}

/// Announces status changes without changing the accessible name of the window.
pub fn announce_status(hwnd: HWND) {
    let Some(text) = state::with_state(|state| {
        state
            .notice
            .clone()
            .or_else(|| {
                (!state.playlist_visible)
                    .then(|| state.message.clone())
                    .flatten()
            })
            .unwrap_or_default()
    }) else {
        return;
    };
    let changed = LAST.with_borrow_mut(|last| update_text(last, &text));
    if !changed {
        return;
    }
    set_help(hwnd, (!text.is_empty()).then_some(text.as_str()));
    if !text.is_empty() {
        unsafe {
            NotifyWinEvent(
                EVENT_OBJECT_LIVEREGIONCHANGED,
                hwnd,
                OBJID_CLIENT.0,
                CHILDID_SELF as i32,
            );
        }
    }
}

/// The shortcut list is painted directly, so automation reaches its headings, keys and
/// descriptions through the Close button's accessible help text.
pub fn annotate_close(handle: HWND) {
    set_help(handle, Some(&reference()));
}

fn set_help(hwnd: HWND, text: Option<&str>) {
    ANNOTATIONS.with(|service| {
        let Some(properties) = &service.properties else {
            return;
        };
        if let Some(text) = text {
            let wide = crate::w32::wide(text);
            for property in HELP_PROPERTIES {
                let _ = unsafe {
                    properties.SetHwndPropStr(
                        hwnd,
                        OBJID_CLIENT.0 as u32,
                        CHILDID_SELF,
                        property,
                        PCWSTR(wide.as_ptr()),
                    )
                };
            }
        } else {
            let _ = unsafe {
                properties.ClearHwndProps(
                    hwnd,
                    OBJID_CLIENT.0 as u32,
                    CHILDID_SELF,
                    &HELP_PROPERTIES,
                )
            };
        }
    });
}

fn update_text(last: &mut String, text: &str) -> bool {
    if *last == text {
        return false;
    }
    last.clear();
    last.push_str(text);
    true
}

/// The whole F1 list as one text, from the same `SHORTCUT_GROUPS` that painting uses, so what
/// automation reports is exactly what is painted.
fn reference() -> String {
    let mut text = String::new();
    for (heading, rows) in crate::input::SHORTCUT_GROUPS {
        text.push_str(heading);
        text.push('\n');
        for (key, what) in *rows {
            text.push_str(key);
            text.push_str(" — ");
            text.push_str(what);
            text.push('\n');
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_accessible_reference_lists_every_grouped_shortcut() {
        let reference = reference();
        for (heading, rows) in crate::input::SHORTCUT_GROUPS {
            assert!(reference.contains(heading));
            for (key, what) in *rows {
                assert!(reference.contains(&format!("{key} — {what}")));
            }
        }
    }

    #[test]
    fn a_text_change_announces_once_and_an_unchanged_text_not_at_all() {
        let mut last = String::new();
        let mut step = |text: &str| update_text(&mut last, text);
        assert!(step("Saved."));
        assert!(!step("Saved."), "the same text must not announce again");
        assert!(step("Copied."));
        assert!(step(""), "clearing announces the end of a message");
        assert!(!step(""));
    }
}
