//! Undo for in-memory playlist edits: Remove, Remove Duplicates and Shuffle rows. A snapshot
//! keeps the exact backing files, the visible order and the selected row; it is committed only
//! when the edit really changed the rows, and applied only while no later row change happened.

use super::*;

/// What one playlist edit took away. `post_token` is the rows token right after the edit, so
/// `undo_playlist_edit` can tell an untouched list from one another edit already changed.
pub(super) struct PlaylistUndo {
    files: Vec<PathBuf>,
    order: Vec<usize>,
    index: usize,
    post_token: u64,
}

impl State {
    /// Snapshot of the rows as they are now, before an edit. The caller commits it after the
    /// edit, and only a changed rows token makes the commit stick.
    pub(super) fn capture_playlist_edit(&self) -> PlaylistUndo {
        PlaylistUndo {
            files: self.photos.files.clone(),
            order: self.photos.order.clone(),
            index: self.photos.index,
            post_token: self.photos.rows_token(),
        }
    }

    /// Remembers the snapshot as the undoable edit, but only when the edit changed the rows.
    pub(super) fn commit_playlist_edit(&mut self, mut undo: PlaylistUndo) {
        let token = self.photos.rows_token();
        if token != undo.post_token {
            undo.post_token = token;
            self.playlist_undo = Some(undo);
        }
    }

    /// Restores the last committed edit. A later row change (another open, add, remove or file
    /// operation) moved the rows token past the saved one, so the restore is dropped instead of
    /// clobbering it; navigation alone does not change the token and still permits undo.
    pub fn undo_playlist_edit(&mut self) {
        let Some(undo) = self.playlist_undo.take() else {
            return;
        };
        if self.photos.rows_token() != undo.post_token {
            return;
        }
        // A fresh list, so replies of the replaced one cannot match its rows token.
        let mut fresh = PhotoList::empty();
        fresh.files = undo.files;
        fresh.order = undo.order;
        fresh.index = undo.index.min(fresh.files.len().saturating_sub(1));
        fresh.formats = self.photos.formats.clone();
        self.photos = fresh;
        self.load_current();
    }

    /// Randomizes the visible rows, keeping the shown occurrence, as the one undoable edit.
    pub fn shuffle_playlist_rows(&mut self) {
        let undo = self.capture_playlist_edit();
        let shown = self.photos.file_index(self.photos.index);
        self.photos.shuffle(random_seed());
        if let Some(index) = shown {
            self.photos.index = self.photos.position_of_file(index).unwrap_or(0);
        }
        self.commit_playlist_edit(undo);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with_rows(rows: &[&str]) -> State {
        let mut state = State::new(crate::test_path("unused-config.json"), &[]);
        state.photos.files = rows.iter().map(PathBuf::from).collect();
        state
    }

    #[test]
    fn undo_restores_files_order_and_selection_after_a_remove() {
        let mut state = state_with_rows(&["a.png", "b.png", "c.png", "b.png"]);
        state.photos.index = 2;
        let undo = state.capture_playlist_edit();
        let removed = state.photos.remove_rows(&[1]);
        assert_eq!(removed, 1);
        state.commit_playlist_edit(undo);
        assert_eq!(state.photos.files.len(), 3);
        state.undo_playlist_edit();
        assert_eq!(
            state.photos.files,
            vec!["a.png", "b.png", "c.png", "b.png"]
                .into_iter()
                .map(PathBuf::from)
                .collect::<Vec<_>>()
        );
        assert_eq!(state.photos.index, 2);
        assert_eq!(state.photos.current(), Some(&PathBuf::from("c.png")));
    }

    #[test]
    fn commit_without_a_row_change_keeps_no_undo() {
        let mut state = state_with_rows(&["a.png", "b.png"]);
        let undo = state.capture_playlist_edit();
        state.photos.jump_to(1); // navigation alone changes no rows token
        state.commit_playlist_edit(undo);
        assert!(state.playlist_undo.is_none());
        state.undo_playlist_edit();
        assert_eq!(state.photos.len(), 2);
    }

    #[test]
    fn navigation_alone_still_permits_undo() {
        let mut state = state_with_rows(&["a.png", "b.png", "c.png"]);
        let undo = state.capture_playlist_edit();
        state.photos.remove_rows(&[0]);
        state.commit_playlist_edit(undo);
        state.photos.jump_to(1);
        state.undo_playlist_edit();
        assert_eq!(state.photos.len(), 3);
        assert_eq!(state.photos.current(), Some(&PathBuf::from("a.png")));
    }

    #[test]
    fn undo_is_dropped_when_a_later_row_change_happened() {
        let mut state = state_with_rows(&["a.png", "b.png", "c.png"]);
        let undo = state.capture_playlist_edit();
        state.photos.remove_rows(&[0]);
        state.commit_playlist_edit(undo);
        // An unrelated later edit moves the rows token past the saved one.
        state.photos.remove_rows(&[0]);
        state.undo_playlist_edit();
        assert_eq!(state.photos.len(), 1, "the later removal must survive");
        assert_eq!(state.photos.current(), Some(&PathBuf::from("c.png")));
    }

    #[test]
    fn shuffle_is_undoable_and_keeps_the_shown_occurrence() {
        let mut state = state_with_rows(&["a.png", "b.png", "c.png", "d.png"]);
        state.photos.index = 1;
        state.shuffle_playlist_rows();
        assert_eq!(state.photos.current(), Some(&PathBuf::from("b.png")));
        state.undo_playlist_edit();
        let restored: Vec<_> = state.photos.visible_rows().cloned().collect();
        assert_eq!(
            restored,
            vec!["a.png", "b.png", "c.png", "d.png"]
                .into_iter()
                .map(PathBuf::from)
                .collect::<Vec<_>>()
        );
        assert_eq!(state.photos.index, 1);
        assert_eq!(state.photos.current(), Some(&PathBuf::from("b.png")));
    }
}
