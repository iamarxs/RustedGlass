//! The settings panel and the rename / copy / move prompt.
//!
//! The original is a XAML settings window with hundreds of options. This is the same idea at
//! Win32 size: the settings the viewer reads are rows of check boxes and text fields, and a check
//! box *is* its config key, so a click applies at once and `Save & Close` writes the changed keys
//! with `Config::save_user`. The fields are real child controls, so typing, focus and check marks
//! are the system's; the panel background, the labels of number fields and the button are painted.

use std::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use serde_json::Value;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, GetSysColorBrush, COLOR_WINDOW, HBRUSH, HDC, HGDIOBJ,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Controls::{SetWindowTheme, BST_CHECKED};
use windows::Win32::UI::Input::KeyboardAndMouse::{SetFocus, VK_RETURN};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    DefSubclassProc, FileOpenDialog, FileSaveDialog, IFileOpenDialog, IFileSaveDialog, IShellItem,
    SetWindowSubclass, FILEOPENDIALOGOPTIONS, FOS_ALLOWMULTISELECT, FOS_PICKFOLDERS,
    SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, PostMessageW, SendMessageW, SetWindowTextW, BM_GETCHECK,
    BM_SETCHECK, BS_AUTOCHECKBOX, BS_PUSHBUTTON, ES_AUTOHSCROLL, HMENU, WINDOW_STYLE, WM_COMMAND,
    WM_KEYDOWN, WM_SETFONT, WS_BORDER, WS_CHILD, WS_TABSTOP, WS_VISIBLE,
};

use crate::config::Config;
use crate::fileops::Pending;
use crate::w32;

/// What the panel is asking for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dialog {
    None,
    Settings,
    /// The list of keys the viewer answers to, from `input::SHORTCUT_GROUPS`.
    Shortcuts,
    /// A name or a folder the user has to type before a file operation can run.
    Text(String),
}

pub const MARGIN: i32 = 16;
pub const ROW_HEIGHT: i32 = 26;
pub const LABEL_WIDTH: i32 = 340;
pub const FIELD_WIDTH: i32 = 90;
const PICK_WIDTH: i32 = 90;
/// Path rows get a short label so the path itself has room to be read.
const PATH_LABEL_WIDTH: i32 = 200;
const PATH_FIELD_WIDTH: i32 = 230;
const HEADER: i32 = 32;
const BUTTON_WIDTH: i32 = 130;
const BUTTON_HEIGHT: i32 = 26;
pub const PANEL_COLOR: COLORREF = COLORREF(0x001A_1A1A);
pub const TEXT_COLOR: COLORREF = COLORREF(0x00DD_DDDD);
const TEXT_HEADING: COLORREF = COLORREF(0x008C_CFFF);
/// The id of the panel's one button ("Save & Close", "Apply", "Close"), whatever panel is open.
pub const APPLY_ID: usize = 800;
/// The "Pick…" button of a path row is this id plus the row's index.
pub const PICK_BASE: usize = 700;
/// Control ids start here, so a `WM_COMMAND` can be traced back to the row it came from.
pub const BASE_ID: usize = 100;
/// The single field of the rename / copy / move panel.
pub const PROMPT_ID: usize = 900;
/// Where the description of a shortcut starts. The longest key listed is "Ctrl + Wheel".
const KEY_COLUMN: i32 = 120;
/// A check box's control is only its box: the label is painted by the parent in the panel's own
/// colours, which a button cannot do for its own text.
const CHECK_WIDTH: i32 = 20;
const CHECK_LABEL_X: i32 = MARGIN + CHECK_WIDTH + 8;
const ROW_LABEL_TOP: i32 = 4;
/// A column of the shortcut list.
const COLUMN_WIDTH: i32 = 300;

// The child controls of the open panel, as `(control id, window)`. Empty when nothing is open;
// they are children of the viewer window, so they live and die with it.
thread_local! {
    static CONTROLS: RefCell<Vec<(usize, HWND)>> = const { RefCell::new(Vec::new()) };
    // The panel's colour as a brush. Windows asks for it every time a control repaints, so it is
    // made once and kept.
    static PANEL_BRUSH: RefCell<Option<HBRUSH>> = const { RefCell::new(None) };
    // How far the shortcut list is scrolled, and how far it can be scrolled. Painting works out
    // the maximum; the wheel in `view::wndproc` changes the offset.
    static SCROLL: Cell<i32> = const { Cell::new(0) };
    static SCROLL_MAX: Cell<i32> = const { Cell::new(0) };
}

/// Whether a `WM_COMMAND` id is one of the "Pick…" buttons. The tool bar's ids are higher than
/// these, so without this test a bar button would open the folder picker instead of its action.
pub fn is_pick(id: usize) -> bool {
    id >= PICK_BASE && id < PICK_BASE + ROWS.len()
}

/// Scroll the shortcut list by a wheel step. The offset is clamped by what painting measured, so
/// a list that fits the window never moves.
pub fn scroll_by(delta: i32) {
    let limit = SCROLL_MAX.with(Cell::get);
    SCROLL.with(|cell| cell.set((cell.get() - delta).clamp(0, limit)));
}

/// Rows the settings panel shows. The position in
/// the returned list is the control id offset and the row's place in the panel, so drawing,
/// creating and reading the controls must all use it.
fn rows(dialog: &Dialog) -> Vec<(usize, &'static Row)> {
    match dialog {
        Dialog::Settings => ROWS
            .iter()
            .enumerate()
            .map(|(index, row)| (index, row))
            .collect(),
        _ => Vec::new(),
    }
}

