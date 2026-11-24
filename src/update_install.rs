//! Handoff from the running viewer to an isolated, temporary copy of itself.

mod helper;
mod signals;
mod staged;
#[cfg(test)]
mod tests;

use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use windows::core::{w, PCWSTR};
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

const HELPER_ARG: &str = "--rg-update-helper";
const HELPER_NAME: &str = "helper.exe";
const NEXT_NAME: &str = "next.exe";
const BACKUP_NAME: &str = "previous.exe";
const STAGE_PREFIX: &str = ".rustedglass-update-";
const HANDOFF_ENV: &str = "RUSTEDGLASS_UPDATE_HANDOFF";
const EXPECTED_ENV: &str = "RUSTEDGLASS_UPDATE_EXPECTED";
const CLOSE_WAIT_MS: u32 = 30_000;

struct Stage(PathBuf);

impl Stage {
    fn create(parent: &Path) -> io::Result<Self> {
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let path = parent.join(format!("{STAGE_PREFIX}{}-{time}", std::process::id()));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn name(&self) -> &str {
        // Stage::create and Stage::open accept only ASCII names.
        self.0
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or_default()
    }

    fn open(parent: &Path, name: &OsStr) -> Option<Self> {
        let token = name.to_str()?.strip_prefix(STAGE_PREFIX)?;
        let (pid, time) = token.split_once('-')?;
        if pid.parse::<u32>().ok()? == 0 || time.parse::<u128>().is_err() {
            return None;
        }
        let path = parent.join(name);
        // The stage must remain a real directory next to the installed executable.
        (path.canonicalize().ok()? == path && path.is_dir()).then_some(Self(path))
    }

    fn clean(&self) {
        if !matches!(fs::symlink_metadata(self.0.join(BACKUP_NAME)), Err(e) if e.kind() == io::ErrorKind::NotFound)
        {
            return;
        }
        for name in [NEXT_NAME, HELPER_NAME] {
            let _ = fs::remove_file(self.0.join(name));
        }
        let _ = fs::remove_dir(&self.0);
    }
}

/// The caller may close normally once the helper owns a handle to the current process.
pub(crate) fn schedule(exe: &[u8]) -> Result<(), String> {
    let target = current_exe().map_err(|error| format!("Could not locate RustedGlass: {error}"))?;
    let parent = target
        .parent()
        .ok_or("The executable has no parent folder.")?;
    let target_name = target
        .file_name()
        .ok_or("The executable has no filename.")?;
    let _reservation = signals::reserve(parent)?;
    let stage = Stage::create(parent)
        .map_err(|error| format!("Could not stage the update next to RustedGlass: {error}"))?;
    let result = start_helper(&stage, &target, target_name, exe);
    if result.is_err() {
        stage.clean();
    }
    result
}

fn start_helper(
    stage: &Stage,
    target: &Path,
    target_name: &OsStr,
    exe: &[u8],
) -> Result<(), String> {
    let helper = stage.0.join(HELPER_NAME);
    // Only the checksum crosses the boundary; the helper re-checks the staged bytes itself.
    let expected = format!("{:x}", Sha256::digest(exe));
    let prepare = || -> io::Result<()> {
        // create_new: a pre-placed helper.exe fails the update instead of being executed.
        let mut source = File::open(target)?;
        let mut copy = File::options().write(true).create_new(true).open(&helper)?;
        io::copy(&mut source, &mut copy).and_then(|_| copy.sync_all())?;
        let mut f = File::options()
            .write(true)
            .create_new(true)
            .open(stage.0.join(NEXT_NAME))?;
        f.write_all(exe)?;
        f.sync_all()
    };
    prepare().map_err(|error| format!("Could not stage the update: {error}"))?;
    let armed = signals::create(&signals::event_name("armed", stage.name()))?;
    let mut child = Command::new(helper)
        .arg(HELPER_ARG)
        .arg(std::process::id().to_string())
        .arg(target_name)
        .arg("--")
        .args(std::env::args_os().skip(1))
        .env_remove(HANDOFF_ENV)
        .env(EXPECTED_ENV, &expected)
        .spawn()
        .map_err(|error| format!("Could not start the update helper: {error}"))?;
    if signals::wait_for_signal(&armed, &child, 10_000) {
        return Ok(());
    }
    if child.try_wait().ok().flatten().is_none() && child.kill().is_ok() {
        let _ = child.wait();
    }
    Err(
        "The update helper did not confirm it was ready. The current version is still installed."
            .to_owned(),
    )
}

pub(crate) fn run_helper_if_requested() -> bool {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(OsStr::new(HELPER_ARG)) {
        return false;
    }
    if let Err(error) = helper::run(args.collect()) {
        report_failure(&error);
    }
    true
}

/// A newly restarted viewer acknowledges startup and removes the temporary helper after it exits.
pub(crate) fn signal_ready() {
    let Ok(handoff) = std::env::var(HANDOFF_ENV) else {
        return;
    };
    std::env::remove_var(HANDOFF_ENV);
    let Some((pid, name)) = handoff.split_once(':') else {
        return;
    };
    let Ok(pid) = pid.parse::<u32>() else {
        return;
    };
    let Ok(exe) = current_exe() else { return };
    let Some(stage) = exe
        .parent()
        .and_then(|parent| Stage::open(parent, OsStr::new(name)))
    else {
        return;
    };
    let Ok(helper) = signals::process(pid, &stage.0.join(HELPER_NAME)) else {
        return;
    };
    let _ = signals::signal(&signals::event_name("ready", stage.name()));
    let _ = std::thread::Builder::new()
        .name("update-cleanup".to_owned())
        .spawn(move || {
            if signals::wait_one(&helper, u32::MAX) {
                stage.clean();
            }
        });
}

fn current_exe() -> io::Result<PathBuf> {
    std::env::current_exe()?.canonicalize()
}

fn report_failure(message: &str) {
    let text = crate::w32::wide(message);
    let _ = unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            w!("RustedGlass update"),
            MB_OK | MB_ICONERROR,
        )
    };
}
