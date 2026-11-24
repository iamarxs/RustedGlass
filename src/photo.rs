//! Image files: the format list, folder enumeration, navigation and decoding.
//!
//! Only enabled decoders are browsable.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const IMAGE_FORMATS: &str = ".apng;.bmp;.dds;.exr;.farbfeld;.ff;.gif;.gifv;.hdr;.ico;.jfif;.jpe;.jpeg;.jpg;.pam;.pbm;.pgm;.png;.pnm;.ppm;.qoi;.tga;.tif;.tiff;.webp";

/// Extensions with a leading dot, lower-cased.
pub fn image_formats() -> Vec<String> {
    IMAGE_FORMATS
        .split(';')
        .map(|e| e.trim().to_lowercase())
        .filter(|e| !e.is_empty())
        .collect()
}

pub fn is_image_file(path: &Path, formats: &[String]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| formats.contains(&format!(".{}", e.to_lowercase())))
        .unwrap_or(false)
}

struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 17;
        self.state = x;
        x
    }
}

/// A value no earlier list or row change has used, so a stale token never matches a fresh one.
fn next_rows_token() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// An image list plus the position in it.
pub struct PhotoList {
    pub files: Vec<PathBuf>,
    pub index: usize,
    pub formats: Vec<String>,
    /// The playlist row order, as indices into `files`. Empty means `files` order.
    pub order: Vec<usize>,
    random_pass: Option<Vec<usize>>,
    rows_token: u64,
    layout_token: u64,
}

impl PhotoList {
    /// An empty playlist, which is what a launch with nothing displayable gets.
    pub fn empty() -> Self {
        Self {
            files: Vec::new(),
            index: 0,
            formats: image_formats(),
            order: Vec::new(),
            random_pass: None,
            rows_token: next_rows_token(),
            layout_token: next_rows_token(),
        }
    }

    /// Builds the list for `path`, recursing only when a directory was opened.
    pub fn from_path(
        path: &Path,
        include_hidden: bool,
        recurse: bool,
        formats: Vec<String>,
    ) -> Self {
        let opened_directory = path.is_dir();
        let dir = if opened_directory {
            path.to_path_buf()
        } else {
            path.parent().map(Path::to_path_buf).unwrap_or_default()
        };

        let files = collect(&dir, include_hidden, recurse && opened_directory, &formats);
        let wanted = if path.is_file() { Some(path) } else { None };

        let index = match wanted {
            Some(file) => find_opened_file(&files, file).unwrap_or(0),
            None => 0,
        };

        Self {
            files,
            index,
            formats,
            order: Vec::new(),
            random_pass: None,
            rows_token: next_rows_token(),
            layout_token: next_rows_token(),
        }
    }

    /// Changes with any change of the visible rows, not with the shown row alone.
    pub(crate) fn rows_token(&self) -> u64 {
        self.rows_token
    }

    pub(crate) fn layout_token(&self) -> u64 {
        self.layout_token
    }

