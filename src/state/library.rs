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

    fn append_rows(&mut self, rows: PreparedImport) -> usize {
        let was_empty = self.photos.is_empty();
        let added = self.photos.extend_files(rows.files);
        self.report_skipped(rows.skipped);
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
        let original_count = rows.files.len() + rows.skipped + rows.missing;
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
        Ok(if loaded { self.photos.len() } else { 0 })
    }

    /// Writes the rows, in the order they are shown, to a `.rgpl` file.
    pub fn save_playlist(&mut self, path: &Path) -> Result<usize, String> {
        let count = rgpl::write_file(path, self.photos.visible_rows())?;
        self.message = Some(format!("Saved {} rows to {}", count, path.display()));
        Ok(count)
    }

    /// Sorts the playlist rows by name; `descending` reverses them.
    pub fn sort_by_name(&mut self, descending: bool) {
        self.photos.sort_by_name(descending);
    }

    /// Takes the shown entry out of the playlist, leaving the file on disk.
    pub fn remove_current(&mut self) -> bool {
        if self.photos.remove_current().is_none() {
            return false;
        }
        self.load_current();
        true
    }

    /// Deletes the shown file, then takes its entry out of the playlist.
    pub fn delete_current(&mut self) -> Result<PathBuf, String> {
        let path = self.photos.current().cloned().ok_or("No image selected")?;
        fileops::delete(&path)?;
        self.photos.remove_matching(|entry| entry == path);
        self.load_current();
        Ok(path)
    }

    /// Takes the listed rows out of the playlist, leaving the files on disk. The shown picture
    /// stays shown when its row is not one of them.
    pub fn remove_many(&mut self, rows: &[usize]) {
        self.photos.remove_rows(rows);
        self.load_current();
    }

    /// Sends the files of the listed rows to the recycle bin and drops their entries. A file that
    /// could not be sent is reported, and the rest still go; its rows stay.
    pub fn delete_many(&mut self, rows: &[usize]) -> Option<String> {
        let mut failed = None;
        let mut failure_count = 0;
        let mut attempts = std::collections::HashMap::new();
        for row in sorted_desc(rows) {
            if let Some(path) = self.photos.path_at(row) {
                attempts
                    .entry(path.clone())
                    .or_insert_with(|| match fileops::delete(path) {
                        Ok(()) => true,
                        Err(error) => {
                            failure_count += 1;
                            failed.get_or_insert(error);
                            false
                        }
                    });
            }
        }
        self.photos
            .remove_matching(|path| attempts.get(path) == Some(&true));
        self.load_current();
        failed.map(|error| {
            format!(
                "Could not recycle {failure_count} of {} files. {error}",
                attempts.len()
            )
        })
    }

    /// Moves the shown file into the folder of the `MoveToFolder` setting, and drops its entry.
    pub fn move_current_to_target(&mut self) -> Result<PathBuf, String> {
        if self.knobs.move_to.as_os_str().is_empty() {
            return Err(String::from(
                "No 'Move to' folder is set. Open the settings (S) and pick one.",
            ));
        }
        let path = self.photos.current().cloned().ok_or("No image selected")?;
        let target = fileops::move_to(&path, &self.knobs.move_to)?;
        self.photos.remove_matching(|entry| entry == path);
        self.load_current();
        self.message = Some(format!("Moved to {}", target.display()));
        Ok(target)
    }

    /// Renames the image for which the prompt was opened.
    pub fn apply_pending(&mut self, text: &str) -> Result<PathBuf, String> {
        let path = self.pending.as_ref().ok_or("No rename is open")?;
        if self.navigation_pending || self.load_pending || self.photos.current() != Some(path) {
            return Err("Image selection changed. Open the file command again.".into());
        }
        let target = fileops::rename(path, text)?;
        self.photos.replace_path(path, &target);

        self.pending = None;
        self.message = None;
        Ok(target)
    }
}
