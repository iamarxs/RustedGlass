use super::worker::OpenedOutcome;
use super::*;

impl State {
    pub(super) fn apply_opened(
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
            OpenedOutcome::Still { image, eligible } => {
                self.animated_selection = false;
                if let Some(path) = self.photos.current().cloned() {
                    // The decode lane validated the file after the decode; storing takes
                    // its stat and no further stat of our own.
                    let validated = eligible.then_some(identity).flatten();
                    self.bitmap = Some(crate::cache::store_validated(&path, validated, image));
                }
            }
            OpenedOutcome::Cached => {
                if let Some(path) = self.photos.current().cloned() {
                    match identity.and_then(|stat| crate::cache::hit(&path, stat)) {
                        Some(bitmap) => self.bitmap = Some(bitmap),
                        // The predicted hit was evicted meanwhile: reopen through the
                        // worker, which validates the file before decoding for real.
                        None => {
                            match self.worker.as_ref().map(|worker| {
                                worker.open(path.clone(), crate::cache::candidate(&path))
                            }) {
                                Some(Ok(generation)) => {
                                    self.selection = generation;
                                    self.load_pending = true;
                                }
                                Some(Err(_)) => {
                                    self.report_unreadable(&path, "The decoder is unavailable.")
                                }
                                None => {}
                            }
                        }
                    }
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
        if self.bitmap.is_none() && !self.load_pending {
            self.message = Some(
                "Drop files or a folder here. Press O to open files or F to open a folder.".into(),
            );
        }
        true
    }
}
