//! The helper holds the installation reservation until replacement or rollback is complete.

use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::{Component, Path};
use std::process::{Child, Command};

use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
};

use super::staged::VerifiedFile;
use super::{
    current_exe, signals, Stage, BACKUP_NAME, CLOSE_WAIT_MS, EXPECTED_ENV, HANDOFF_ENV,
    HELPER_NAME, NEXT_NAME,
};

struct Args {
    parent_pid: u32,
    target: OsString,
    original: Vec<OsString>,
}

impl Args {
    fn parse(args: Vec<OsString>) -> Result<Self, String> {
        let [pid, target, separator, original @ ..] = args.as_slice() else {
            return Err("The update request is incomplete.".to_owned());
        };
        let parent_pid = pid
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|&pid| pid > 0)
            .ok_or("Invalid update process ID.")?;
        if separator != "--" || !is_exe_name(target) {
            return Err("The update request is malformed.".to_owned());
        }
        Ok(Self {
            parent_pid,
            target: target.clone(),
            original: original.to_vec(),
        })
    }
}

pub(super) fn run(args: Vec<OsString>) -> Result<(), String> {
    let args = Args::parse(args)?;
    let expected = std::env::var(EXPECTED_ENV)
        .map_err(|_| "The update request has no expected checksum.".to_owned())?;
    std::env::remove_var(EXPECTED_ENV);
    let own_exe =
        current_exe().map_err(|error| format!("Could not locate the update helper: {error}"))?;
    let stage_path = own_exe.parent().ok_or("Missing staging folder.")?;
    let parent = stage_path.parent().ok_or("Missing installation folder.")?;
    let stage = stage_path
        .file_name()
        .and_then(|name| Stage::open(parent, name))
        .filter(|stage| stage.0.join(HELPER_NAME) == own_exe)
        .ok_or("This process is not a staged update helper.")?;
    let target = parent.join(&args.target);
    if target.canonicalize().ok().as_deref() != Some(target.as_path()) {
        return Err("The update target changed after staging.".to_owned());
    }

    let _reservation = signals::open_reservation(parent)?;
    let viewer = signals::process(args.parent_pid, &target)?;
    signals::signal(&signals::event_name("armed", stage.name()))?;
    if !signals::wait_one(&viewer, CLOSE_WAIT_MS) {
        return Err(
            "RustedGlass did not close in time. The current version is still installed.".to_owned(),
        );
    }

    let result = install(&stage, &target, &expected, &args.original);
    stage.clean();
    result
}

fn install(
    stage: &Stage,
    target: &Path,
    expected: &str,
    original: &[OsString],
) -> Result<(), String> {
    let next = stage.0.join(NEXT_NAME);
    let backup = stage.0.join(BACKUP_NAME);
    let verified = verify_staged(&next, expected)?;
    // A second viewer running from this file keeps it locked for writing.
    File::options().write(true).open(target)
        .map_err(|error| format!("Could not open RustedGlass for replacement. Close any other instances and try again.\n{error}"))?;
    let ready = signals::create(&signals::event_name("ready", stage.name()))?;
    replace_with_backup(target, verified, &backup)?;

    let handoff = format!("{}:{}", std::process::id(), stage.name());
    let launch = || {
        Command::new(target)
            .args(original)
            .env(HANDOFF_ENV, &handoff)
            .spawn()
    };
    let failure = match launch() {
        Ok(mut child) => {
            if signals::wait_for_signal(&ready, &child, 30_000) {
                let _ = fs::remove_file(&backup);
                return Ok(());
            }
            stop_child(&mut child);
            "The new version did not report a successful start.".to_owned()
        }
        Err(error) => format!("Could not start the new version: {error}"),
    };
    drop(ready);
    replace(&backup, target).map_err(|error| {
        format!(
            "{failure}\nCould not restore the previous executable. It is saved at {}.\n{error}",
            backup.display()
        )
    })?;
    match launch() {
        Ok(_) => Err(format!(
            "{failure}\nThe previous version was restored and restarted."
        )),
        Err(error) => Err(format!(
            "{failure}\nThe previous version was restored. You can start it again.\n{error}"
        )),
    }
}

fn stop_child(child: &mut Child) {
    if child.try_wait().ok().flatten().is_none() && child.kill().is_ok() {
        let _ = child.wait();
    }
}

pub(super) fn replace_with_backup(
    target: &Path,
    next: VerifiedFile,
    backup: &Path,
) -> Result<(), String> {
    let mut old = File::open(target)
        .map_err(|error| format!("Could not read the current executable: {error}"))?;
    let mut spare = File::options()
        .write(true)
        .create_new(true)
        .open(backup)
        .map_err(|error| format!("Could not create an update backup: {error}"))?;
    let copy = io::copy(&mut old, &mut spare).and_then(|_| spare.sync_all());
    drop(spare);
    drop(old);
    let result = copy
        .map_err(|error| error.to_string())
        .and_then(|_| next.replace(target));
    if let Err(error) = result {
        let _ = fs::remove_file(backup);
        return Err(format!(
            "Could not replace the executable. The current version is still installed.\n{error}"
        ));
    }
    Ok(())
}

/// Re-reads the staged `next.exe` through a non-write-shared handle and hashes it, so the
/// bytes swapped in are the bytes the viewer verified.
pub(super) fn verify_staged(next: &Path, expected: &str) -> Result<VerifiedFile, String> {
    let Some(expected) = parse_sha256(expected) else {
        return Err("The update request has an invalid expected checksum.".to_owned());
    };
    VerifiedFile::open(next, expected)
}

/// 64 lowercase hex characters, as produced by `format!("{:x}", digest)`.
fn parse_sha256(text: &str) -> Option<[u8; 32]> {
    let bytes = text.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (out, pair) in out.iter_mut().zip(bytes.chunks(2)) {
        let hi = (*pair.first()? as char).to_digit(16)?;
        let lo = (*pair.get(1)? as char).to_digit(16)?;
        *out = (hi * 16 + lo) as u8;
    }
    Some(out)
}

pub(super) fn replace(source: &Path, destination: &Path) -> Result<(), String> {
    let wide = |path: &Path| {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let source = wide(source);
    let destination = wide(destination);
    // Both paths are on the same volume. Keep the old name in place until the replacement is ready.
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| error.to_string())
}

pub(super) fn is_exe_name(name: &OsStr) -> bool {
    let path = Path::new(name);
    !name.encode_wide().any(|unit| unit == u16::from(b':'))
        && matches!(path.components().next(), Some(Component::Normal(_)))
        && path.components().count() == 1
        && path.file_name() == Some(name)
        && path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
}
