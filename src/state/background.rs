//! Draining the background lanes (see `state::worker`): finished results are applied to
//! the state here, on the UI thread, in the order they finished. Every decode result
//! carries the generation of the selection that asked for it, every import result the
//! caller's tag, so anything older than what the state already holds — or anything that
//! belongs to a selection that is no longer showing — is dropped without touching the
//! visible picture or the playlist.

use super::worker::{Event, OpenedOutcome, PreparedImport, Worker};
use super::*;

impl State {
    /// Drains finished background work without waiting. Returns `true` when the picture,
    /// the playlist or a notice changed and the caller should repaint.
    pub fn poll_background(&mut self) -> bool {
        let mut changed = false;
        while let Some(event) = self.worker.as_ref().and_then(Worker::next_event) {
            changed |= self.apply_event(event);
        }
        changed
    }

    /// Submits a folder/playlist/batch preparation to the import lane. The result's tag
    /// comes back with it; a full lane is reported, never silent.
    pub fn submit_import(&mut self, source: Source, replace: bool) -> bool {
        self.prep_tag += 1;
        let tag = self.prep_tag;
        let result = self.worker.as_ref().map(|worker| {
            if replace {
                worker.replace(tag, source)
            } else {
                worker.append(tag, source)
            }
        });
        match result {
            Some(Ok(())) => {
                if replace {
                    self.replace_tag = tag;
                    self.inspection = None;
                    self.open_playlist_path = None;
                    self.startup_select = None;
                    self.startup_slideshow = false;
                }
                true
            }
            Some(Err(worker::Rejected::Busy)) => {
                self.notice = Some(String::from(
                    "Import queue is busy; the files were not added. Try again in a moment.",
                ));
                false
            }
            Some(Err(worker::Rejected::Disconnected)) => {
                self.notice = Some("Image import is unavailable.".into());
                false
            }
            None => false,
        }
    }

    /// Whether the current selection is an animation, whether it plays from the sync
    /// cursor or on the decode thread.
    pub fn is_animated(&self) -> bool {
        self.animation.is_some() || self.animated_selection
    }

    /// A reply of `generation` is accepted only while it is still the current selection —
    /// never for an outdated one.
    fn accepts(&self, generation: u64) -> bool {
        !self.navigation_pending && generation == self.selection
    }

    fn apply_event(&mut self, event: Event) -> bool {
        match event {
            Event::Inspected {
                revision,
                path,
                text,
            } => self.finish_image_info(revision, &path, text),
            Event::Opened {
                generation,
                identity,
                outcome,
            } => self.apply_opened(generation, identity, outcome),
            Event::Frame { generation, frame } => {
                if !self.accepts(generation) || !self.frame_pending {
                    return false;
                }
                self.frame_pending = false;
                self.show_frame(frame);
                true
            }
            Event::FrameEnded { generation } => {
                if !self.accepts(generation) || !self.frame_pending {
                    return false;
                }
                self.frame_pending = false;
                self.animation_played_once = true;
                if !self.slideshow_elapsed {
                    self.rewind_animation();
                }
                true
            }
            Event::FrameFailed { generation } => {
                if !self.accepts(generation) || !self.frame_pending {
                    return false;
                }
                // The stream is gone on the decode thread; the last good frame stays and
                // the file is not reopened by itself.
                self.stop_animation();
                true
            }
            Event::Appended { tag, rows } => {
                if tag < self.replace_tag {
                    return false;
                }
                let was_empty = self.photos.is_empty();
                let added = self.photos.extend_files(rows.files);
                self.report_skipped(rows.skipped);
                if was_empty && added != 0 {
                    self.load_current();
                }
                true
            }
            Event::Replaced { tag, rows } => {
                if tag != self.replace_tag {
                    return false;
                }
                self.apply_replaced(rows);
                true
            }
            Event::PrepFailed { tag, error } => {
                if tag < self.replace_tag {
                    return false;
                }
                if tag == self.replace_tag {
                    self.open_playlist_path = None;
                    self.startup_select = None;
                    self.startup_slideshow = false;
                }
                self.notice = Some(error);
                true
            }
        }
    }

    fn apply_opened(
        &mut self,
        generation: u64,
        identity: Option<crate::cache::FileStat>,
        outcome: OpenedOutcome,
    ) -> bool {
        if !self.accepts(generation) || !self.load_pending {
            return false;
        }
        self.load_pending = false;
        self.file_size = identity.map(|(len, _)| len);
        match outcome {
            OpenedOutcome::Still { image } => {
                self.animated_selection = false;
                if let Some(path) = self.photos.current().cloned() {
                    self.bitmap = Some(crate::cache::store_checked(&path, identity, image));
                }
            }
            OpenedOutcome::AnimatedFirst { frame } => {
                self.animated_selection = true;
                self.frame_pending = false;
                self.show_frame(frame);
            }
            OpenedOutcome::Failed { error } => {
                self.animated_selection = false;
                if let Some(path) = self.photos.current().cloned() {
                    self.report_unreadable(&path, &error);
                }
            }
        }
        if self.bitmap.is_none() {
            self.message = Some(
                "Drop files or a folder here. Press O to open files or F to open a folder.".into(),
            );
        }
        true
    }

    /// The latest replacement's rows: folders, playlists and batches all arrive the same
    /// way, with the caller-chosen position when a command-line file started the scan.
    fn apply_replaced(&mut self, rows: PreparedImport) {
        let playlist = self.open_playlist_path.take();
        let PreparedImport {
            files,
            skipped,
            missing,
        } = rows;
        let empty_playlist = playlist.is_some() && skipped == 0 && missing == 0;
        self.report_skipped(skipped);
        let select = self.startup_select.take();
        let start_slideshow = std::mem::take(&mut self.startup_slideshow);
        if !files.is_empty() || empty_playlist {
            let mut photos = PhotoList::empty();
            photos.files = files;
            if let Some(select) = select {
                if let Some(position) = photos.files.iter().position(|file| *file == select) {
                    photos.index = position;
                }
            }
            self.photos = photos;
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
    }
}

#[cfg(test)]
mod tests;
