//! The `.rgpl` playlist file: one path per line, a line starting with `#` is a comment.
//!
//! Saves use absolute paths. Relative paths in existing playlists still resolve from the
//! playlist's folder.

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

pub const EXTENSION: &str = "rgpl";
pub const PACKED_EXTENSION: &str = "rgplz";

mod packed;

/// The lines of a playlist file, comments and empty lines gone. `base` is the file the text came
/// from: a relative path is taken from its folder.
#[cfg(test)]
pub fn parse(text: &str, base: &Path) -> Vec<PathBuf> {
    read_rows(text.as_bytes(), base).unwrap()
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

/// Collects the streamed rows for the parser and format tests.
#[cfg(test)]
pub fn read_file(path: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    visit_file(path, &|| true, &mut |path| paths.push(path))?;
    Ok(paths)
}

/// Streams the file's rows to `row` as they are read. `active` is checked before every row
/// and before every chunk of a long row or of decompressed archive data, so a cancelled read
/// returns `Ok` after the caller stops sending rows.
pub fn visit_file(
    path: &Path,
    active: &dyn Fn() -> bool,
    row: &mut dyn FnMut(PathBuf),
) -> Result<(), String> {
    if !active() {
        return Ok(());
    }
    let result = fs::File::open(path).and_then(|file| {
        if has_extension(path, PACKED_EXTENSION) {
            packed::visit(file, path, active, row)
        } else {
            visit_rows(file, path, active, row)
        }
    });
    if active() {
        result.map_err(|error| format!("{path:?}: {error}"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
fn read_rows(input: impl io::Read, path: &Path) -> io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    visit_rows(input, path, &|| true, &mut |path| paths.push(path))?;
    Ok(paths)
}

fn visit_rows(
    input: impl io::Read,
    path: &Path,
    active: &dyn Fn() -> bool,
    row: &mut dyn FnMut(PathBuf),
) -> io::Result<()> {
    let directory = path.parent().unwrap_or(path);
    let mut input = io::BufReader::with_capacity(64 * 1024, input);
    let mut bytes = Vec::new();
    let mut first = true;
    loop {
        if !active() {
            return Ok(());
        }
        let available = input.fill_buf()?;
        if !active() {
            return Ok(());
        }
        if available.is_empty() {
            if !bytes.is_empty() {
                push_row(&bytes, &mut first, directory, row)?;
            }
            return Ok(());
        }
        let take = available
            .iter()
            .position(|&byte| byte == b'\n')
            .map_or(available.len(), |index| index + 1);
        bytes.extend_from_slice(&available[..take]);
        input.consume(take);
        if bytes.last() == Some(&b'\n') {
            push_row(&bytes, &mut first, directory, row)?;
            bytes.clear();
        }
    }
}

fn push_row(
    bytes: &[u8],
    first: &mut bool,
    directory: &Path,
    row: &mut dyn FnMut(PathBuf),
) -> io::Result<()> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "playlist is not valid UTF-8"))?;
    let line = text.lines().next().unwrap_or("");
    let line = if *first {
        *first = false;
        line.trim_start_matches('\u{feff}')
    } else {
        line
    };
    if line.trim().is_empty() || line.starts_with('#') {
        return Ok(());
    }
    row(resolve(line, directory));
    Ok(())
}

/// Streams one absolute path per line. A failure mid-save leaves the old playlist in place.
pub fn write_file<'a>(
    path: &Path,
    files: impl IntoIterator<Item = &'a PathBuf>,
) -> Result<usize, String> {
    let path = std::path::absolute(path).map_err(|e| e.to_string())?;
    let mut count = 0usize;
    crate::atomic_file::write_with(&path, |output| {
        count = if has_extension(&path, PACKED_EXTENSION) {
            packed::write(output, files)?
        } else {
            write_rows(output, files)?
        };
        Ok(())
    })?;
    Ok(count)
}

fn write_rows<'a>(
    output: impl Write,
    files: impl IntoIterator<Item = &'a PathBuf>,
) -> io::Result<usize> {
    let mut output = io::BufWriter::new(output);
    let mut count = 0;
    for file in files {
        let absolute = std::path::absolute(file)?;
        let line = absolute.to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{absolute:?} is not valid UTF-8"),
            )
        })?;
        output.write_all(line.as_bytes())?;
        output.write_all(b"\n")?;
        count += 1;
    }
    output.flush()?;
    Ok(count)
}

pub fn is_playlist_file(path: &Path) -> bool {
    has_extension(path, EXTENSION) || has_extension(path, PACKED_EXTENSION)
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .map(|e| e.eq_ignore_ascii_case(extension))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests;
