use std::cell::{Cell, RefCell};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use windows::core::{w, HRESULT, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Controls::{
    TaskDialogIndirect, TASKDIALOGCONFIG, TASKDIALOG_BUTTON, TASKDIALOG_NOTIFICATIONS,
    TDCBF_CLOSE_BUTTON, TDE_CONTENT, TDF_ALLOW_DIALOG_CANCELLATION, TDF_CALLBACK_TIMER,
    TDF_POSITION_RELATIVE_TO_WINDOW, TDM_ENABLE_BUTTON, TDM_SET_ELEMENT_TEXT, TDN_CREATED,
    TDN_TIMER,
};
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, PostMessageW, SendMessageW, IDCLOSE, MB_ICONERROR, MB_OK, WM_CLOSE,
};

use crate::update_check::{self, UpdateStatus};
use crate::{update_install, update_package};

const LATEST_RELEASE: PCWSTR = w!("https://github.com/iamarxs/RustedGlass/releases/latest");
const OPEN_RELEASE: i32 = 101;
const CONTINUE: i32 = 102;

enum Outcome {
    Checked(UpdateStatus),
    Downloaded(Vec<u8>),
    ManualDownload,
}

struct Dialog {
    result: Receiver<Result<Outcome, String>>,
    next: RefCell<Option<Outcome>>,
    finished: Cell<bool>,
    downloading: bool,
}

pub(super) fn show(hwnd: HWND) {
    let Some(Outcome::Checked(UpdateStatus::Available(version))) = run(hwnd, None) else {
        return;
    };
    let Some(Outcome::Downloaded(exe)) = run(hwnd, Some(version)) else {
        return;
    };
    let result = update_install::schedule(&exe).and_then(|()| {
        unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) }
            .map_err(|error| format!("Could not close RustedGlass for the update: {error}"))
    });
    if let Err(error) = result {
        let message = crate::w32::wide(&error);
        let _ = unsafe {
            MessageBoxW(
                Some(hwnd),
                PCWSTR(message.as_ptr()),
                w!("Could not install the update"),
                MB_OK | MB_ICONERROR,
            )
        };
    }
}

fn download(version: &str) -> Result<Outcome, String> {
    let Some(manifest) = update_check::fetch_asset(version, "update.json", 64 * 1024)? else {
        return Ok(Outcome::ManualDownload);
    };
    let signature = update_check::fetch_asset(version, "update.json.sig", 64)?
        .ok_or("The release is missing its update signature.")?;
    let package = update_package::verify_manifest(version, &manifest, &signature)?;
    let zip = update_check::fetch_asset(version, package.filename(), 64 * 1024 * 1024)?
        .ok_or("The release is missing its signed ZIP.")?;
    Ok(Outcome::Downloaded(package.extract(&zip)?))
}

fn run(hwnd: HWND, version: Option<String>) -> Option<Outcome> {
    let (sender, result) = mpsc::channel();
    let downloading = version.is_some();
    let _ = std::thread::Builder::new()
        .name("update-check".to_owned())
        .spawn(move || {
            let outcome = match version {
                Some(version) => download(&version),
                None => update_check::check().map(Outcome::Checked),
            };
            let _ = sender.send(outcome);
        });
    let state = Dialog {
        result,
        next: RefCell::new(None),
        finished: Cell::new(false),
        downloading,
    };
    let (instruction, pending, action) = if downloading {
        (
            w!("Download update"),
            w!("Downloading and verifying the update…"),
            w!("Install and restart"),
        )
    } else {
        (
            w!("Check for updates"),
            w!("Checking GitHub for a new version…"),
            w!("Download update"),
        )
    };
    let buttons = [
        TASKDIALOG_BUTTON {
            nButtonID: CONTINUE,
            pszButtonText: action,
        },
        TASKDIALOG_BUTTON {
            nButtonID: OPEN_RELEASE,
            pszButtonText: w!("Open release page"),
        },
    ];
    let dialog = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: hwnd,
        dwFlags: TDF_ALLOW_DIALOG_CANCELLATION
            | TDF_CALLBACK_TIMER
            | TDF_POSITION_RELATIVE_TO_WINDOW,
        dwCommonButtons: TDCBF_CLOSE_BUTTON,
        pszWindowTitle: w!("RustedGlass Updates"),
        pszMainInstruction: instruction,
        pszContent: pending,
        cButtons: buttons.len() as u32,
        pButtons: buttons.as_ptr(),
        nDefaultButton: IDCLOSE.0,
        pfCallback: Some(on_notification),
        lpCallbackData: (&state as *const Dialog) as isize,
        ..Default::default()
    };
    let mut chosen = 0;
    unsafe { TaskDialogIndirect(&dialog, Some(&mut chosen), None, None) }.ok()?;
    if chosen == OPEN_RELEASE {
        super::open_link(hwnd, LATEST_RELEASE);
    }
    if chosen == CONTINUE {
        return state.next.into_inner();
    }
    None
}

