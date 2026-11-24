use std::path::{Component, Path, Prefix};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::GetDriveTypeW;

use super::{Event, Lane, Wake};

pub(super) fn safe_playlist_path(path: &Path) -> bool {
    let mut components = path.components();
    let supported = match components.next() {
        Some(Component::Prefix(prefix)) => {
            matches!(
                prefix.kind(),
                Prefix::Disk(_)
                    | Prefix::VerbatimDisk(_)
                    | Prefix::UNC(..)
                    | Prefix::VerbatimUNC(..)
            ) && path.is_absolute()
        }
        Some(Component::RootDir) => false,
        _ => true,
    };
    supported
        && !path.components().any(
            |part| matches!(part, Component::Normal(name) if name.to_string_lossy().contains(':')),
        )
}

pub(super) fn is_remote(path: &Path) -> bool {
    match path.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::UNC(..) | Prefix::VerbatimUNC(..) => true,
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                // GetDriveTypeW accepts a drive root and identifies mapped network drives.
                let root = [u16::from(letter), b':' as u16, b'\\' as u16, 0];
                const DRIVE_REMOTE: u32 = 4;
                unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) == DRIVE_REMOTE }
            }
            _ => false,
        },
        _ => false,
    }
}

pub(super) fn ask_remote(
    tag: u64,
    events: &Lane<Event>,
    wake: &Wake,
    active: &impl Fn() -> bool,
) -> bool {
    let (respond, result) = mpsc::sync_channel(1);
    wake.deliver(events, Event::RemoteImport { tag, respond });
    while active() {
        match result.recv_timeout(Duration::from_millis(50)) {
            Ok(allowed) => return allowed,
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
    false
}
