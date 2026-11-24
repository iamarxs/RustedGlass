//! Standalone checks for folder enumeration error propagation.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rg-folder-tests-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn formats() -> Vec<String> {
    vec![".png".to_string()]
}

#[test]
fn failed_root_open_reports_folder() {
    let missing = std::env::temp_dir().join("rg-folder-tests-missing-root");
    let error = visit_folder(&missing, false, false, &formats(), &|| true, &mut |_, _| {
        panic!("no rows from a failed root")
    })
    .expect_err("missing root must fail");
    assert_eq!(error.folder, missing);
    assert!(error.error.kind() == std::io::ErrorKind::NotFound);
    assert!(error.to_string().contains("rg-folder-tests-missing-root"));
}

#[test]
fn empty_root_is_a_successful_scan() {
    let dir = scratch_dir("empty");
    let seen = Arc::new(AtomicUsize::new(0));
    let seen_read = Arc::clone(&seen);
    let result = visit_folder(&dir, false, false, &formats(), &|| true, &mut |_, _| {
        seen_read.fetch_add(1, Ordering::SeqCst);
    });
    assert!(result.is_ok(), "empty root scans fine");
    assert_eq!(seen.load(Ordering::SeqCst), 0);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cancel_returns_ok_with_partial_rows() {
    let dir = scratch_dir("cancel");
    for index in 0..8 {
        fs::write(dir.join(format!("file{index}.png")), b"").expect("write scratch file");
    }
    // Give up after the first delivered row: the scan must end calmly, not with an error.
    let seen = Arc::new(AtomicUsize::new(0));
    let seen_read = Arc::clone(&seen);
    let result = visit_folder(
        &dir,
        false,
        false,
        &formats(),
        &|| seen_read.load(Ordering::SeqCst) < 1,
        &mut |_, _| {
            seen.fetch_add(1, Ordering::SeqCst);
        },
    );
    assert!(matches!(result, Ok(())), "cancellation is not an error");
    let delivered = seen.load(Ordering::SeqCst);
    assert!(delivered >= 1 && delivered < 8);
    let _ = fs::remove_dir_all(&dir);
}
