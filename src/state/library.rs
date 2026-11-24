use super::worker::{prep, PreparedImport};
use super::*;

impl State {
    /// Changes how the next image is picked while preserving the current image and playlist.
    pub fn set_random_order(&mut self, enabled: bool) {
        self.knobs.random_order = enabled;
        self.config
            .set("EnableRandomDisplayOrder", Value::Bool(enabled));
        self.photos.reset_random_pass();
    }

    /// Replace the playlist with supported paths from the picker or drop.
    pub fn open_files(&mut self, paths: &[PathBuf]) {
        if self.worker.is_some() {
            if let [path] = paths {
                if photo::is_image_file(path, &self.photos.formats) && path.is_file() {
                    self.open_direct_image(path);
                    return;
                }
            }
            self.submit_import(
                Source::Batch {
                    paths: paths.to_vec(),
                    include_hidden: self.knobs.include_hidden,
                    recurse: self.knobs.recurse,
                },
                true,
            );
            return;
        }
        let rows = prep::batch(
            paths,
            self.knobs.include_hidden,
            self.knobs.recurse,
            &self.photos.formats,
        );
        // `from_files` sorts the rows by name, the order a replacing batch arrives in.
        let photos = PhotoList::from_files(&rows.files, photo::image_formats());
        self.report_skipped(rows.skipped);
        self.report_import_error(rows.read_error);
        if !photos.is_empty() {
            self.photos = photos;
            self.load_current();
        }
    }

    pub fn open_folder(&mut self, dir: &Path) {
        self.notice = None;
        if self.worker.is_some() {
            self.submit_import(
                Source::Folder {
                    path: dir.to_path_buf(),
                    include_hidden: self.knobs.include_hidden,
                    recurse: self.knobs.recurse,
                    seed: None,
                },
                true,
            );
            return;
        }
        let rows = prep::folder(
            dir,
            self.knobs.include_hidden,
            self.knobs.recurse,
            &self.photos.formats,
        );
        self.report_import_error(rows.read_error);
        if !rows.files.is_empty() {
            self.photos = PhotoList::empty();
            self.photos.files = rows.files;
            self.load_current();
        }
    }

    /// Append supported paths, keeping intentional duplicate entries.
    pub fn add_files(&mut self, paths: &[PathBuf]) -> usize {
        if self.worker.is_some() {
            return if self.submit_import(
                Source::Batch {
                    paths: paths.to_vec(),
                    include_hidden: self.knobs.include_hidden,
                    recurse: self.knobs.recurse,
                },
                false,
            ) {
                // The count arrives with the Appended event; batched drops report there.
                paths.len()
            } else {
                0
            };
        }
        let rows = prep::paths(paths, &self.photos.formats);
        self.append_rows(rows)
    }

    pub(super) fn append_rows(&mut self, rows: PreparedImport) -> usize {
        let was_empty = self.photos.is_empty();
        let added = self.photos.extend_files(rows.files);
        self.report_skipped(rows.skipped);
        self.report_remote_skipped(rows.remote_skipped);
        self.report_import_error(rows.read_error);
        if added > 0 && was_empty {
            self.load_current();
            return self.photos.len();
        }
        added
    }

    pub fn add_folder(&mut self, dir: &Path) -> usize {
        if self.worker.is_some() {
            self.submit_import(
                Source::Folder {
                    path: dir.to_path_buf(),
                    include_hidden: self.knobs.include_hidden,
                    recurse: self.knobs.recurse,
                    seed: None,
                },
                false,
            );
            return 0;
        }
        let rows = prep::folder(
            dir,
            self.knobs.include_hidden,
            self.knobs.recurse,
            &self.photos.formats,
        );
        self.append_rows(rows)
    }

    pub fn append_drop(&mut self, paths: Vec<PathBuf>) {
        if self.worker.is_some() {
            self.add_files(&paths);
            return;
        }
        let rows = prep::batch(
            &paths,
            self.knobs.include_hidden,
            self.knobs.recurse,
            &self.photos.formats,
        );
        self.append_rows(rows);
    }

    /// Reports how many import candidates were dropped.
    pub(super) fn report_skipped(&mut self, skipped: usize) {
        self.notice = (skipped != 0).then(|| format!("Skipped {skipped} unsupported image(s)."));
    }

    pub(super) fn report_import_error(&mut self, error: Option<String>) {
        if let Some(error) = error {
            self.notice = Some(match self.notice.take() {
                Some(other) => format!("{error} {other}"),
                None => error,
            });
        }
    }

    pub(super) fn report_remote_skipped(&mut self, skipped: usize) {
        if skipped != 0 {
            let message = format!("Skipped {skipped} image(s) on network shares.");
            self.notice = Some(match self.notice.take() {
                Some(other) => format!("{other} {message}"),
                None => message,
            });
        }
    }

    /// Read paths in playlist order and report missing entries together.
    pub fn open_playlist(&mut self, path: &Path) -> Result<usize, String> {
        if self.worker.is_some() {
            if self.submit_import(
                Source::PlaylistFile {
                    path: path.to_path_buf(),
                },
                true,
            ) {
                self.open_playlist_path = Some(path.to_path_buf());
                return Ok(0);
            }
            return Err(format!(
                "Could not queue the playlist at {}",
                path.display()
            ));
        }
        let rows = prep::playlist(path, &self.photos.formats)?;
        // Every row is a file, a missing path or a skipped one, so the original count is
        // their sum: an empty playlist still clears, an all-missing one leaves the rows.
        let original_count = rows.files.len() + rows.skipped + rows.missing + rows.remote_skipped;
        let loaded = !rows.files.is_empty() || original_count == 0;
        self.report_skipped(rows.skipped);
        if loaded {
            self.photos = PhotoList::empty();
            self.photos.files = rows.files;
            self.load_current();
            if self.message.is_none() {
                self.message = Some(format!(
                    "Opened {} images from {}",
                    self.photos.len(),
                    path.display()
                ));
            }
        }
        if rows.missing != 0 {
            let warning = format!("Playlist skipped {} missing image(s).", rows.missing);
            self.notice = Some(match self.notice.take() {
                Some(other) => format!("{warning} {other}"),
                None => warning,
            });
        }
        self.report_remote_skipped(rows.remote_skipped);
        Ok(if loaded { self.photos.len() } else { 0 })
    }

    /// Writes the rows, in the order they are shown, to a `.rgpl` file.
    pub fn save_playlist(&mut self, path: &Path) -> Result<usize, String> {
        let count = rgpl::write_file(path, self.photos.visible_rows())?;
        self.message = Some(format!("Saved {} rows to {}", count, path.display()));
        Ok(count)
    }

    /// Takes the shown entry out of the playlist, leaving the file on disk.
    pub fn remove_current(&mut self) -> bool {
        let undo = self.capture_playlist_edit();
        if self.photos.remove_current().is_none() {
            return false;
        }
        self.commit_playlist_edit(undo);
        self.load_current();
        true
    }

    /// Takes the listed rows out of the playlist, leaving the files on disk. The shown picture
    /// stays shown when its row is not one of them.
    pub fn remove_many(&mut self, rows: &[usize]) {
        let undo = self.capture_playlist_edit();
        if self.photos.remove_rows(rows) != 0 {
            self.commit_playlist_edit(undo);
            self.load_current();
        }
    }
}

mod files;
