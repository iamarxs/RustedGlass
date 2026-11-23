use std::cell::Cell;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use windows::core::{w, HRESULT, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Controls::{
    TaskDialogIndirect, TASKDIALOGCONFIG, TASKDIALOG_BUTTON, TASKDIALOG_NOTIFICATIONS,
    TDCBF_CLOSE_BUTTON, TDE_CONTENT, TDF_ALLOW_DIALOG_CANCELLATION, TDF_CALLBACK_TIMER,
    TDF_ENABLE_HYPERLINKS, TDF_POSITION_RELATIVE_TO_WINDOW, TDM_ENABLE_BUTTON,
    TDM_SET_ELEMENT_TEXT, TDN_CREATED, TDN_HYPERLINK_CLICKED, TDN_TIMER,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, IDCLOSE, SW_SHOWNORMAL};

use crate::update_check::UpdateStatus;

const REPOSITORY: &str = "https://github.com/iamarxs/RustedGlass";
const LATEST_RELEASE: PCWSTR = w!("https://github.com/iamarxs/RustedGlass/releases/latest");
const CHECK_FOR_UPDATES: i32 = 100;
const OPEN_RELEASE: i32 = 101;

pub fn show(hwnd: HWND) {
    let content = crate::w32::wide(&format!(
        "Version {}\nCompiler: {}\nTarget: {}\nBuild profile: {}\n\n<a href=\"{REPOSITORY}\">{REPOSITORY}</a>",
        crate::VERSION, env!("RG_RUSTC"), env!("RG_TARGET"), env!("RG_PROFILE")
    ));
    let button = TASKDIALOG_BUTTON {
        nButtonID: CHECK_FOR_UPDATES,
        pszButtonText: w!("Check for updates"),
    };
    let dialog = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: hwnd,
        dwFlags: TDF_ENABLE_HYPERLINKS | TDF_POSITION_RELATIVE_TO_WINDOW,
        dwCommonButtons: TDCBF_CLOSE_BUTTON,
        pszWindowTitle: w!("About RustedGlass"),
        pszMainInstruction: w!("RustedGlass"),
        pszContent: PCWSTR(content.as_ptr()),
        cButtons: 1,
        pButtons: &button,
        nDefaultButton: IDCLOSE.0,
        pfCallback: Some(on_notification),
        ..Default::default()
    };
    let mut chosen = 0;
    if unsafe { TaskDialogIndirect(&dialog, Some(&mut chosen), None, None) }.is_ok()
        && chosen == CHECK_FOR_UPDATES
    {
        show_update_check(hwnd);
    }
}

unsafe extern "system" fn on_notification(
    hwnd: HWND,
    message: TASKDIALOG_NOTIFICATIONS,
    _: WPARAM,
    _: LPARAM,
    _: isize,
) -> HRESULT {
    if message == TDN_HYPERLINK_CLICKED {
        let url = crate::w32::wide(REPOSITORY);
        open_link(hwnd, PCWSTR(url.as_ptr()));
    }
    HRESULT(0)
}

struct UpdateDialog {
    result: Receiver<Result<UpdateStatus, String>>,
    finished: Cell<bool>,
}

fn show_update_check(hwnd: HWND) {
    let (sender, result) = mpsc::channel();
    // Closing the dialog drops the receiver. The bounded request can then finish on its own.
    let _ = std::thread::Builder::new()
        .name("update-check".to_owned())
        .spawn(move || {
            let _ = sender.send(crate::update_check::check());
        });
    let state = UpdateDialog {
        result,
        finished: Cell::new(false),
    };
    let button = TASKDIALOG_BUTTON {
        nButtonID: OPEN_RELEASE,
        pszButtonText: w!("Open release page"),
    };
    let dialog = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: hwnd,
        dwFlags: TDF_ALLOW_DIALOG_CANCELLATION
            | TDF_CALLBACK_TIMER
            | TDF_POSITION_RELATIVE_TO_WINDOW,
        dwCommonButtons: TDCBF_CLOSE_BUTTON,
        pszWindowTitle: w!("RustedGlass Updates"),
        pszMainInstruction: w!("Check for updates"),
        pszContent: w!("Checking GitHub for a new version…"),
        cButtons: 1,
        pButtons: &button,
        nDefaultButton: IDCLOSE.0,
        pfCallback: Some(on_update_notification),
        lpCallbackData: (&state as *const UpdateDialog) as isize,
        ..Default::default()
    };
    let mut chosen = 0;
    if unsafe { TaskDialogIndirect(&dialog, Some(&mut chosen), None, None) }.is_ok()
        && chosen == OPEN_RELEASE
    {
        open_link(hwnd, LATEST_RELEASE);
    }
}

unsafe extern "system" fn on_update_notification(
    hwnd: HWND,
    message: TASKDIALOG_NOTIFICATIONS,
    _: WPARAM,
    _: LPARAM,
    context: isize,
) -> HRESULT {
    if message == TDN_CREATED {
        enable_release_button(hwnd, false);
    }
    if message == TDN_TIMER {
        // TaskDialogIndirect invokes the callback only while its stack-owned state is alive.
        let state = unsafe { &*(context as *const UpdateDialog) };
        if !state.finished.get() {
            match state.result.try_recv() {
                Ok(result) => show_update_result(hwnd, state, result),
                Err(TryRecvError::Disconnected) => {
                    show_update_result(hwnd, state, Err("The check could not finish.".to_owned()));
                }
                Err(TryRecvError::Empty) => {}
            }
        }
    }
    HRESULT(0)
}

fn show_update_result(hwnd: HWND, state: &UpdateDialog, result: Result<UpdateStatus, String>) {
    state.finished.set(true);
    let (message, can_open) = match result {
        Ok(UpdateStatus::Available(version)) => (
            format!(
                "RustedGlass {version} is available. You have version {}.",
                crate::VERSION
            ),
            true,
        ),
        Ok(UpdateStatus::Current) => (
            format!("RustedGlass {} is up to date.", crate::VERSION),
            false,
        ),
        Ok(UpdateStatus::Ahead(version)) => (
            format!(
                "You’re running RustedGlass {}, which is newer than the latest published release ({version}).",
                crate::VERSION
            ),
            false,
        ),
        Ok(UpdateStatus::NoStableRelease) => (
            "GitHub did not return a stable release.".to_owned(),
            false,
        ),
        Err(error) => (format!("Could not check for updates.\n{error}"), false),
    };
    let message = crate::w32::wide(&message);
    let _ = unsafe {
        SendMessageW(
            hwnd,
            TDM_SET_ELEMENT_TEXT.0 as u32,
            Some(WPARAM(TDE_CONTENT.0 as usize)),
            Some(LPARAM(message.as_ptr() as isize)),
        )
    };
    enable_release_button(hwnd, can_open);
}

fn enable_release_button(hwnd: HWND, enable: bool) {
    let _ = unsafe {
        SendMessageW(
            hwnd,
            TDM_ENABLE_BUTTON.0 as u32,
            Some(WPARAM(OPEN_RELEASE as usize)),
            Some(LPARAM(isize::from(enable))),
        )
    };
}

fn open_link(hwnd: HWND, url: PCWSTR) {
    let _ = unsafe { ShellExecuteW(Some(hwnd), w!("open"), url, None, None, SW_SHOWNORMAL) };
}
