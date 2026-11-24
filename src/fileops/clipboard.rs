//! Copies a file's absolute name to the clipboard as text.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;

/// Copies the file's absolute Windows path as Unicode text.
pub(crate) fn copy_path(hwnd: HWND, path: &Path) -> Result<(), String> {
    let name = path_text(path).map_err(|error| format!("Could not copy file path: {error}"))?;
    let size = name.len() * std::mem::size_of::<u16>();
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, size)
            .map_err(|error| format!("Could not copy file path: {error}"))?;
        let result = (|| {
            let bytes = GlobalLock(memory).cast::<u16>();
            if bytes.is_null() {
                return Err(windows::core::Error::from_win32());
            }
            std::ptr::copy_nonoverlapping(name.as_ptr(), bytes, name.len());
            let _ = GlobalUnlock(memory);

            OpenClipboard(Some(hwnd))?;
            let result = EmptyClipboard().and_then(|()| {
                SetClipboardData(u32::from(CF_UNICODETEXT.0), Some(HANDLE(memory.0)))
            });
            let _ = CloseClipboard();
            result.map(|_| ())
        })();
        if result.is_err() {
            let _ = GlobalFree(Some(memory));
        }
        result.map_err(|error| format!("Could not copy file path: {error}"))
    }
}

fn path_text(path: &Path) -> std::io::Result<Vec<u16>> {
    Ok(std::path::absolute(path)?
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_path_is_absolute_unicode_text() {
        let path = Path::new("Kuva ä 🖼.png");
        let text = path_text(path).unwrap();
        assert_eq!(text.last(), Some(&0));
        assert_eq!(
            String::from_utf16(&text[..text.len() - 1]).unwrap(),
            std::env::current_dir()
                .unwrap()
                .join(path)
                .to_string_lossy()
        );
    }
}
