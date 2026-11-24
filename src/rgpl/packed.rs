use std::io::{self, Read, Seek};

use super::*;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

const MANIFEST: &str = "playlist.rgpl";

/// Checks cancellation before every archive read, including central-directory and
/// decompression chunks, so a superseded read stops instead of finishing its metadata.
struct ActiveFile<'a> {
    file: fs::File,
    active: &'a dyn Fn() -> bool,
}

impl Read for ActiveFile<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if !(self.active)() {
            // read_exact retries Interrupted, so cancellation must be non-retryable.
            return Err(io::Error::other("playlist read cancelled"));
        }
        self.file.read(buffer)
    }
}

impl Seek for ActiveFile<'_> {
    fn seek(&mut self, position: io::SeekFrom) -> io::Result<u64> {
        if !(self.active)() {
            return Err(io::Error::other("playlist read cancelled"));
        }
        self.file.seek(position)
    }
}

pub(super) fn visit(
    file: fs::File,
    path: &Path,
    active: &dyn Fn() -> bool,
    row: &mut dyn FnMut(PathBuf),
) -> io::Result<()> {
    if !active() {
        return Ok(());
    }
    let mut archive = match ZipArchive::new(ActiveFile { file, active }) {
        Ok(archive) => archive,
        Err(_) if !active() => return Ok(()),
        Err(error) => return Err(io::Error::other(error)),
    };
    if !active() {
        return Ok(());
    }
    if archive.len() != 1 {
        return Err(invalid_package());
    }
    let manifest = archive.by_index(0).map_err(io::Error::other)?;
    if !active() {
        return Ok(());
    }
    if manifest.name_raw() != MANIFEST.as_bytes() || !manifest.is_file() || manifest.is_symlink() {
        return Err(invalid_package());
    }
    // Keep path resolution relative to the archive, and send no member to disk.
    let result = visit_rows(manifest, path, active, row);
    if !active() {
        return Ok(());
    }
    result
}

fn invalid_package() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "compressed playlist must contain just playlist.rgpl",
    )
}

pub(super) fn write<'a>(
    output: &mut fs::File,
    files: impl IntoIterator<Item = &'a PathBuf>,
) -> io::Result<usize> {
    let mut archive = ZipWriter::new(output);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .large_file(true);
    archive
        .start_file(MANIFEST, options)
        .map_err(io::Error::other)?;
    let count = write_rows(&mut archive, files)?;
    archive.finish().map_err(io::Error::other)?;
    Ok(count)
}
