use super::*;

impl Worker {
    pub fn prioritize_selection(&self, loading: bool) {
        self.selected_loading.store(loading, Ordering::Release);
    }

    /// Reads one batch, yielding between files when selected-image I/O takes priority.
    pub fn metadata(
        &self,
        revision: u64,
        paths: Vec<(PathBuf, Option<crate::photo::FileFacts>)>,
        read_header: bool,
        offscreen: bool,
    ) -> Result<(), Rejected> {
        self.prep.push(PrepRequest::Metadata {
            revision,
            paths,
            read_header,
            background_header: read_header && offscreen,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pending_selection_preempts_only_offscreen_header_work() {
        let worker = Worker::new(&crate::photo::image_formats());
        let request = vec![(PathBuf::from("metadata.png"), None)];
        worker.prioritize_selection(true);
        worker.metadata(1, request.clone(), true, true).unwrap();
        assert!(matches!(
            super::super::tests::next_event(&worker),
            Event::Metadata { revision: 1, entries, complete: false } if entries.is_empty()
        ));

        worker.metadata(2, request.clone(), true, false).unwrap();
        assert!(matches!(
            super::super::tests::next_event(&worker),
            Event::Metadata { revision: 2, entries, complete: true } if entries.len() == 1
        ));

        worker.prioritize_selection(false);
        worker.metadata(3, request, true, true).unwrap();
        assert!(matches!(
            super::super::tests::next_event(&worker),
            Event::Metadata { revision: 3, entries, complete: true } if entries.len() == 1
        ));
    }
}
