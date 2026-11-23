//! The `.rgpl` playlist file: one path per line, a line starting with `#` is a comment.
//!
//! Paths relative to the playlist file's own folder stay relative, so a folder that is moved
//! or copied somewhere else still loads.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

pub const EXTENSION: &str = "rgpl";

/// Largest playlist file read; a larger file is refused instead of being loaded.
pub const MAX_INPUT_BYTES: u64 = 16 * 1024 * 1024;
/// Most entries a playlist may list; a file listing one more is refused.
pub const MAX_ENTRIES: usize = 100_000;

/// The lines of a playlist file, comments and empty lines gone. `base` is the file the text came
/// from: a relative path is taken from its folder.
#[cfg(test)]
pub fn parse(text: &str, base: &Path) -> Vec<PathBuf> {
    parse_capped(text, base, usize::MAX).0
}

/// `parse` with a limit: also says whether the text held more entries than the limit.
fn parse_capped(text: &str, base: &Path, limit: usize) -> (Vec<PathBuf>, bool) {
    let directory = base.parent().unwrap_or(base);
    let mut paths = Vec::new();

    for line in text.trim_start_matches('\u{feff}').lines() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        if paths.len() == limit {
            return (paths, true);
        }
        paths.push(resolve(line, directory));
    }
    (paths, false)
}

fn resolve(line: &str, directory: &Path) -> PathBuf {
    let path = PathBuf::from(line);
    if path.is_relative() {
        let joined = directory.join(&path);
        if joined.is_absolute() {
            std::path::absolute(&joined).unwrap_or(joined)
        } else {
            joined
        }
    } else {
        path
    }
}

pub fn read_file(path: &Path) -> Result<Vec<PathBuf>, String> {
    let mut file = fs::File::open(path).map_err(|error| format!("{path:?}: {error}"))?;
    let bytes =
        read_capped(&mut file, MAX_INPUT_BYTES).map_err(|error| format!("{path:?}: {error}"))?;
    if bytes.len() as u64 > MAX_INPUT_BYTES {
        return Err(format!(
            "{path:?}: playlist is larger than {} MiB",
            MAX_INPUT_BYTES / (1024 * 1024)
        ));
    }
    let text =
        String::from_utf8(bytes).map_err(|_| format!("{path:?}: playlist is not valid UTF-8"))?;

    let (entries, more) = parse_capped(&text, path, MAX_ENTRIES);
    if more {
        return Err(format!(
            "{path:?}: playlist has more than {MAX_ENTRIES} entries"
        ));
    }
    Ok(entries)
}

/// Reads at most `cap` bytes plus one, so a file exactly at the cap is accepted and one byte
/// over it is detected.
fn read_capped(file: &mut fs::File, cap: u64) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    file.by_ref().take(cap + 1).read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Writes the list, one path per line. Paths on the same volume are written relative to
/// it, so the same folder can be moved and the playlist still loads. A save that would not
/// read back (over the entry or byte cap) fails before the old playlist is replaced.
pub fn write_file<'a>(
    path: &Path,
    files: impl IntoIterator<Item = &'a PathBuf>,
) -> Result<usize, String> {
    let path = std::path::absolute(path).map_err(|e| e.to_string())?;
    let directory = path.parent().unwrap_or(&path);
    let mut text = String::new();
    let mut count = 0usize;
    let over_limit = || {
        format!(
            "{path:?}: save exceeds the playlist caps of {MAX_ENTRIES} entries / {} MiB",
            MAX_INPUT_BYTES / (1024 * 1024)
        )
    };

    for file in files {
        if count == MAX_ENTRIES {
            return Err(over_limit());
        }
        let absolute = std::path::absolute(file).map_err(|e| e.to_string())?;
        let written = relative_to(directory, &absolute).unwrap_or(absolute);
        let line = written
            .to_str()
            .ok_or_else(|| format!("{written:?} is not valid UTF-8"))?;
        if line.starts_with('#') {
            text.push_str(".\\");
        }
        text.push_str(line);
        text.push('\n');
        if text.len() as u64 > MAX_INPUT_BYTES {
            return Err(over_limit());
        }
        count += 1;
    }

    crate::atomic_file::write(&path, text.as_bytes())?;
    Ok(count)
}

fn relative_to(directory: &Path, file: &Path) -> Option<PathBuf> {
    for (depth, ancestor) in directory
        .ancestors()
        .filter(|path| path.has_root())
        .enumerate()
    {
        if let Ok(suffix) = file.strip_prefix(ancestor) {
            let mut relative: PathBuf = std::iter::repeat_n("..", depth).collect();
            relative.push(suffix);
            return Some(relative);
        }
    }
    None
}

