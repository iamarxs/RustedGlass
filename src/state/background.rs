//! Draining the background lanes (see `state::worker`): finished results are applied to
//! the state here, on the UI thread, in the order they finished. Every decode result
//! carries the generation of the selection that asked for it, every import result the
//! caller's tag, so anything older than what the state already holds — or anything that
//! belongs to a selection that is no longer showing — is dropped without touching the
//! visible picture or the playlist.

use std::time::{Duration, Instant};

use super::worker::{Event, FolderSeed, PreparedImport, Worker};
use super::*;

const IMPORT_NOTICE_DURATION: Duration = Duration::from_secs(3);

impl State {
    /// Drains finished background work without waiting. Returns `true` when the picture,
    /// the playlist or a notice changed and the caller should repaint.
    #[cfg(test)]
    pub fn poll_background(&mut self) -> bool {
        self.poll_background_budget(usize::MAX)
    }

    pub fn poll_background_budget(&mut self, max: usize) -> bool {
        let mut changed = false;
        for _ in 0..max {
            let Some(event) = self.worker.as_ref().and_then(Worker::next_event) else {
                break;
            };
            changed |= self.apply_event(event);
        }
        // The fallback timer drains even while a slow scan has no results to deliver.
        changed |= self
            .import_notice_until
            .take_if(|deadline| Instant::now() >= *deadline)
            .is_some();
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
                self.pending_import = Some(tag);
                self.import_notice_until = Some(Instant::now() + IMPORT_NOTICE_DURATION);
                if replace {
                    self.folder_retry = None;
                    self.playlist_undo = None;
                    self.replace_tag = tag;
                    self.import_preview = None;
                    self.import_rows = Vec::new();
                    self.remote_import = None;
                    self.inspection = None;
                    self.open_playlist_path = None;
                    self.startup_select = None;
                    self.direct_seed = None;
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

    pub fn importing(&self) -> bool {
        self.pending_import.is_some()
    }

    pub fn show_import_notice(&self) -> bool {
        self.import_notice_until.is_some()
    }

    pub fn browsing_import_preview(&self) -> bool {
        self.import_preview.is_some() || self.retrying_folder_scan()
    }

    fn import_finished(&mut self, tag: u64) {
        if self.pending_import == Some(tag) {
            self.pending_import = None;
            self.import_notice_until = None;
        }
    }

    /// A reply of `generation` is accepted only while it is still the current selection —
    /// never for an outdated one.
    fn accepts(&self, generation: u64) -> bool {
        !self.navigation_pending && generation == self.selection
    }

    fn apply_event(&mut self, event: Event) -> bool {
        match event {
            Event::FolderRows {
                tag,
                replacing,
                rows,
            } => {
                if tag < self.replace_tag {
                    return false;
                }
                if let Err(rows) = self.stage_retry_rows(tag, rows) {
                    self.apply_folder_rows(tag, replacing, rows);
                    true
                } else {
                    false
                }
            }
            Event::FolderFinished {
                tag,
                order,
                seed,
                failure,
            } => {
                if tag < self.replace_tag {
                    return false;
                }
                self.import_finished(tag);
                self.complete_folder_scan(tag, order, seed, failure);
                true
            }
            Event::Deduplicated {
                replace_tag,
                revision,
                rows,
            } => self.apply_deduplication(replace_tag, revision, &rows),
            Event::ImportPreview {
                tag,
                replacing,
                files,
            } => {
                if tag < self.replace_tag
                    || (replacing && self.startup_select.is_some())
                    || files.is_empty()
                {
                    return false;
                }
                if self.import_preview.is_none() {
                    if replacing {
                        self.photos = PhotoList::empty();
                    }
                    self.import_preview = Some((tag, self.photos.len()));
                }
                let was_empty = self.photos.is_empty();
                self.photos.extend_files(files);
                if was_empty {
                    self.load_current();
                }
                true
            }
            Event::ImportRows { tag, files } => {
                if tag >= self.replace_tag {
                    self.import_rows.extend(files);
                }
                false
            }
            Event::RemoteImport { tag, respond } => {
                if tag >= self.replace_tag {
                    self.remote_import = Some(respond);
                }
                false
            }
            Event::Inspected {
                revision,
                path,
                text,
            } => self.finish_image_info(revision, &path, text),
            Event::Metadata {
                revision,
                entries,
                complete,
            } => crate::playlist::metadata_ready(revision, &mut self.photos, entries, complete),
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
            Event::Appended { tag, mut rows } => {
                if tag < self.replace_tag {
                    return false;
                }
                self.import_finished(tag);
                self.finish_import_files(&mut rows);
                self.finish_append(tag, rows);
                true
            }
            Event::Replaced { tag, mut rows } => {
                if tag != self.replace_tag {
                    return false;
                }
                self.import_finished(tag);
                self.finish_import_files(&mut rows);
                self.apply_replaced(rows);
                true
            }
            Event::PrepFailed { tag, error } => {
                if tag < self.replace_tag {
                    return false;
                }
                self.import_finished(tag);
                self.import_rows.clear();
                if tag == self.replace_tag {
                    self.import_preview = None;
                    self.open_playlist_path = None;
                    self.startup_select = None;
                    self.startup_slideshow = false;
                }
                self.notice = Some(error);
                true
            }
        }
    }
}

mod dedup;
mod import;
mod metadata;
mod retry;
mod selection;
pub(super) use retry::FolderRetry;

#[cfg(test)]
mod tests;
