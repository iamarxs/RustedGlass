use super::*;

impl PhotoList {
    /// Move a visible row while keeping the selected occurrence of an image.
    pub fn move_row(&mut self, from: usize, to: usize) {
        if from >= self.len() || to >= self.len() || from == to {
            return;
        }
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
    }

    /// Orders the entries by name, reversed when `descending`.
    pub fn sort_by_name(&mut self, descending: bool) {
        let selected = self.file_index(self.index);
        self.order.clear();
        self.index = sort_names(&mut self.files, descending, selected).unwrap_or(0);
    }

    pub fn remove_matching(&mut self, mut matches: impl FnMut(&Path) -> bool) {
        for row in (0..self.len()).rev() {
            if self.path_at(row).is_some_and(|path| matches(path)) {
                self.remove_at(row);
            }
        }
    }
}
