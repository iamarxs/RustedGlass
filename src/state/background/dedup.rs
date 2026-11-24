use super::*;

impl State {
    pub fn request_remove_duplicates(&mut self) {
        let revision = self.photos.rows_token();
        let paths: Vec<_> = self.photos.visible_rows().cloned().collect();
        if let Some(worker) = &self.worker {
            if worker
                .deduplicate(self.replace_tag, revision, paths)
                .is_err()
            {
                self.notice = Some("Could not check playlist duplicates. Try again.".into());
            }
        } else {
            let rows = photo::duplicate_rows(paths, &|| true);
            self.apply_deduplication(self.replace_tag, revision, &rows);
        }
    }

    pub(super) fn apply_deduplication(
        &mut self,
        replace_tag: u64,
        revision: u64,
        rows: &[(usize, usize)],
    ) -> bool {
        if self.replace_tag != replace_tag || self.photos.rows_token() != revision {
            return false;
        }
        let removed = if rows.is_empty() {
            0
        } else {
            let undo = self.capture_playlist_edit();
            let removed = self.photos.apply_duplicate_rows(rows);
            self.commit_playlist_edit(undo);
            removed
        };
        self.notice = Some(match removed {
            0 => "No duplicate entries.".into(),
            1 => "Removed 1 duplicate entry.".into(),
            _ => format!("Removed {removed} duplicate entries."),
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_duplicate_reply_cannot_remove_rows_from_a_new_order_or_import() {
        let mut state = State::new(crate::test_path("unused-config.json"), &[]);
        state.photos.extend_files(
            ["z.png", "a.png", "z.png"]
                .into_iter()
                .map(PathBuf::from)
                .collect(),
        );
        let revision = state.photos.rows_token();
        state.photos.move_row(0, 1);
        assert!(!state.apply_deduplication(0, revision, &[(2, 0)]));
        let revision = state.photos.rows_token();
        state.replace_tag += 1;
        assert!(!state.apply_deduplication(0, revision, &[(2, 1)]));
        assert_eq!(state.photos.len(), 3);

        state.photos.index = 2;
        assert!(state.apply_deduplication(1, revision, &[(2, 1)]));
        assert_eq!(state.photos.index, 1);
        state.undo_playlist_edit();
        assert_eq!(state.photos.len(), 3);
        assert_eq!(state.photos.index, 2);
    }
}
