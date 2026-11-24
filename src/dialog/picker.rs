use super::*;
use windows::core::w;
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

pub fn pick_playlist(parent: HWND) -> Option<PathBuf> {
    init_com();
    let Ok(dialog) = (unsafe {
        CoCreateInstance::<_, IFileOpenDialog>(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
    }) else {
        return None;
    };
    let filter = [COMDLG_FILTERSPEC {
        pszName: w!("RustedGlass playlists (*.rgpl; *.rgplz)"),
        pszSpec: w!("*.rgpl;*.rgplz"),
    }];
    unsafe {
        dialog
            .SetOptions(dialog.GetOptions().ok()? | FOS_FORCEFILESYSTEM)
            .ok()?;
        let _ = dialog.SetTitle(w!("Open playlist"));
        let _ = dialog.SetFileTypes(&filter);
        if dialog.Show(Some(parent)).is_err() {
            return None;
        }
        path_of(&dialog.GetResult().ok()?)
    }
}

pub fn pick_playlist_destination(parent: HWND) -> Option<PathBuf> {
    let filters = [
        COMDLG_FILTERSPEC {
            pszName: w!("Compressed playlist (*.rgplz)"),
            pszSpec: w!("*.rgplz"),
        },
        COMDLG_FILTERSPEC {
            pszName: w!("RustedGlass playlist (*.rgpl)"),
            pszSpec: w!("*.rgpl"),
        },
    ];
    save_dialog(
        parent,
        "Save playlist",
        &filters,
        "playlist",
        crate::rgpl::PACKED_EXTENSION,
        FOS_STRICTFILETYPES,
    )
}

pub fn pick_image_destination(parent: HWND, image: &Path) -> Option<PathBuf> {
    let extension = image.extension()?.to_str()?;
    let pattern = w32::wide(&format!("*.{extension}"));
    let filter = [COMDLG_FILTERSPEC {
        pszName: w!("Original image format"),
        pszSpec: PCWSTR(pattern.as_ptr()),
    }];
    save_dialog(
        parent,
        "Save as",
        &filter,
        &w32::file_name(image),
        extension,
        FOS_STRICTFILETYPES,
    )
}

fn save_dialog(
    parent: HWND,
    heading: &str,
    filters: &[COMDLG_FILTERSPEC],
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
    unsafe {
        let _ = dialog.SetTitle(PCWSTR(w32::wide(heading).as_ptr()));
        dialog.SetFileTypes(filters).ok()?;
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
