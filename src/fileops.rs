//! File operations the tool bar offers: delete, rename, copy to, move to, show in Explorer.
//!
//! The C# originals live in `Common/BHelper/File.cs` / `IO.cs`. A delete goes to the recycle bin,
//! like `BFile.MoveToRecycleBin`.

use crate::w32::file_name;
use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{CopyFileW, MoveFileExW, MOVE_FILE_FLAGS};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{
    FileOperation, IFileOperation, IShellItem, SHCreateItemFromParsingName,
    SHOpenFolderAndSelectItems, SHParseDisplayName, FOFX_EARLYFAILURE, FOFX_RECYCLEONDELETE,
    FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
};

fn shell_file<T>(
    path: &Path,
    operation: impl FnOnce(PCWSTR) -> windows::core::Result<T>,
) -> Result<T, String> {
    let absolute = std::path::absolute(path).map_err(|error| format!("{path:?}: {error}"))?;
    let wide: Vec<u16> = absolute
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let init = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let result = operation(PCWSTR(wide.as_ptr()));
    if init.is_ok() {
        unsafe { CoUninitialize() };
    }
    result.map_err(|error| format!("{path:?}: {error}"))
}

/// Sends an image file to the Windows Recycle Bin.
pub fn delete(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("{path:?} is not a file"));
    }
    let aborted = shell_file(path, |wide| unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(wide, None)?;
        let operation: IFileOperation =
            CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER)?;
        operation.SetOperationFlags(
            FOFX_RECYCLEONDELETE
                | FOFX_EARLYFAILURE
                | FOF_NOCONFIRMATION
                | FOF_NOERRORUI
                | FOF_SILENT,
        )?;
        operation.DeleteItem(&item, None)?;
        operation.PerformOperations()?;
        operation.GetAnyOperationsAborted()
    })?;
    if aborted.as_bool() || path.exists() {
        return Err(format!("{path:?} could not be moved to the Recycle Bin"));
    }
    Ok(())
}

/// Renames the file inside its own folder, and returns the new path. A name without an
/// extension keeps the file's current one, so a rename cannot stop it being a picture.
pub fn rename(path: &Path, new_name: &str) -> Result<PathBuf, String> {
    let trimmed = new_name.trim();
    if matches!(trimmed, "" | "." | "..")
        || trimmed.chars().any(|c| {
            c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
    {
        return Err(format!("{trimmed:?} is not a file name"));
    }

    let name = if trimmed.contains('.') {
        trimmed.to_string()
    } else {
        format!("{trimmed}{}", extension(path))
    };

    if name.ends_with(['.', ' ']) || reserved_name(&name) {
        return Err(format!("{name:?} is not a Windows file name"));
    }

    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let target = dir.join(&name);

    if target == path {
        return Ok(target);
    }

    transfer(path, &target, false)
        .map_err(|e| format!("cannot rename {} to {name}: {e}", path.display()))?;
    Ok(target)
}

fn reserved_name(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        matches!(
            stem.strip_prefix(prefix),
            Some("1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")
        )
    })
}

/// Copies the file into `dir`, choosing a numbered name if needed.
pub fn copy_to(path: &Path, dir: &Path) -> Result<PathBuf, String> {
    if dir.as_os_str().is_empty() || !dir.is_dir() {
        return Err("Enter an existing folder to copy the image to.".into());
    }
    transfer_to_folder(path, dir, true)
}

/// Moves the file into `dir`, choosing a numbered name if needed.
pub fn move_to(path: &Path, dir: &Path) -> Result<PathBuf, String> {
    let source_dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if let (Ok(source), Ok(destination)) = (source_dir.canonicalize(), dir.canonicalize()) {
        if source == destination {
            return Err(format!(
                "{} is already in {}",
                path.display(),
                dir.display()
            ));
        }
    }
    transfer_to_folder(path, dir, false)
}

fn transfer_to_folder(path: &Path, dir: &Path, copy: bool) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .ok_or_else(|| format!("{path:?} has no file name"))?;
    let mut target = dir.join(name);
    let mut number = 2;
    loop {
        match transfer(path, &target, copy) {
            Ok(()) => return Ok(target),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists || target.is_dir() => {}
            Err(error) => {
                let verb = if copy { "copy" } else { "move" };
                return Err(format!(
                    "cannot {verb} {} to {}: {error}",
                    path.display(),
                    target.display()
                ));
            }
        }
        let mut numbered = OsString::from(path.file_stem().unwrap_or(name));
        numbered.push(format!(" ({number})"));
        if let Some(extension) = path.extension() {
            numbered.push(".");
            numbered.push(extension);
        }
        target = dir.join(numbered);
        number += 1;
    }
}

fn transfer(path: &Path, target: &Path, copy: bool) -> std::io::Result<()> {
    fn wide_path(path: &Path) -> std::io::Result<Vec<u16>> {
        let name = path.file_name().ok_or(std::io::ErrorKind::InvalidInput)?;
        let dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let absolute = dir.canonicalize()?.join(name);
        Ok(absolute.as_os_str().encode_wide().chain([0]).collect())
    }

    let source = wide_path(path)?;
    let target = wide_path(target)?;
    let (source, target) = (PCWSTR(source.as_ptr()), PCWSTR(target.as_ptr()));
    unsafe {
        if copy {
            CopyFileW(source, target, true)
        } else {
            MoveFileExW(source, target, MOVE_FILE_FLAGS(0))
        }
    }
    .map_err(|error| std::io::Error::from_raw_os_error(error.code().0 & 0xffff))
}

/// Opens the file's folder in Explorer with the file selected (`ShellHelper.ShowInExplorer`).
pub fn show_in_explorer(path: &Path) -> Result<(), String> {
    shell_file(path, |wide| unsafe {
        let mut item = std::ptr::null_mut();
        SHParseDisplayName(wide, None, &mut item, 0, None)?;
        let result = SHOpenFolderAndSelectItems(item, None, 0);
        CoTaskMemFree(Some(item.cast()));
        result
    })
}

/// A file operation waiting for the name or folder the user has to type in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pending {
    Rename,
    CopyTo(PathBuf),
}

impl Pending {
    /// What the text field starts with: the current name, or the folder a copy would land in.
    pub fn prefill(&self, path: &Path) -> String {
        match self {
            Pending::Rename => stem(path),
            Pending::CopyTo(dir) => dir.display().to_string(),
        }
    }

    /// Runs the operation with what the user typed. The result is the file the viewer now points at.
    pub fn apply(&self, path: &Path, text: &str) -> Result<PathBuf, String> {
        match self {
            Pending::Rename => rename(path, text),
            Pending::CopyTo(_) => copy_to(path, Path::new(text.trim())),
        }
    }
}

/// What a rename prompt pre-fills: the name without its extension.
pub fn stem(path: &Path) -> String {
    let name = file_name(path);
    match name.rfind('.') {
        Some(dot) if dot > 0 => name[..dot].to_string(),
        _ => name,
    }
}

/// The extension a rename must keep, so the file stays a picture.
pub fn extension(path: &Path) -> String {
    match name_rfind_dot(path) {
        Some(dot) => file_name(path)[dot..].to_string(),
        None => String::new(),
    }
}

fn name_rfind_dot(path: &Path) -> Option<usize> {
    let name = file_name(path);
    let dot = name.rfind('.')?;
    (dot > 0).then_some(dot)
}

#[cfg(test)]
mod tests;