    fn rows_changed(&mut self) {
        self.rows_token = next_rows_token();
        self.layout_token = self.rows_token;
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Maps a position in the visible order to an index into `files`.
    fn at(&self, position: usize) -> Option<usize> {
        if position >= self.files.len() {
            return None;
        }
        if self.order.is_empty() {
            return Some(position);
        }
        self.order
            .get(position)
            .copied()
            .filter(|&i| i < self.files.len())
    }

    pub fn current(&self) -> Option<&PathBuf> {
        self.path_at(self.index)
    }

    pub fn path_at(&self, position: usize) -> Option<&PathBuf> {
        self.at(position).map(|i| &self.files[i])
    }

    /// Folder scans keep discovery order in the backing vector even when the list is sorted.
    pub(crate) fn metadata_path_at(&self, position: usize) -> Option<&PathBuf> {
        self.files.get(position)
    }

    pub(crate) fn finish_folder_order(
        &mut self,
        start: usize,
        order: Vec<usize>,
        selected: Option<usize>,
    ) {
        debug_assert_eq!(start + order.len(), self.len());
        let selected = selected.or_else(|| self.file_index(self.index));
        if start == 0 {
            if order.iter().copied().eq(0..self.len()) {
                self.order.clear();
            } else {
                self.order = order;
            }
        } else {
            if self.order.is_empty() {
                self.order.extend(0..start);
            } else {
                self.order.truncate(start);
            }
            self.order
                .extend(order.into_iter().map(|offset| start + offset));
        }
        self.index = selected
            .and_then(|index| self.position_of_file(index))
            .unwrap_or(0);
        self.reset_random_pass();
        self.rows_changed();
    }

    /// The entries in the order browsing and the playlist menu use.
    #[cfg(test)] // only the PhotoList tests and the older save path used the collected form
    pub fn visible(&self) -> Vec<&PathBuf> {
        self.visible_rows().collect()
    }

    pub fn visible_rows(&self) -> impl Iterator<Item = &PathBuf> {
        (0..self.files.len()).filter_map(|position| self.path_at(position))
    }

    /// Find the visible row for a backing index, even when several rows share a path.
    pub fn position_of_file(&self, file_index: usize) -> Option<usize> {
        if self.order.is_empty() {
            (file_index < self.files.len()).then_some(file_index)
        } else {
            self.order.iter().position(|&index| index == file_index)
        }
    }

    /// True when the shown entry is the last one of the visible order.
    pub fn is_at_last(&self) -> bool {
        !self.is_empty() && self.index + 1 >= self.files.len()
    }

    /// Randomizes the visible order. `seed` is passed in so tests are repeatable.
    pub fn shuffle(&mut self, seed: u64) {
        self.reset_random_pass();
        let mut rng = XorShift64 {
            state: seed | 0x9d_df_e1_ed_c4_a1_95_65,
        };
        let mut order: Vec<usize> = (0..self.files.len()).collect();
        for i in (1..order.len()).rev() {
            let j = rng.next_u64() as usize % (i + 1);
            order.swap(i, j);
        }
        self.order = order;
        self.index = 0;
        self.rows_changed();
    }

    /// Appends new images without changing the current row or the visible order.
    pub fn add_files(&mut self, paths: &[PathBuf]) -> usize {
        let added: Vec<_> = paths
            .iter()
            .filter(|path| is_image_file(path, &self.formats))
            .cloned()
            .collect();
        self.extend_files(added)
    }

    pub(crate) fn extend_files(&mut self, added: Vec<PathBuf>) -> usize {
        let count = added.len();
        if count != 0 {
            self.reset_random_pass();
            self.rows_token = next_rows_token();
        }
        if !self.order.is_empty() {
            self.order
                .extend(self.files.len()..self.files.len() + count);
        }
        self.files.extend(added);
        count
    }

    /// A playlist of exactly these files. Used by the file picker, which replaces what is shown.
    pub fn from_files(paths: &[PathBuf], formats: Vec<String>) -> Self {
        let mut list = Self {
            files: Vec::new(),
            index: 0,
            formats,
            order: Vec::new(),
            random_pass: None,
            rows_token: next_rows_token(),
            layout_token: next_rows_token(),
        };
        list.add_files(paths);
        list.sort_by_name(false);
        list.index = 0;
        list
    }

    /// The backing index of a visible row.
    pub fn file_index(&self, position: usize) -> Option<usize> {
        self.at(position)
    }

    /// Removes the entry at a visible position; the entry after it takes its place.
    pub fn remove_at(&mut self, position: usize) -> Option<PathBuf> {
        let file_index = self.at(position)?;
        self.reset_random_pass();
        let removed = self.files.remove(file_index);
        self.order.retain(|index| *index != file_index);
        for index in &mut self.order {
            if *index > file_index {
                *index -= 1;
            }
        }
        self.index = if position < self.index {
            self.index - 1
        } else {
            self.index
        }
        .min(self.files.len().saturating_sub(1));
        self.rows_changed();
        Some(removed)
    }

    /// Removes the shown entry from the playlist, leaving the file on disk.
    pub fn remove_current(&mut self) -> Option<PathBuf> {
        self.remove_at(self.index)
    }

    /// Renames the resolved rows, keeping their order and the shown occurrence.
    pub(crate) fn replace_rows(&mut self, rows: &[usize], target: &Path) {
        let mut replaced = false;
        for &row in rows {
            if let Some(index) = self.at(row) {
                let entry = &mut self.files[index];
                if entry != target {
                    *entry = target.to_path_buf();
                    replaced = true;
                }
            }
        }
        if replaced {
            self.rows_changed();
        }
    }

    /// Moves to a visible position, as the playlist does.
    pub fn jump_to(&mut self, position: usize) -> Option<&PathBuf> {
        if self.at(position).is_none() {
            return None;
        }
        self.reset_random_pass();
        self.index = position;
        self.current()
    }

    /// Moves within the list, optionally wrapping at either end.
    pub fn step(&mut self, step: i32, loop_back: bool) -> bool {
        let count = self.files.len() as i32;
        if count == 0 {
            return false;
        }
        let next = self.index as i32 + step;

        if next >= 0 && next < count {
            self.index = next as usize;
            return true;
        }

        if loop_back {
            self.index = (((next % count) + count) % count) as usize;
            return true;
        }

        false
    }
}

pub(crate) fn find_opened_file(files: &[PathBuf], path: &Path) -> Option<usize> {
    files.iter().position(|file| file == path).or_else(|| {
        let actual = path.canonicalize().ok()?;
        // Compare normalized absolute paths only for matching file names. On a
        // case-sensitive volume, canonicalization keeps distinct paths distinct.
        let name = path.file_name()?.to_string_lossy().to_lowercase();
        files.iter().position(|file| {
            file.file_name()
                .is_some_and(|candidate| candidate.to_string_lossy().to_lowercase() == name)
                && file
                    .canonicalize()
                    .is_ok_and(|candidate| candidate == actual)
        })
    })
}

/// Lists image files in `dir`, optionally including hidden entries and subfolders.
#[cfg(test)]
mod format_tests;
#[cfg(test)]
mod tests;

mod folder;
pub use folder::collect;
pub(crate) use folder::{collect_reporting, sort_names, visit_folder, FolderScanError, NameOrder};

mod columns;
mod order;
pub(crate) use order::duplicate_rows;
mod path_identity;
pub(crate) use path_identity::path_identity;
mod random;
pub(crate) use columns::{Column, Details, FileFacts};