pub fn is_playlist_file(path: &Path) -> bool {
    path.extension()
        .map(|e| e.eq_ignore_ascii_case(EXTENSION))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_and_blank_lines_are_not_entries() {
        let dir = crate::test_path("rgpl_comments");
        fs::create_dir_all(&dir).unwrap();
        for name in ["a.png", "b.png"] {
            fs::write(dir.join(name), b"x").unwrap();
        }

        let list_file = dir.join("notes.rgpl");
        let text = "\u{feff}# a comment\na.png\n\n# another\nb.png\n\t\n";
        assert_eq!(
            parse(text, &list_file),
            vec![dir.join("a.png"), dir.join("b.png")]
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_relative_path_is_taken_from_the_playlist_folder() {
        let dir = crate::test_path("rgpl_relative");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.png"), b"x").unwrap();

        let list_file = dir.join("list.rgpl");
        fs::write(&list_file, "a.png\nmissing.png\n").unwrap();

        assert_eq!(
            read_file(&list_file).unwrap(),
            vec![dir.join("a.png"), dir.join("missing.png")],
            "missing paths are returned for the viewer to report"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_written_playlist_reads_back_as_the_same_list() {
        let dir = crate::test_path("rgpl_round_trip");
        let outside = crate::test_path("rgpl_round_trip_outside");
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        for name in ["a.png", "sub/b.png"] {
            fs::write(dir.join(name), b"x").unwrap();
        }
        let away = outside.join("c.png");
        fs::write(&away, b"x").unwrap();

        let list_file = dir.join("saved.rgpl");
        let files = vec![
            dir.join("a.png"),
            dir.join("sub/b.png"),
            away,
            dir.join("a.png"),
        ];
        assert_eq!(write_file(&list_file, &files).unwrap(), 4);

        let text = fs::read_to_string(&list_file).unwrap();
        assert!(
            text.contains("a.png\n"),
            "a path below the folder is written relative"
        );
        assert!(
            !text.contains("./a.png"),
            "no leading separator to confuse a reader"
        );
        assert_eq!(
            read_file(&list_file).unwrap(),
            files,
            "the same list comes back"
        );

        fs::remove_dir_all(&dir).ok();
        fs::remove_dir_all(&outside).ok();
    }

    #[test]
    fn a_save_over_the_caps_refuses_and_keeps_the_old_file() {
        let dir = crate::test_path("rgpl_save_cap");
        fs::create_dir_all(&dir).unwrap();
        let list_file = dir.join("capped.rgpl");
        fs::write(&list_file, "keep\n").unwrap();

        let too_many: Vec<PathBuf> = (0..=MAX_ENTRIES)
            .map(|i| dir.join(format!("{i}.png")))
            .collect();
        let error = write_file(&list_file, &too_many).unwrap_err();
        assert!(error.contains(&MAX_ENTRIES.to_string()), "{error}");
        assert_eq!(fs::read_to_string(&list_file).unwrap(), "keep\n");

        // Several normal-length names over the byte cap, so the entry cap alone is not hit.
        let long_name = "x".repeat(200);
        let bytes_over: Vec<PathBuf> = (0..83_000)
            .map(|i| dir.join(format!("{long_name}{i}")))
            .collect();
        let error = write_file(&list_file, &bytes_over).unwrap_err();
        assert!(error.contains("MiB"), "{error}");
        assert_eq!(fs::read_to_string(&list_file).unwrap(), "keep\n");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn only_a_rgpl_is_a_playlist() {
        assert!(is_playlist_file(Path::new("my.rgpl")));
        assert!(!is_playlist_file(Path::new("my.png")));
        assert!(!is_playlist_file(Path::new("no extension")));
    }

    #[test]
    fn the_input_cap_accepts_exactly_the_cap_and_detects_one_byte_more() {
        let dir = crate::test_path("rgpl_input_cap");
        fs::create_dir_all(&dir).unwrap();
        let data = dir.join("data.txt");

        fs::write(&data, b"abcd").unwrap();
        let mut file = fs::File::open(&data).unwrap();
        assert_eq!(read_capped(&mut file, 4).unwrap(), b"abcd");

        fs::write(&data, b"abcde").unwrap();
        let mut file = fs::File::open(&data).unwrap();
        assert_eq!(
            read_capped(&mut file, 4).unwrap().len(),
            5,
            "the caller sees cap-plus-one and can refuse the file"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_entry_cap_loads_at_the_cap_and_refuses_one_entry_more() {
        let dir = crate::test_path("rgpl_entry_cap");
        fs::create_dir_all(&dir).unwrap();
        let list_file = dir.join("many.rgpl");

        let at_cap = "a.png\n".repeat(MAX_ENTRIES);
        fs::write(&list_file, &at_cap).unwrap();
        assert_eq!(read_file(&list_file).unwrap().len(), MAX_ENTRIES);

        fs::write(&list_file, format!("{at_cap}a.png\n")).unwrap();
        let error = read_file(&list_file).unwrap_err();
        assert!(error.contains(&MAX_ENTRIES.to_string()), "{error}");

        fs::remove_dir_all(&dir).ok();
    }
}
