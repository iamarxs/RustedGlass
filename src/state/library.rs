use super::*;

impl State {
    pub(super) fn load_new_order(&mut self) {
        if self.knobs.random_order {
            self.photos.shuffle(random_seed());
        }
        self.load_current();
    }

    /// Writes the display order: by name, or shuffled, keeping the shown entry where it is.
    pub fn apply_order(&mut self) {
        let shown = self.photos.file_index(self.photos.index);
        if self.knobs.random_order {
            self.photos.shuffle(random_seed());
        } else {
            // Clearing the order returns to the names; `index` is put back below.
            self.photos.order.clear();
        }
        if let Some(index) = shown {
            self.photos.index = self
                .photos
                .position_of_file(index)
                .unwrap_or(self.photos.index);
        }
    }

    /// Turns the random display order on or off, and re-orders the files. A new order starts at
    /// its first entry, which is what makes the shuffle visible. Starting a slideshow turns it on,
    /// as the "Shuffle" option of the original slideshow menu does.
    pub fn set_random_order(&mut self, enabled: bool) {
        self.knobs.random_order = enabled;
        self.config
            .set("EnableRandomDisplayOrder", Value::Bool(enabled));
        self.apply_order();
        if enabled {
            self.photos.index = 0;
            self.load_current();
        }
    }

    /// Replace the playlist with supported paths from the picker or drop.
    pub fn open_files(&mut self, paths: &[PathBuf]) {
        let files = self.filter_image_types(paths.to_vec());
        if !files.is_empty() {
            self.photos = PhotoList::from_files(&files, photo::image_formats());
            self.load_new_order();
        }
    }

    pub fn open_folder(&mut self, dir: &Path) {
        let files = self.folder_images(dir);
        self.notice = None;
        if !files.is_empty() {
            self.photos = PhotoList::empty();
            self.photos.files = files;
            self.load_new_order();
        }
    }

    /// Append supported paths, keeping intentional duplicate entries.
    pub fn add_files(&mut self, paths: &[PathBuf]) -> usize {
        let files = self.filter_image_types(paths.to_vec());
        let was_empty = self.photos.is_empty();
        let added = self.photos.add_files(&files);
        if added > 0 && was_empty {
            self.load_new_order();
            return self.photos.len();
        }
        added
    }

    pub fn add_folder(&mut self, dir: &Path) -> usize {
        self.add_files(&self.folder_images(dir))
    }

    pub fn append_drop(&mut self, paths: Vec<PathBuf>) {
        let mut files = Vec::new();
        for path in paths {
            if path.is_dir() {
                files.extend(self.folder_images(&path));
            } else {
                files.push(path);
            }
        }
        self.add_files(&files);
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
            self.load_new_order();
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

    /// Sorts the rows by name; `descending` reverses them. Sorting turns the random order off in
    /// the config as well, otherwise the next pass of a slideshow would shuffle the rows again.
    pub fn sort_by_name(&mut self, descending: bool) {
        self.config
            .set("EnableRandomDisplayOrder", Value::Bool(false));
        self.reload_knobs();
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

    /// Applies the rename or copy the prompt asked for, and keeps the playlist in step.
    pub fn apply_pending(&mut self, text: &str) -> Result<PathBuf, String> {
        let (pending, path) = self.pending.as_ref().ok_or("No file operation is open")?;
        if self.navigation_pending || self.photos.current() != Some(path) {
            return Err("Image selection changed. Open the file command again.".into());
        }
        let target = pending.apply(path, text)?;

        match pending {
            Pending::Rename => {
                for entry in &mut self.photos.files {
                    if entry == path {
                        entry.clone_from(&target);
                    }
                }
            }
            Pending::CopyTo(_) => {
                self.add_files(std::slice::from_ref(&target));
            }
        }

        self.pending = None;
        self.message = None;
        Ok(target)
    }
}
