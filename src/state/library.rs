use super::*;

impl State {
    /// Changes how the next image is picked while preserving the current image and playlist.
    pub fn set_random_order(&mut self, enabled: bool) {
        self.knobs.random_order = enabled;
        self.config
            .set("EnableRandomDisplayOrder", Value::Bool(enabled));
        self.photos.reset_random_pass();
    }

    /// A drop expands folders to their images and playlist files to their rows (each playlist
    /// loads, so dropping several adds every list); other paths stay as they are.
    fn expand_paths(&self, paths: Vec<PathBuf>) -> Vec<PathBuf> {
        let mut files = Vec::new();
        for path in paths {
            if path.is_dir() {
                files.extend(self.folder_images(&path));
            } else if rgpl::is_playlist_file(&path) {
                if let Ok(entries) = rgpl::read_file(&path) {
                    files.extend(entries.into_iter().filter(|entry| entry.exists()));
                }
            } else {
                files.push(path);
            }
        }
        files
    }

    /// Replace the playlist with supported paths from the picker or drop.
    pub fn open_files(&mut self, paths: &[PathBuf]) {
        let files = self.filter_image_types(self.expand_paths(paths.to_vec()));
        if !files.is_empty() {
            self.photos = PhotoList::from_files(&files, photo::image_formats());
            self.load_current();
        }
    }

    pub fn open_folder(&mut self, dir: &Path) {
        let files = self.folder_images(dir);
        self.notice = None;
        if !files.is_empty() {
            self.photos = PhotoList::empty();
            self.photos.files = files;
            self.load_current();
        }
    }

    /// Append supported paths, keeping intentional duplicate entries.
    pub fn add_files(&mut self, paths: &[PathBuf]) -> usize {
        let files = self.filter_image_types(paths.to_vec());
        let was_empty = self.photos.is_empty();
        let added = self.photos.add_files(&files);
        if added > 0 && was_empty {
            self.load_current();
            return self.photos.len();
        }
        added
    }

    pub fn add_folder(&mut self, dir: &Path) -> usize {
        self.add_files(&self.folder_images(dir))
    }

    pub fn append_drop(&mut self, paths: Vec<PathBuf>) {
        self.add_files(&self.expand_paths(paths));
    }

    fn folder_images(&self, dir: &Path) -> Vec<PathBuf> {
        photo::collect(
            dir,
            self.knobs.include_hidden,
            self.knobs.recurse,
            &self.photos.formats,
        )
    }

    fn filter_image_types(&mut self, mut files: Vec<PathBuf>) -> Vec<PathBuf> {
        let count = files.len();
        files.retain(|path| photo::is_image_file(path, &self.photos.formats));
        let skipped = count - files.len();
        self.notice = (skipped != 0).then(|| format!("Skipped {skipped} unsupported image(s)."));
        files
    }

    /// Read paths in playlist order and report missing entries together.
    pub fn open_playlist(&mut self, path: &Path) -> Result<usize, String> {
        let mut files = rgpl::read_file(path)?;
        let original_count = files.len();
        files.retain(|file| file.exists());
        let missing = original_count - files.len();
        let files = self.filter_image_types(files);
        let loaded = !files.is_empty() || original_count == 0;
        if loaded {
            self.photos = PhotoList::empty();
            self.photos.files = files;
            self.load_current();
            if self.message.is_none() {
                self.message = Some(format!(
                    "Opened {} images from {}",
                    self.photos.len(),
                    path.display()
                ));
            }
        }
        if missing != 0 {
            let warning = format!("Playlist skipped {missing} missing image(s).");
            self.notice = Some(match self.notice.take() {
                Some(other) => format!("{warning} {other}"),
                None => warning,
            });
        }
        Ok(if loaded { self.photos.len() } else { 0 })
    }

    /// Writes the rows, in the order they are shown, to a `.rgpl` file.
    pub fn save_playlist(&mut self, path: &Path) -> Result<usize, String> {
        let files: Vec<PathBuf> = self.photos.visible().into_iter().cloned().collect();
        let count = rgpl::write_file(path, &files)?;
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
        for row in sorted_desc(rows) {
            self.photos.remove_at(row);
        }
        self.load_current();
    }

    /// Sends the files of the listed rows to the recycle bin and drops their entries. A file that
    /// could not be sent is reported, and the rest still go.
    pub fn delete_many(&mut self, rows: &[usize]) -> Option<String> {
        let mut failed = None;
        let mut attempts = std::collections::HashMap::new();
        for row in sorted_desc(rows) {
            if let Some(path) = self.photos.path_at(row) {
                attempts
                    .entry(path.clone())
                    .or_insert_with(|| match fileops::delete(path) {
                        Ok(()) => true,
                        Err(error) => {
                            failed = Some(error);
                            false
                        }
                    });
            }
        }
        self.photos
            .remove_matching(|path| attempts.get(path) == Some(&true));
        self.load_current();
        failed
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
        if self.navigation_pending || self.photos.current() != Some(path) {
            return Err("Image selection changed. Open the file command again.".into());
        }
        let target = fileops::rename(path, text)?;
        for entry in &mut self.photos.files {
            if entry == path {
                entry.clone_from(&target);
            }
        }

        self.pending = None;
        self.message = None;
        Ok(target)
    }
}
