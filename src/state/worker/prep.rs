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

/// A folder or `.rgpl` file to turn into a list of image paths.
pub enum Source {
    Folder {
        path: PathBuf,
        include_hidden: bool,
        recurse: bool,
    },
    /// Entries come back as listed, missing ones included, the way `rgpl::read_file`
    /// always has; the caller reports them.
    PlaylistFile { path: PathBuf },
    /// A dropped batch: folders expand, playlist files load their rows, other paths stay.
    Batch {
        paths: Vec<PathBuf>,
        include_hidden: bool,
        recurse: bool,
    },
}

/// A request to the preparation lane.
pub(super) enum PrepRequest {
    /// The prepared files append to the playlist.
    Append {
        tag: u64,
        source: Source,
    },
    /// The prepared files replace the whole playlist.
    Replace {
        tag: u64,
        source: Source,
    },
    Inspect {
        revision: u64,
        path: PathBuf,
    },
}

/// Prepared rows and how many candidates were dropped, counted the way the synchronous
/// methods have always counted them.
#[derive(Default)]
pub struct PreparedImport {
    pub files: Vec<PathBuf>,
    pub skipped: usize,
    pub missing: usize,
}

/// Prepares `source`; an unreadable playlist is the caller's error to report.
pub fn prepare(source: &Source, formats: &[String]) -> Result<PreparedImport, String> {
    match source {
        Source::Folder {
            path,
            include_hidden,
            recurse,
        } => Ok(folder(path, *include_hidden, *recurse, formats)),
        Source::PlaylistFile { path } => playlist(path, formats),
        Source::Batch {
            paths,
            include_hidden,
            recurse,
        } => Ok(batch(paths, *include_hidden, *recurse, formats)),
    }
}

/// A folder's images; the enumeration already checked every entry.
pub fn folder(
    dir: &Path,
    include_hidden: bool,
    recurse: bool,
    formats: &[String],
) -> PreparedImport {
    PreparedImport {
        files: photo::collect(dir, include_hidden, recurse, formats),
        skipped: 0,
        missing: 0,
    }
}

/// A playlist's rows: gone files are counted missing, existing unusable rows skipped.
pub fn playlist(path: &Path, formats: &[String]) -> Result<PreparedImport, String> {
    let mut rows = PreparedImport::default();
    for file in rgpl::read_file(path)? {
        push(&mut rows, file, formats, Gone::Missing);
    }
    Ok(rows)
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
    let mut rows = PreparedImport::default();
    for path in paths {
        if path.is_dir() {
            rows.files
                .extend(photo::collect(path, include_hidden, recurse, formats));
        } else if rgpl::is_playlist_file(path) {
            if let Ok(entries) = rgpl::read_file(path) {
                for entry in entries {
                    push(&mut rows, entry, formats, Gone::Silent);
                }
            }
        } else {
            push(&mut rows, path.clone(), formats, Gone::Skipped);
        }
    }
    rows
}

/// How a path that vanished between listing and checking is accounted.
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

/// The preparation lane: one source at a time, in the order requested.
pub(super) fn lane(
    requests: &Lane<PrepRequest>,
    events: &Lane<Event>,
    formats: &[String],
    wake: &Wake,
) {
    while let Some(request) = requests.pop() {
        let (tag, replacing, source) = match request {
            PrepRequest::Append { tag, source } => (tag, false, source),
            PrepRequest::Replace { tag, source } => (tag, true, source),
            PrepRequest::Inspect { revision, path } => {
                let text = crate::metadata::describe(&path);
                wake.deliver(
                    events,
                    Event::Inspected {
                        revision,
                        path,
                        text,
                    },
                );
                continue;
            }
        };
        let event = match prepare(&source, formats) {
            Ok(mut rows) => {
                // A replacing batch arrives in Explorer's name order, as the synchronous
                // picker's `PhotoList::from_files` does.
                if replacing && matches!(source, Source::Batch { .. }) {
                    let mut photos = photo::PhotoList::empty();
                    photos.files = rows.files;
                    photos.sort_by_name(false);
                    rows.files = photos.files;
                }
                if replacing {
                    Event::Replaced { tag, rows }
                } else {
                    Event::Appended { tag, rows }
                }
            }
            Err(error) => Event::PrepFailed { tag, error },
        };
        wake.deliver(events, event);
    }
}
