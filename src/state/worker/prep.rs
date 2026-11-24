//! One deterministic preparation of folders, playlists and dropped batches into the
//! image rows the playlist takes, shared by the background lane and the synchronous
//! `State` methods, so both produce the same files, order and accounting. Nothing here
//! needs a `Worker`: the lane only hands the result to its event queue.
//!
//! `photo::collect` already stats and filters every folder entry it returns (RG12), so a
//! folder's rows are taken as they are, without a second stat or extension pass; every
//! other candidate costs one stat.

use std::path::{Path, PathBuf};

use super::{Event, Lane, Wake};
use crate::{photo, rgpl};

mod progressive;
mod remote;
mod runner;
use remote::{is_remote, safe_playlist_path};
pub(super) use runner::lane;

mod spec;
pub(super) use spec::PrepRequest;
pub use spec::{PreparedImport, Source};

struct Scan<'a> {
    active: &'a dyn Fn() -> bool,
    remote: &'a mut dyn FnMut() -> bool,
    emit: Option<&'a mut dyn FnMut(Vec<PathBuf>)>,
    found: &'a mut dyn FnMut(&Path),
}

impl Scan<'_> {
    fn flush(&mut self, rows: &mut PreparedImport) {
        if rows.files.len() == IMPORT_BATCH {
            if let Some(emit) = &mut self.emit {
                emit(std::mem::take(&mut rows.files));
            }
        }
    }

    fn prepare(&mut self, source: &Source, formats: &[String]) -> Result<PreparedImport, String> {
        match source {
            Source::Folder {
                path,
                include_hidden,
                recurse,
                ..
            } => Ok(self.folder(path, *include_hidden, *recurse, formats)),
            Source::PlaylistFile { path } => self.playlist(path, formats),
            Source::Batch {
                paths,
                include_hidden,
                recurse,
            }
            | Source::Folders {
                paths,
                include_hidden,
                recurse,
            } => Ok(self.batch(paths, *include_hidden, *recurse, formats)),
        }
    }

    fn folder(
        &mut self,
        path: &Path,
        include_hidden: bool,
        recurse: bool,
        formats: &[String],
    ) -> PreparedImport {
        let (files, error) = photo::collect_reporting(
            path,
            include_hidden,
            recurse,
            formats,
            &self.active,
            self.found,
        );
        PreparedImport {
            files,
            read_error: error.map(|error| error.to_string()),
            ..PreparedImport::default()
        }
    }

    fn playlist(&mut self, path: &Path, formats: &[String]) -> Result<PreparedImport, String> {
        let mut rows = PreparedImport::default();
        self.add_playlist(&mut rows, path, formats, Gone::Missing)?;
        if let Some(error) = rows.read_error.take() {
            return Err(error);
        }
        Ok(rows)
    }

    fn add_playlist(
        &mut self,
        rows: &mut PreparedImport,
        path: &Path,
        formats: &[String],
        gone: Gone,
    ) -> Result<(), String> {
        let active = self.active;
        let result = rgpl::visit_file(path, active, &mut |file| {
            if !active() {
                return;
            }
            // Do not look up a path until the input type and remote policy allow it.
            if !safe_playlist_path(&file) || !photo::is_image_file(&file, formats) {
                rows.skipped += 1;
            } else if is_remote(&file) && !(self.remote)() {
                rows.remote_skipped += 1;
            } else if active() {
                push(rows, file, formats, gone);
            }
            self.flush(rows);
        });
        if let Err(error) = result {
            if rows.read_error.is_none() {
                rows.read_error = Some(error);
            }
        }
        Ok(())
    }

    fn batch(
        &mut self,
        paths: &[PathBuf],
        include_hidden: bool,
        recurse: bool,
        formats: &[String],
    ) -> PreparedImport {
        let mut rows = PreparedImport::default();
        for path in paths {
            if !(self.active)() {
                break;
            }
            if path.is_dir() {
                let (files, error) = photo::collect_reporting(
                    path,
                    include_hidden,
                    recurse,
                    formats,
                    &self.active,
                    self.found,
                );
                if let Some(error) = error {
                    rows.read_error.get_or_insert_with(|| error.to_string());
                }
                for file in files {
                    if !(self.active)() {
                        break;
                    }
                    rows.files.push(file);
                    self.flush(&mut rows);
                }
            } else if rgpl::is_playlist_file(path) {
                let _ = self.add_playlist(&mut rows, path, formats, Gone::Silent);
            } else {
                push(&mut rows, path.clone(), formats, Gone::Skipped);
                self.flush(&mut rows);
            }
        }
        rows
    }
}

fn deny_remote() -> bool {
    false
}

/// A folder's images; the enumeration already checked every entry.
pub fn folder(
    dir: &Path,
    include_hidden: bool,
    recurse: bool,
    formats: &[String],
) -> PreparedImport {
    Scan {
        active: &|| true,
        remote: &mut deny_remote,
        emit: None,
        found: &mut |_| {},
    }
    .folder(dir, include_hidden, recurse, formats)
}

/// A playlist's rows: gone files are counted missing, existing unusable rows skipped.
pub fn playlist(path: &Path, formats: &[String]) -> Result<PreparedImport, String> {
    Scan {
        active: &|| true,
        remote: &mut deny_remote,
        emit: None,
        found: &mut |_| {},
    }
    .playlist(path, formats)
}

/// Candidates checked as they are, without expanding folders: a gone path is skipped,
/// the way a direct add has always reported it.
pub fn paths(paths: &[PathBuf], formats: &[String]) -> PreparedImport {
    let mut rows = PreparedImport::default();
    for path in paths {
        push(&mut rows, path.clone(), formats, Gone::Skipped);
    }
    rows
}

/// A drop batch: folder images, playlist rows (gone ones silently dropped, as always),
/// other paths kept as they are.
pub fn batch(
    paths: &[PathBuf],
    include_hidden: bool,
    recurse: bool,
    formats: &[String],
) -> PreparedImport {
    Scan {
        active: &|| true,
        remote: &mut deny_remote,
        emit: None,
        found: &mut |_| {},
    }
    .batch(paths, include_hidden, recurse, formats)
}

/// How a path that vanished between listing and checking is accounted.
#[derive(Clone, Copy)]
enum Gone {
    /// A playlist row that went: counted missing, so the caller warns.
    Missing,
    /// A directly named path that went: counted skipped, so the caller reports it.
    Skipped,
    /// A dropped playlist's row that went: stays silent, as it always has.
    Silent,
}

fn push(rows: &mut PreparedImport, path: PathBuf, formats: &[String], gone: Gone) {
    match path.metadata() {
        Ok(metadata) if metadata.is_file() && photo::is_image_file(&path, formats) => {
            rows.files.push(path);
        }
        Err(_) => match gone {
            Gone::Missing => rows.missing += 1,
            Gone::Skipped => rows.skipped += 1,
            Gone::Silent => {}
        },
        _ => rows.skipped += 1,
    }
}

const IMPORT_BATCH: usize = 256;
