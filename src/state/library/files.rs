use super::*;
use std::collections::{hash_map::Entry, HashMap};

impl State {
    pub fn delete_current(&mut self) -> Result<PathBuf, String> {
        let path = self.photos.current().cloned().ok_or("No image selected")?;
        let rows = self.photos.rows_for_path(&path);
        fileops::delete(&path)?;
        self.photos.remove_rows(&rows);
        self.playlist_undo = None;
        self.load_current();
        Ok(path)
    }

    /// Recycle each directory entry once, then remove the rows of successful operations.
    pub fn delete_many(&mut self, rows: &[usize]) -> Option<String> {
        self.recycle_rows(rows, fileops::delete)
    }

    fn recycle_rows(
        &mut self,
        rows: &[usize],
        mut recycle: impl FnMut(&Path) -> Result<(), String>,
    ) -> Option<String> {
        let rows = sorted_desc(rows);
        let keys = self.photos.row_identities(
            rows.iter()
                .filter_map(|&row| self.photos.path_at(row).map(PathBuf::as_path)),
        );
        let mut failed = None;
        let mut failure_count = 0;
        let mut attempts = HashMap::new();
        for row in rows {
            if let (Some(path), Some(Some(key))) = (self.photos.path_at(row), keys.get(row)) {
                if let Entry::Vacant(attempt) = attempts.entry(key) {
                    let result = recycle(path);
                    attempt.insert(result.is_ok());
                    if let Err(error) = result {
                        failure_count += 1;
                        failed.get_or_insert(error);
                    }
                }
            }
        }
        let removed: Vec<_> = keys
            .iter()
            .enumerate()
            .filter_map(|(row, key)| {
                key.as_ref()
                    .filter(|key| attempts.get(key) == Some(&true))
                    .map(|_| row)
            })
            .collect();
        if self.photos.remove_rows(&removed) != 0 {
            self.playlist_undo = None;
            self.load_current();
        }
        failed.map(|error| {
            format!(
                "Could not recycle {failure_count} of {} files. {error}",
                attempts.len()
            )
        })
    }

    pub fn move_current_to_target(&mut self) -> Result<PathBuf, String> {
        if self.knobs.move_to.as_os_str().is_empty() {
            return Err("No 'Move to' folder is set. Open the settings (S) and pick one.".into());
        }
        let path = self.photos.current().cloned().ok_or("No image selected")?;
        let rows = self.photos.rows_for_path(&path);
        let target = fileops::move_to(&path, &self.knobs.move_to)?;
        self.photos.remove_rows(&rows);
        self.playlist_undo = None;
        self.load_current();
        self.message = Some(format!("Moved to {}", target.display()));
        Ok(target)
    }

    pub fn apply_pending(&mut self, text: &str) -> Result<PathBuf, String> {
        let path = self.pending.as_ref().ok_or("No rename is open")?;
        if self.navigation_pending || self.load_pending || self.photos.current() != Some(path) {
            return Err("Image selection changed. Open the file command again.".into());
        }
        let rows = self.photos.rows_for_path(path);
        let target = fileops::rename(path, text)?;
        if &target != path {
            self.photos.replace_rows(&rows, &target);
            self.playlist_undo = None;
        }
        self.pending = None;
        self.message = None;
        Ok(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_and_move_update_every_spelling_and_keep_unrelated_rows() {
        let dir = crate::test_path("rename-move-matching-rows");
        let destination = dir.join("moved");
        std::fs::create_dir_all(&destination).unwrap();
        let original = dir.join("Trip.ppm");
        let alias = dir.join("trip.PPM");
        let other = dir.join("Other.ppm");
        for file in [&original, &other] {
            std::fs::write(file, b"P6\n1 1\n255\n\xff\x00\x00").unwrap();
        }
        let mut state = State::new(crate::test_path("unused-config.json"), &[]);
        state.photos.files = vec![original, other.clone(), alias.clone()];
        state.photos.index = 2;
        state.pending = Some(alias);
        let renamed = state.apply_pending("holiday.ppm").unwrap();
        assert_eq!(
            state.photos.visible_rows().collect::<Vec<_>>(),
            [&renamed, &other, &renamed]
        );
        assert_eq!(state.photos.index, 2);
        state.photos.files[0] = dir.join("HOLIDAY.PPM");
        state.knobs.move_to = destination;
        let moved = state.move_current_to_target().unwrap();
        assert_eq!(state.photos.visible_rows().collect::<Vec<_>>(), [&other]);
        assert!(!renamed.exists());
        assert!(moved.is_file());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn recycle_keeps_failed_aliases_and_attempts_each_file_once() {
        let mut state = State::new(crate::test_path("unused-config.json"), &[]);
        let dir = crate::test_path("recycle-matching-rows");
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("A.ppm");
        let b = dir.join("B.ppm");
        for file in [&a, &b] {
            std::fs::write(file, b"P6\n1 1\n255\n\xff\x00\x00").unwrap();
        }
        let b_alias = dir.join("b.PPM");
        state.photos.files = vec![a.clone(), b.clone(), dir.join("a.PPM"), b_alias.clone()];
        state.photos.index = 3;
        let mut calls = 0;
        let error = state.recycle_rows(&[0, 1, 2], |path| {
            calls += 1;
            if path.eq(&b) {
                Err("In use".into())
            } else {
                std::fs::remove_file(path).map_err(|error| error.to_string())
            }
        });
        assert_eq!(calls, 2);
        assert_eq!(
            error.as_deref(),
            Some("Could not recycle 1 of 2 files. In use")
        );
        assert_eq!(
            state.photos.visible_rows().collect::<Vec<_>>(),
            [&b, &b_alias]
        );
        assert_eq!(state.photos.index, 1);
        std::fs::remove_file(b).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
