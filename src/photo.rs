//! Image files: the format list, folder enumeration, navigation and decoding.
//!
//! Ported from `Common/Types/Const.cs` (`IMAGE_FORMATS`), `Common/BHelper/Path.cs` and the
//! navigation part of the main-window view model. Only enabled Rust decoders are browsable.

use std::fs;
use std::path::{Path, PathBuf};

pub const IMAGE_FORMATS: &str = ".bmp;.dds;.gif;.gifv;.hdr;.ico;.jfif;.jpe;.jpeg;.jpg;.pam;.pbm;.pgm;.png;.pnm;.ppm;.qoi;.tga;.tif;.tiff;.webp";

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

/// An image list plus the position in it.
pub struct PhotoList {
    pub files: Vec<PathBuf>,
    pub index: usize,
    pub formats: Vec<String>,
    /// The visible order, as indices into `files`. Empty means `files` order;
    /// the randomizer fills it and clearing it returns to the names. `index` is always a
    /// position in this visible order, so browsing and the slideshow follow the
    /// shuffle with no extra handling.
    pub order: Vec<usize>,
}

impl PhotoList {
    /// An empty playlist, which is what a launch with nothing displayable gets.
    pub fn empty() -> Self {
        Self {
            files: Vec::new(),
            index: 0,
            formats: image_formats(),
            order: Vec::new(),
        }
    }

    /// Builds the list for the folder containing `path` (a file or a directory).
    pub fn from_path(
        path: &Path,
        include_hidden: bool,
        recurse: bool,
        formats: Vec<String>,
    ) -> Self {
        let dir = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent().map(Path::to_path_buf).unwrap_or_default()
        };

        let files = collect(&dir, include_hidden, recurse, &formats);
        let wanted = if path.is_file() { Some(path) } else { None };

        let index = match wanted {
            Some(file) => files.iter().position(|f| f == file).unwrap_or(0),
            None => 0,
        };

        Self {
            files,
            index,
            formats,
            order: Vec::new(),
        }
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

    /// The entries in the order browsing and the playlist menu use.
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
    }

    /// Appends new images without changing the current row or the visible order.
    pub fn add_files(&mut self, paths: &[PathBuf]) -> usize {
        let added: Vec<_> = paths
            .iter()
            .filter(|path| is_image_file(path, &self.formats))
            .cloned()
            .collect();
        let count = added.len();
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
        Some(removed)
    }

    /// Removes the shown entry from the playlist, leaving the file on disk.
    pub fn remove_current(&mut self) -> Option<PathBuf> {
        self.remove_at(self.index)
    }

    /// Moves to a visible position, as the playlist does.
    pub fn jump_to(&mut self, position: usize) -> Option<&PathBuf> {
        if self.at(position).is_none() {
            return None;
        }
        self.index = position;
        self.current()
    }

    /// Moves `step` positions, optionally wrapping, and optionally jumping to the next sibling
    /// folder that holds images when the ends of this folder are reached.
    pub fn step(
        &mut self,
        step: i32,
        loop_back: bool,
        auto_sibling_dir: bool,
        shuffle_seed: Option<u64>,
        (include_hidden, recurse): (bool, bool),
    ) -> bool {
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

        if auto_sibling_dir {
            if let Some(files) = sibling_images(
                self.current().and_then(|p| p.parent()),
                step,
                include_hidden,
                recurse,
                &self.formats,
            ) {
                self.files = files;
                self.order.clear();
                self.index = 0;
                if let Some(seed) = shuffle_seed {
                    self.shuffle(seed);
                }
                if step < 0 {
                    self.index = self.files.len().saturating_sub(1);
                }
                return true;
            }
        }

        false
    }
}

/// Lists image files in `dir`, optionally including hidden entries and subfolders.
#[cfg(test)]
mod tests;

mod folder;
pub use folder::collect;
use folder::{sibling_images, sort_names};

mod order;
