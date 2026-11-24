use windows::core::{w, HRESULT, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Controls::{
    TaskDialogIndirect, TASKDIALOGCONFIG, TASKDIALOG_BUTTON, TASKDIALOG_NOTIFICATIONS,
    TDCBF_CLOSE_BUTTON, TDF_ENABLE_HYPERLINKS, TDF_POSITION_RELATIVE_TO_WINDOW,
    TDN_HYPERLINK_CLICKED,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{IDCLOSE, SW_SHOWNORMAL};

mod update;

const REPOSITORY: &str = "https://github.com/iamarxs/RustedGlass";
const CHECK_FOR_UPDATES: i32 = 100;

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
        update::show(hwnd);
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

fn open_link(hwnd: HWND, url: PCWSTR) {
    let _ = unsafe { ShellExecuteW(Some(hwnd), w!("open"), url, None, None, SW_SHOWNORMAL) };
}