/// The settings panel is wide enough for a folder path and its picker button, and is anchored to
/// the left so a resize never has to move a control. The shortcut list is not a panel of controls:
/// it is laid out over the whole window (`draw_shortcuts`).
fn settings_width() -> i32 {
    MARGIN * 2 + PATH_LABEL_WIDTH + PATH_FIELD_WIDTH + PICK_WIDTH + 12
}

fn row_y(index: usize) -> i32 {
    HEADER + (index as i32) * ROW_HEIGHT
}

/// The "Pick…" button of a path row, next to its field. `create` adds the same offset to the
/// top as it does for the field, so the two line up.
fn pick_rect(y: i32) -> (i32, i32, i32, i32) {
    (
        settings_width() - MARGIN - PICK_WIDTH,
        y,
        PICK_WIDTH,
        ROW_HEIGHT - 4,
    )
}

/// How many rows a panel has. The panel's button is placed below the last one. The shortcut list
/// has no rows of controls, so it is not counted here (`apply_rect`).
fn row_count(dialog: &Dialog) -> usize {
    match dialog {
        Dialog::Settings => rows(dialog).len(),
        Dialog::Text(_) => 1,
        _ => 0,
    }
}

/// Where the panel's button sits: below its last row, at the panel's right edge. Everything under
/// it is empty panel, which is why it is not put at the bottom of the window - except for the
/// shortcut list, which fills the window and gets its Close button at the bottom.
fn apply_rect(dialog: &Dialog, parent: HWND) -> (i32, i32, i32, i32) {
    if *dialog == Dialog::Shortcuts {
        let (x, y, width, height) = shortcut_button_rect(unsafe { w32::client_size(parent) });
        return (x, y - 2, width, height);
    }
    (
        settings_width() - MARGIN - BUTTON_WIDTH,
        row_y(row_count(dialog)) + 6,
        BUTTON_WIDTH,
        BUTTON_HEIGHT,
    )
}

fn shortcut_button_rect((width, height): (u32, u32)) -> (i32, i32, i32, i32) {
    let button_width = (width as i32 - 2 * MARGIN).clamp(1, BUTTON_WIDTH);
    (
        (width as i32 - MARGIN - button_width).max(0),
        (height as i32 - BUTTON_HEIGHT - MARGIN).max(0),
        button_width,
        BUTTON_HEIGHT,
    )
}

fn title(dialog: &Dialog) -> &'static str {
    match dialog {
        Dialog::Settings => "Settings",
        Dialog::Shortcuts => "Keyboard shortcuts",
        Dialog::Text(_) => "Type a name or a folder",
        Dialog::None => "",
    }
}

fn button_label(dialog: &Dialog) -> &'static str {
    match dialog {
        Dialog::Text(_) => "Apply",
        Dialog::Shortcuts => "Close",
        _ => "Save && Close",
    }
}

/// The text a settings field starts with.
fn value_text(config: &Config, row: &Row) -> String {
    match row.kind {
        Kind::CheckBox => String::new(),
        Kind::TextField if row.key == "SlideshowIntervalTo" => config
            .get_f64(row.key, config.get_f64("SlideshowInterval", 5.0))
            .to_string(),
        Kind::TextField => config
            .get_f64(row.key, row.default.parse().unwrap_or_default())
            .to_string(),
        Kind::PathField => config.get_str(row.key, row.default),
    }
}

/// Copies the panel into the configuration: one key after a check box click, all of them when
/// the panel is saved. Text fields that do not parse are left as they are.
pub fn apply_to_config(dialog: &Dialog, config: &mut Config, only: Option<usize>) {
    if *dialog != Dialog::Settings {
        return;
    }
    for (index, row) in rows(dialog) {
        let id = BASE_ID + index;
        if only.is_some_and(|wanted| wanted != id)
            || matches!(row.key, "SlideshowInterval" | "SlideshowIntervalTo")
        {
            continue;
        }
        match row.kind {
            Kind::CheckBox => {
                let state = CONTROLS.with(|controls| {
                    controls
                        .borrow()
                        .iter()
                        .find(|(control, _)| *control == id)
                        .map(|(_, handle)| checked(*handle))
                });
                if let Some(is_checked) = state {
                    config.set(row.key, Value::Bool(is_checked));
                }
            }
            Kind::PathField => {
                if let Some(text) = field_text(id) {
                    config.set(row.key, Value::String(text.trim().to_string()));
                }
            }
            Kind::TextField => {
                if let Some(number) =
                    field_text(id).and_then(|text| text.trim().parse::<f64>().ok())
                {
                    if let Some(value) = serde_json::Number::from_f64(number) {
                        config.set(row.key, Value::Number(value));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;

mod picker;
pub use picker::{pick_files, pick_folder, pick_one_file, pick_save_file};

mod layout;
pub use layout::{open, resize_shortcuts};

mod controls;
use controls::{checked, create, field_text, prefill, prompt_key, set_checked, untheme};
pub use controls::{click_checkbox_label, close, prompt_text, set_field_text, slideshow_intervals};

mod draw;
pub use draw::{color_control, draw};

mod rows;
use rows::{Kind, Row, ROWS};
