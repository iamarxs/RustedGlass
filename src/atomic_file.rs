use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Replace a file only after its complete new contents have been written to the same directory.
pub fn write(path: &Path, contents: &[u8]) -> Result<(), String> {
    write_with(path, |output| output.write_all(contents))
}

pub fn copy(source: &Path, target: &Path) -> Result<(), String> {
    let mut input = fs::File::open(source).map_err(|error| format!("{source:?}: {error}"))?;
    write_with(target, |output| io::copy(&mut input, output).map(|_| ()))
}

/// Replace a file after the callback has written and flushed its contents.
pub fn write_with(
    path: &Path,
    write: impl FnOnce(&mut fs::File) -> io::Result<()>,
) -> Result<(), String> {
    let directory = path.parent().unwrap_or(Path::new("."));
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let pending = directory.join(format!(
        ".rustedglass-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut output = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&pending)
        .map_err(|error| format!("{pending:?}: {error}"))?;
    let written = write(&mut output).and_then(|_| output.sync_all());
    drop(output);
    if let Err(error) = written.and_then(|_| fs::rename(&pending, path)) {
        let _ = fs::remove_file(&pending);
        return Err(format!("{path:?}: {error}"));
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;

    #[test]
    fn failed_replace_keeps_the_previous_file_and_removes_the_temporary_file() {
        let dir = crate::test_path("atomic-file-test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, b"previous").unwrap();
        let locked = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();

        let result = write(&path, b"replacement");
        drop(locked);
        let contents = fs::read(&path).unwrap();
        let remaining = fs::read_dir(&dir).unwrap().count();
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
        assert!(result.is_err());
        assert_eq!(contents, b"previous");
        assert_eq!(remaining, 1);
    }
}
