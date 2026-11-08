use windows::core::{w, HRESULT, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Controls::{
    TaskDialogIndirect, TASKDIALOGCONFIG, TASKDIALOG_NOTIFICATIONS, TDCBF_CLOSE_BUTTON,
    TDF_ENABLE_HYPERLINKS, TDF_POSITION_RELATIVE_TO_WINDOW, TDN_HYPERLINK_CLICKED,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

const REPOSITORY: &str = "https://github.com/iamarxs/RustedGlass";

pub fn show(hwnd: HWND) {
    let content = crate::w32::wide(&format!(
        "Version {}\nCompiler: {}\nTarget: {}\nBuild profile: {}\n\n<a href=\"{REPOSITORY}\">{REPOSITORY}</a>",
        crate::VERSION, env!("RG_RUSTC"), env!("RG_TARGET"), env!("RG_PROFILE")
    ));
    let dialog = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: hwnd,
        dwFlags: TDF_ENABLE_HYPERLINKS | TDF_POSITION_RELATIVE_TO_WINDOW,
        dwCommonButtons: TDCBF_CLOSE_BUTTON,
        pszWindowTitle: w!("About RustedGlass"),
        pszMainInstruction: w!("RustedGlass"),
        pszContent: PCWSTR(content.as_ptr()),
        pfCallback: Some(on_notification),
        ..Default::default()
    };
    let _ = unsafe { TaskDialogIndirect(&dialog, None, None, None) };
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
        let _ = unsafe {
            ShellExecuteW(
                Some(hwnd),
                w!("open"),
                PCWSTR(url.as_ptr()),
                None,
                None,
                SW_SHOWNORMAL,
            )
        };
    }
    HRESULT(0)
}