unsafe extern "system" fn on_notification(
    hwnd: HWND,
    message: TASKDIALOG_NOTIFICATIONS,
    _: WPARAM,
    _: LPARAM,
    context: isize,
) -> HRESULT {
    if message == TDN_CREATED {
        enable_continue(hwnd, false);
    } else if message == TDN_TIMER {
        // The dialog only calls back while its stack-owned state is alive.
        let state = unsafe { &*(context as *const Dialog) };
        if !state.finished.get() {
            match state.result.try_recv() {
                Ok(result) => show_result(hwnd, state, result),
                Err(TryRecvError::Disconnected) => {
                    show_result(
                        hwnd,
                        state,
                        Err("The update task could not finish.".to_owned()),
                    );
                }
                Err(TryRecvError::Empty) => {}
            }
        }
    }
    HRESULT(0)
}

fn show_result(hwnd: HWND, state: &Dialog, result: Result<Outcome, String>) {
    state.finished.set(true);
    let (message, next) = match result {
        Ok(Outcome::Checked(UpdateStatus::Available(version))) => (
            format!(
                "RustedGlass {version} is available. You have version {}.",
                crate::VERSION
            ),
            Some(Outcome::Checked(UpdateStatus::Available(version))),
        ),
        Ok(Outcome::Checked(UpdateStatus::Current)) => (
            format!("RustedGlass {} is up to date.", crate::VERSION),
            None,
        ),
        Ok(Outcome::Checked(UpdateStatus::Ahead(version))) => (
            format!(
                "You’re running RustedGlass {}, which is newer than the latest published release ({version}).",
                crate::VERSION
            ),
            None,
        ),
        Ok(Outcome::Checked(UpdateStatus::NoStableRelease)) => (
            "GitHub did not return a stable release.".to_owned(),
            None,
        ),
        Ok(outcome @ Outcome::Downloaded(_)) => (
            "The verified update is ready. RustedGlass will restart to install it.".to_owned(),
            Some(outcome),
        ),
        Ok(Outcome::ManualDownload) => (
            "This release is available as a manual download from GitHub.".to_owned(),
            None,
        ),
        Err(error) => {
            let heading = if state.downloading {
                "Could not prepare the update."
            } else {
                "Could not check for updates."
            };
            (format!("{heading}\n{error}"), None)
        }
    };
    let can_continue = next.is_some();
    state.next.replace(next);
    let message = crate::w32::wide(&message);
    let _ = unsafe {
        SendMessageW(
            hwnd,
            TDM_SET_ELEMENT_TEXT.0 as u32,
            Some(WPARAM(TDE_CONTENT.0 as usize)),
            Some(LPARAM(message.as_ptr() as isize)),
        )
    };
    enable_continue(hwnd, can_continue);
}

fn enable_continue(hwnd: HWND, enable: bool) {
    let _ = unsafe {
        SendMessageW(
            hwnd,
            TDM_ENABLE_BUTTON.0 as u32,
            Some(WPARAM(CONTINUE as usize)),
            Some(LPARAM(isize::from(enable))),
        )
    };
}
