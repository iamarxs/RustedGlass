use super::*;

impl State {
    /// Also interrupts an outstanding offscreen batch at its next file boundary.
    pub fn prioritize_metadata_selection(&self) -> bool {
        let pending = self.load_pending || self.navigation_pending;
        if let Some(worker) = &self.worker {
            worker.prioritize_selection(pending);
        }
        pending
    }

    /// The caller retries a busy lane on its next refresh.
    pub fn request_metadata(
        &self,
        revision: u64,
        paths: Vec<(PathBuf, Option<photo::FileFacts>)>,
        read_header: bool,
        offscreen: bool,
    ) -> bool {
        !self.importing()
            && self.worker.as_ref().is_some_and(|worker| {
                worker
                    .metadata(revision, paths, read_header, offscreen)
                    .is_ok()
            })
    }
}
