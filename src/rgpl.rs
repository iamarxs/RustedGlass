//! The `.rgpl` playlist file: one path per line, a line starting with `#` is a comment.
//!
//! Paths relative to the playlist file's own folder stay relative, so a folder that is moved
//! or copied somewhere else still loads.

use std::fs;
use std::path::{Path, PathBuf};

pub const EXTENSION: &str = "rgpl";

/// The lines of a playlist file, comments and empty lines gone. `base` is the file the text came
/// from: a relative path is taken from its folder.
pub fn parse(text: &str, base: &Path) -> Vec<PathBuf> {
    let directory = base.parent().unwrap_or(base);

    text.trim_start_matches('\u{feff}')
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(|line| {
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
        })
        .collect()
}

pub fn read_file(path: &Path) -> Result<Vec<PathBuf>, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{path:?}: {error}"))?;
    Ok(parse(&text, path))
}

/// Writes the list, one path per line. Paths on the same volume are written relative to
/// it, so the same folder can be moved and the playlist still loads.
pub fn write_file(path: &Path, files: &[PathBuf]) -> Result<usize, String> {
    let path = std::path::absolute(path).map_err(|e| e.to_string())?;
    let directory = path.parent().unwrap_or(&path);
    let mut text = String::new();

    for file in files {
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
    }

    crate::atomic_file::write(&path, text.as_bytes())?;
    Ok(files.len())
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
        let text = "# a comment\na.png\n\n# another\nb.png\n\t\n";
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
    fn only_a_rgpl_is_a_playlist() {
        assert!(is_playlist_file(Path::new("my.rgpl")));
        assert!(!is_playlist_file(Path::new("my.png")));
        assert!(!is_playlist_file(Path::new("no extension")));
    }
}
