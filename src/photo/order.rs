use super::columns::{Column, Details, SortKey};
use super::*;
use std::collections::HashMap;

impl PhotoList {
    /// Sorts the visible rows by a playlist column, keeping the shown occurrence.
    pub(crate) fn sort_by_column(
        &mut self,
        column: Column,
        descending: bool,
        details: &HashMap<PathBuf, Details>,
    ) {
        self.sort_by_column_with(column, descending, |path| details.get(path));
    }

    pub(crate) fn sort_by_column_with<'a>(
        &mut self,
        column: Column,
        descending: bool,
        mut details: impl FnMut(&Path) -> Option<&'a Details>,
    ) {
        self.reset_random_pass();
        let selected = self.file_index(self.index);
        let mut visible_position = Vec::new();
        if !self.order.is_empty() {
            visible_position.resize(self.len(), 0);
            for (position, &index) in self.order.iter().enumerate() {
                visible_position[index] = position;
            }
        }
        let mut keyed: Vec<(SortKey, usize, PathBuf)> = self
            .files
            .drain(..)
            .enumerate()
            .map(|(index, path)| (column.sort_key(&path, details(&path)), index, path))
            .collect();
        // Stable, so equal keys keep their order in both directions.
        keyed.sort_by(|(a, left, _), (b, right, _)| {
            a.compare(b, descending).then_with(|| {
                if visible_position.is_empty() {
                    left.cmp(right)
                } else {
                    visible_position[*left].cmp(&visible_position[*right])
                }
            })
        });
        self.order.clear();
        self.index = selected
            .and_then(|selected| keyed.iter().position(|(_, index, _)| *index == selected))
            .unwrap_or(0);
        self.files = keyed.into_iter().map(|(_, _, path)| path).collect();
        self.rows_changed();
    }

    /// The synchronous test entry point. The viewer resolves keys on its preparation lane.
    #[cfg(test)]
    pub(crate) fn remove_duplicates(&mut self) -> usize {
        let rows = duplicate_rows(self.visible_rows().cloned(), &|| true);
        self.apply_duplicate_rows(&rows)
    }

    /// The worker's row indices belong to the current visible ordering. Move selection
    /// to the kept occurrence before removal, so the preview can stay on the same file.
    pub(crate) fn apply_duplicate_rows(&mut self, duplicates: &[(usize, usize)]) -> usize {
        if let Ok(position) = duplicates.binary_search_by_key(&self.index, |(row, _)| *row) {
            self.index = duplicates[position].1;
        }
        let rows: Vec<_> = duplicates.iter().map(|&(row, _)| row).collect();
        self.remove_rows(&rows)
    }
}

/// For each later occurrence, returns its row and the first row for that path.
pub(crate) fn duplicate_rows(
    paths: impl IntoIterator<Item = PathBuf>,
    active: &dyn Fn() -> bool,
) -> Vec<(usize, usize)> {
    use std::collections::hash_map::Entry;

    let mut identities = HashMap::new();
    let mut parents = super::path_identity::PathIdentities::default();
    let mut first_rows = HashMap::new();
    let mut duplicates = Vec::new();
    for (row, path) in paths.into_iter().enumerate() {
        if !active() {
            break;
        }
        let identity = identities
            .entry(path)
            .or_insert_with_key(|path| parents.resolve(path));
        match first_rows.entry(identity.clone()) {
            Entry::Occupied(first) => duplicates.push((row, *first.get())),
            Entry::Vacant(first) => {
                first.insert(row);
            }
        }
    }
    duplicates
}

impl PhotoList {
    /// Move a visible row while keeping the selected occurrence of an image.
    pub fn move_row(&mut self, from: usize, to: usize) {
        if from >= self.len() || to >= self.len() || from == to {
            return;
        }
        self.reset_random_pass();
        let selected = self.file_index(self.index);
        if self.order.is_empty() {
            self.order.extend(0..self.files.len());
        }
        let moved = self.order.remove(from);
        self.order.insert(to, moved);
        self.index = selected
            .and_then(|index| self.position_of_file(index))
            .unwrap_or(0);
        self.files = self
            .order
            .drain(..)
            .map(|index| self.files[index].clone())
            .collect();
        self.rows_changed();
    }

    /// Orders the entries by name, reversed when `descending`.
    pub fn sort_by_name(&mut self, descending: bool) {
        self.reset_random_pass();
        let selected = self.file_index(self.index);
        self.order.clear();
        self.index = sort_names(&mut self.files, descending, selected).unwrap_or(0);
        self.rows_changed();
    }

    #[cfg(test)]
    pub fn remove_matching(&mut self, mut matches: impl FnMut(&Path) -> bool) {
        let rows: Vec<usize> = (0..self.len())
            .filter(|&row| self.path_at(row).is_some_and(|path| matches(path)))
            .collect();
        self.remove_rows(&rows);
    }

    /// Takes the visible rows in `rows` out in one pass; repeated and out-of-range rows are
    /// ignored. The shown row keeps its picture or falls to the next one, the previous at the
    /// end, and the remaining rows keep their order. Returns how many rows went.
    pub fn remove_rows(&mut self, rows: &[usize]) -> usize {
        let len = self.files.len();
        let mut drop = vec![false; len];
        let mut count = 0;
        for &row in rows {
            if let Some(index) = self.at(row) {
                if !drop[index] {
                    drop[index] = true;
                    count += 1;
                }
            }
        }
        if count == 0 {
            return 0;
        }
        self.reset_random_pass();

        let removed_before = (0..self.index.min(len))
            .filter_map(|row| self.at(row))
            .filter(|&index| drop[index])
            .count();
        let index = self.index.saturating_sub(removed_before);

        if !self.order.is_empty() {
            let mut remap = vec![0; len];
            for (new, old) in (0..len).filter(|&old| !drop[old]).enumerate() {
                remap[old] = new;
            }
            self.order.retain_mut(|old| {
                if drop[*old] {
                    return false;
                }
                *old = remap[*old];
                true
            });
        }
        let mut keep = drop.into_iter().map(|remove| !remove);
        self.files.retain(|_| keep.next() == Some(true));
        self.index = index.min(self.files.len().saturating_sub(1));
        self.rows_changed();
        count
    }
}
