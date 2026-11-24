use super::*;

impl State {
    pub(super) fn apply_folder_rows(
        &mut self,
        tag: u64,
        replacing: bool,
        rows: Vec<(PathBuf, photo::FileFacts)>,
    ) {
        if self.import_preview.is_none() {
            if replacing {
                self.photos = PhotoList::empty();
                crate::playlist::reset_metadata();
            }
            self.import_preview = Some((tag, self.photos.len()));
        }
        let first = self.photos.is_empty();
        let facts = rows
            .iter()
            .map(|(path, facts)| (path.clone(), *facts))
            .collect();
        let start = self.photos.len();
        self.photos
            .extend_files(rows.into_iter().map(|(path, _)| path).collect());
        crate::playlist::seed_metadata(&self.photos, facts);
        if let Some(wanted) = &self.startup_select {
            if let Some(offset) = self.photos.files[start..]
                .iter()
                .position(|path| path == wanted)
            {
                self.photos.index = start + offset;
                self.load_current();
            }
        } else if first {
            self.load_current();
        }
    }

    pub(super) fn finish_folder(&mut self, order: Vec<usize>, seed: Option<FolderSeed>) {
        let wanted = self.startup_select.take();
        let start_slideshow = std::mem::take(&mut self.startup_slideshow);
        let Some((_, start)) = self.import_preview.take() else {
            return;
        };
        let order = self.reconcile_direct_seed(start, order, seed);
        let selected = wanted.as_ref().and_then(|path| {
            self.photos
                .files
                .iter()
                .position(|file| file == path)
                .or_else(|| order.first().copied())
        });
        let previous = self.photos.file_index(self.photos.index);
        self.photos.finish_folder_order(start, order, selected);
        if wanted.is_some() && (self.frame().is_none() || selected != previous) {
            self.load_current();
        }
        if start_slideshow {
            self.start_slideshow();
        }
    }

    /// A directly opened image started as row zero while its folder was scanned. Its real
    /// occurrence arrives with the scan, so drop the provisional row and keep the user's
    /// current occurrence in the scanner's order. If the filter excluded it,
    /// the scanner provides its natural position.
    fn reconcile_direct_seed(
        &mut self,
        start: usize,
        order: Vec<usize>,
        placement: Option<FolderSeed>,
    ) -> Vec<usize> {
        let Some(seed) = self.direct_seed.take() else {
            return order;
        };
        if start != 0 || self.photos.files.first() != Some(&seed) {
            return order;
        }
        let discovered = match placement {
            Some(FolderSeed::Discovered(row)) => Some(row),
            _ => self.photos.files[1..].iter().position(|path| path == &seed),
        };
        match discovered {
            Some(duplicate) => {
                self.photos.files.remove(0);
                self.photos.index = if self.photos.index == 0 {
                    duplicate
                } else {
                    self.photos.index - 1
                };
                order
            }
            None => {
                let mut order: Vec<usize> = order.into_iter().map(|row| row + 1).collect();
                let position = match placement {
                    Some(FolderSeed::InsertAt(position)) => position,
                    _ => 0,
                };
                order.insert(position, 0);
                order
            }
        }
    }

    pub fn take_remote_import(&mut self) -> Option<std::sync::mpsc::SyncSender<bool>> {
        self.remote_import.take()
    }

    pub(super) fn finish_import_files(&mut self, rows: &mut PreparedImport) {
        if !self.import_rows.is_empty() {
            self.import_rows.append(&mut rows.files);
            rows.files = std::mem::take(&mut self.import_rows);
        }
    }

    pub(super) fn finish_append(&mut self, tag: u64, rows: PreparedImport) {
        let selected =
            if let Some((_, start)) = self.import_preview.take_if(|(owner, _)| *owner == tag) {
                let selected = (self.photos.index >= start)
                    .then(|| self.photos.current().cloned())
                    .flatten();
                self.photos
                    .remove_rows(&(start..self.photos.len()).collect::<Vec<_>>());
                selected.map(|path| (start, path))
            } else {
                None
            };
        self.append_rows(rows);
        if let Some((start, path)) = selected {
            let row =
                (start..self.photos.len()).find(|&row| self.photos.path_at(row) == Some(&path));
            if let Some(row) = row {
                self.photos.jump_to(row);
                self.load_current();
            }
        }
    }

    /// The latest replacement's rows: folders, playlists and batches all arrive the same
    /// way, with the caller-chosen position when a command-line file started the scan.
    pub(super) fn apply_replaced(&mut self, rows: PreparedImport) {
        let playlist = self.open_playlist_path.take();
        let PreparedImport {
            files,
            skipped,
            missing,
            remote_skipped,
            read_error,
        } = rows;
        let empty_playlist =
            playlist.is_some() && skipped == 0 && missing == 0 && remote_skipped == 0;
        self.report_skipped(skipped);
        let preview = self.import_preview.take().is_some();
        let select = self.startup_select.take().or_else(|| {
            (preview && self.photos.index != 0)
                .then(|| self.photos.current().cloned())
                .flatten()
        });
        let start_slideshow = std::mem::take(&mut self.startup_slideshow);
        if !files.is_empty() || empty_playlist {
            let mut photos = PhotoList::empty();
            photos.files = files;
            if let Some(select) = select {
                if let Some(position) = photos.files.iter().position(|path| path == &select) {
                    photos.index = position;
                }
            }
            self.photos = photos;
            crate::playlist::reset_metadata();
            self.load_current();
            if let Some(path) = &playlist {
                if self.message.is_none() {
                    self.message = Some(format!(
                        "Opened {} images from {}",
                        self.photos.len(),
                        path.display()
                    ));
                }
            }
            if start_slideshow {
                self.start_slideshow();
            }
        }
        if missing != 0 {
            let warning = format!("Playlist skipped {missing} missing image(s).");
            self.notice = Some(match self.notice.take() {
                Some(other) => format!("{warning} {other}"),
                None => warning,
            });
        }
        self.report_remote_skipped(remote_skipped);
        self.report_import_error(read_error);
    }

    /// Shows `path` at once while its immediate folder is scanned, replacing the playlist.
    /// The row is provisional: the scan's real occurrence replaces it, in natural order.
    pub fn open_direct_image(&mut self, path: &Path) -> bool {
        let absolute = std::path::absolute(path).ok();
        let path = absolute.as_deref().unwrap_or(path);
        if self.worker.is_none() {
            self.photos = PhotoList::from_path(
                path,
                self.knobs.include_hidden,
                false,
                photo::image_formats(),
            );
            self.load_current();
            return true;
        }
        let folder = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let submitted = self.submit_import(
            Source::Folder {
                path: folder,
                include_hidden: self.knobs.include_hidden,
                recurse: false,
                seed: Some(path.to_path_buf()),
            },
            true,
        );
        if !submitted {
            return false;
        }
        let tag = self
            .pending_import
            .expect("the replacement was just submitted");
        let path = path.to_path_buf();
        let mut photos = PhotoList::empty();
        photos.files.push(path.clone());
        self.photos = photos;
        self.import_preview = Some((tag, 0));
        self.direct_seed = Some(path);
        self.startup_select = None;
        crate::playlist::reset_metadata();
        self.load_current();
        true
    }
}

#[cfg(test)]
mod tests;
