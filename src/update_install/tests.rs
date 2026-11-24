use std::ffi::OsStr;
use std::fs;
use std::os::windows::fs::OpenOptionsExt;
use std::path::PathBuf;

use sha2::{Digest, Sha256};

use super::helper::{is_exe_name, replace, replace_with_backup, verify_staged};
use super::{current_exe, signals, Stage, BACKUP_NAME};

fn fixture(name: &str) -> PathBuf {
    let dir = crate::test_path(&format!("update-install-{name}"));
    fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn helper_target_must_be_one_executable_name() {
    for invalid in [
        "",
        ".",
        "..",
        r"dir\app.exe",
        "dir/app.exe",
        "C:app.exe",
        "app:stream.exe",
        "app",
    ] {
        assert!(!is_exe_name(OsStr::new(invalid)), "{invalid}");
    }
    assert!(is_exe_name(OsStr::new("A renamed viewer.EXE")));
}

#[test]
fn staged_bytes_must_match_the_expected_checksum() {
    let dir = fixture("verify");
    let next = dir.join("next.exe");
    fs::write(&next, b"new").unwrap();
    assert!(verify_staged(&next, &hex(b"new")).is_ok());
    assert!(verify_staged(&next, &hex(b"old")).is_err());
    assert!(verify_staged(&next, "not-a-checksum").is_err());
    assert!(verify_staged(&dir.join("missing.exe"), &hex(b"new")).is_err());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn atomic_replacement_and_restore_keep_the_original_bytes() {
    let dir = fixture("atomic-swap");
    let target = dir.join("app.exe");
    let next = dir.join("next.exe");
    let backup = dir.join("backup.exe");
    fs::write(&target, b"old").unwrap();
    fs::write(&next, b"new").unwrap();

    let verified = verify_staged(&next, &hex(b"new")).unwrap();
    assert!(fs::OpenOptions::new().write(true).open(&next).is_err());
    replace_with_backup(&target, verified, &backup).unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"new");
    assert_eq!(fs::read(&backup).unwrap(), b"old");
    replace(&backup, &target).unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"old");
    fs::remove_file(target).unwrap();
    fs::remove_dir(dir).unwrap();
}

#[test]
fn failed_replacement_leaves_the_existing_exe_in_place() {
    let dir = fixture("atomic-failure");
    let target = dir.join("app.exe");
    let next = dir.join("next.exe");
    let backup = dir.join("backup.exe");
    fs::write(&target, b"old").unwrap();
    fs::write(&next, b"new").unwrap();
    let target_reader = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&target)
        .unwrap();
    let verified = verify_staged(&next, &hex(b"new")).unwrap();
    assert!(replace_with_backup(&target, verified, &backup).is_err());
    drop(target_reader);
    assert_eq!(fs::read(&target).unwrap(), b"old");
    assert!(!backup.exists());
    fs::remove_file(next).unwrap();
    fs::remove_file(target).unwrap();
    fs::remove_dir(dir).unwrap();
}

#[test]
fn handoff_holds_the_reservation_until_the_helper_releases_it() {
    let dir = fixture("reservation");
    let ui = signals::reserve(&dir).unwrap();
    assert!(signals::reserve(&dir).is_err());
    let helper = signals::open_reservation(&dir).unwrap();
    drop(ui);
    assert!(signals::reserve(&dir).is_err());
    drop(helper);
    assert!(signals::reserve(&dir).is_ok());
    fs::remove_dir(dir).unwrap();
}

#[test]
fn cleanup_keeps_the_backup_for_recovery() {
    let dir = fixture("backup");
    let stage = Stage::create(&dir).unwrap();
    let backup = stage.0.join(BACKUP_NAME);
    fs::write(&backup, b"old").unwrap();
    stage.clean();
    assert_eq!(fs::read(&backup).unwrap(), b"old");
    fs::remove_file(backup).unwrap();
    stage.clean();
    assert!(!stage.0.exists());
    fs::remove_dir(dir).unwrap();
}

#[test]
fn the_current_exe_is_a_real_canonical_path() {
    let exe = current_exe().unwrap();
    assert!(exe.is_file());
    assert_eq!(exe.canonicalize().unwrap(), exe);
}
