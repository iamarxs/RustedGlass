use super::*;
use windows::Win32::UI::Shell::{FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FOS_STRICTFILETYPES};

/// COM is needed by the file and folder pickers; calling it again is harmless.
fn init_com() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
}

/// A folder picked with the system dialog, `None` if the user cancelled.
pub fn pick_folder(parent: HWND, heading: &str) -> Option<PathBuf> {
    pick(parent, FOS_PICKFOLDERS, heading).first().cloned()
}

/// Files picked with the system dialog; empty if the user cancelled.
pub fn pick_files(parent: HWND, heading: &str) -> Vec<PathBuf> {
    pick(parent, FOS_ALLOWMULTISELECT, heading)
}

/// One file of one kind, for the playlist dialogs. `None` if the user cancelled.
pub fn pick_one_file(parent: HWND, heading: &str, label: &str, pattern: &str) -> Option<PathBuf> {
    init_com();
    let Ok(dialog) = (unsafe {
        CoCreateInstance::<_, IFileOpenDialog>(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
    }) else {
        return None;
    };
    let wide_name = w32::wide(label);
    let wide_pattern = w32::wide(pattern);
    let filter = [COMDLG_FILTERSPEC {
        pszName: PCWSTR(wide_name.as_ptr()),
        pszSpec: PCWSTR(wide_pattern.as_ptr()),
    }];
    unsafe {
        dialog
            .SetOptions(dialog.GetOptions().ok()? | FOS_FORCEFILESYSTEM)
            .ok()?;
        let _ = dialog.SetTitle(PCWSTR(w32::wide(heading).as_ptr()));
        let _ = dialog.SetFileTypes(&filter);
        if dialog.Show(Some(parent)).is_err() {
            return None;
        }
        path_of(&dialog.GetResult().ok()?)
    }
}

/// Ask for a name to save to. `suggested` is filled into the name field, and `pattern` decides
/// which kind of file the dialog offers.
pub fn pick_save_file(
    parent: HWND,
    heading: &str,
    label: &str,
    pattern: &str,
    suggested: &str,
) -> Option<PathBuf> {
    save_dialog(
        parent,
        heading,
        label,
        pattern,
        suggested,
        crate::rgpl::EXTENSION,
        FILEOPENDIALOGOPTIONS(0),
    )
}

pub fn pick_image_destination(parent: HWND, image: &Path) -> Option<PathBuf> {
    let extension = image.extension()?.to_str()?;
    save_dialog(
        parent,
        "Save as",
        "Original image format",
        &format!("*.{extension}"),
        &w32::file_name(image),
        extension,
        FOS_STRICTFILETYPES,
    )
}

fn save_dialog(
    parent: HWND,
    heading: &str,
    label: &str,
    pattern: &str,
    suggested: &str,
    extension: &str,
    flags: FILEOPENDIALOGOPTIONS,
) -> Option<PathBuf> {
    init_com();
    let Ok(dialog) = (unsafe {
        CoCreateInstance::<_, IFileSaveDialog>(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)
    }) else {
        return None;
    };
    let wide_name = w32::wide(label);
    let wide_pattern = w32::wide(pattern);
    let filter = [COMDLG_FILTERSPEC {
        pszName: PCWSTR(wide_name.as_ptr()),
        pszSpec: PCWSTR(wide_pattern.as_ptr()),
    }];
    unsafe {
        let _ = dialog.SetTitle(PCWSTR(w32::wide(heading).as_ptr()));
        let _ = dialog.SetFileTypes(&filter);
        let options = dialog.GetOptions().ok()? | FOS_FORCEFILESYSTEM | FOS_OVERWRITEPROMPT;
        dialog.SetOptions(options | flags).ok()?;
        let _ = dialog.SetDefaultExtension(PCWSTR(w32::wide(extension).as_ptr()));
        let _ = dialog.SetFileName(PCWSTR(w32::wide(suggested).as_ptr()));
        if dialog.Show(Some(parent)).is_err() {
            return None;
        }
        path_of(&dialog.GetResult().ok()?)
    }
}

fn pick(parent: HWND, options: FILEOPENDIALOGOPTIONS, heading: &str) -> Vec<PathBuf> {
    init_com();
    let Ok(dialog) = (unsafe {
        CoCreateInstance::<_, IFileOpenDialog>(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
    }) else {
        return Vec::new();
    };
    let title = w32::wide(heading);
    unsafe {
        let Ok(defaults) = dialog.GetOptions() else {
            return Vec::new();
        };
        if dialog
            .SetOptions(defaults | FOS_FORCEFILESYSTEM | options)
            .is_err()
        {
            return Vec::new();
        }
        let _ = dialog.SetTitle(PCWSTR(title.as_ptr()));
        if dialog.Show(Some(parent)).is_err() {
            return Vec::new();
        }
        let Ok(items) = dialog.GetResults() else {
            return Vec::new();
        };
        let count = items.GetCount().unwrap_or(0);
        (0..count)
            .filter_map(|index| items.GetItemAt(index).ok())
            .filter_map(|item| path_of(&item))
            .collect()
    }
}

/// The path a shell item points at; the task allocator owns the string it hands back.
fn path_of(item: &IShellItem) -> Option<PathBuf> {
    unsafe {
        let raw = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = OsString::from_wide(raw.as_wide());
        CoTaskMemFree(Some(raw.as_ptr() as *const core::ffi::c_void));
        Some(PathBuf::from(path))
    }
}
