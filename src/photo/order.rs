use super::*;

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
