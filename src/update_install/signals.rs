//! Named events and owned process handles for the two sides of the update transaction.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::process::Child;

use sha2::{Digest, Sha256};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{
    CreateEventW, OpenEventW, OpenProcess, QueryFullProcessImageNameW, SetEvent,
    WaitForMultipleObjects, WaitForSingleObject, EVENT_MODIFY_STATE, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
};

pub(super) fn event_name(kind: &str, stage: &str) -> String {
    format!("Local\\RustedGlass-{kind}-{stage}")
}

fn reservation_name(path: &Path) -> String {
    let hash = Sha256::digest(path.to_string_lossy().to_lowercase().as_bytes());
    format!("Local\\RustedGlass-update-{hash:x}")
}

pub(super) fn reserve(path: &Path) -> Result<OwnedHandle, String> {
    let handle = create(&reservation_name(path))?;
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return Err("An update is already in progress in this folder.".to_owned());
    }
    Ok(handle)
}

pub(super) fn open_reservation(path: &Path) -> Result<OwnedHandle, String> {
    open(&reservation_name(path))
}

pub(super) fn create(name: &str) -> Result<OwnedHandle, String> {
    let name = crate::w32::wide(name);
    let handle = unsafe { CreateEventW(None, false, false, PCWSTR(name.as_ptr())) }
        .map_err(|error| format!("Could not create an update event: {error}"))?;
    Ok(unsafe { OwnedHandle::from_raw_handle(handle.0) })
}

fn open(name: &str) -> Result<OwnedHandle, String> {
    let name = crate::w32::wide(name);
    let handle = unsafe { OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(name.as_ptr())) }
        .map_err(|error| format!("Could not open an update event: {error}"))?;
    Ok(unsafe { OwnedHandle::from_raw_handle(handle.0) })
}

pub(super) fn signal(name: &str) -> Result<(), String> {
    let event = open(name)?;
    unsafe { SetEvent(HANDLE(event.as_raw_handle())) }
        .map_err(|error| format!("Could not signal an update event: {error}"))
}

pub(super) fn process(pid: u32, image: &Path) -> Result<OwnedHandle, String> {
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            false,
            pid,
        )
    }
    .map_err(|error| format!("Could not track the update process: {error}"))?;
    let process = unsafe { OwnedHandle::from_raw_handle(handle.0) };
    let mut path = vec![0u16; 32768];
    let mut len = path.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut len,
        )
    }
    .map_err(|error| format!("Could not identify the update process: {error}"))?;
    let path = PathBuf::from(OsString::from_wide(&path[..len as usize]));
    if path.canonicalize().ok().as_deref() != Some(image) {
        return Err("The update process does not match the executable being replaced.".to_owned());
    }
    Ok(process)
}

pub(super) fn wait_one(handle: &OwnedHandle, timeout: u32) -> bool {
    unsafe { WaitForSingleObject(HANDLE(handle.as_raw_handle()), timeout) == WAIT_OBJECT_0 }
}

pub(super) fn wait_for_signal(event: &OwnedHandle, child: &Child, timeout: u32) -> bool {
    let handles = [HANDLE(event.as_raw_handle()), HANDLE(child.as_raw_handle())];
    unsafe { WaitForMultipleObjects(&handles, false, timeout) == WAIT_OBJECT_0 }
}
