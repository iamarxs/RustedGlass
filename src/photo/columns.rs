//! The playlist columns: the column model, per-file details, and sort keys.
//!
//! `Details` is one bounded read per file: filesystem fields plus the image
//! header, mirroring the limits `metadata.rs` uses for Image Info.

use std::borrow::Cow;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTimeEx};
use windows::Win32::UI::Shell::StrCmpLogicalW;

mod header;
mod image_type;
mod jpeg_header;
#[cfg(test)]
mod jpeg_tests;

const MAX_METADATA_INPUT: u64 = 64 * 1024 * 1024;
const MAX_METADATA_ALLOC: u64 = 32 * 1024 * 1024;

/// The cheap filesystem fields of one file, taken in a single stat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FileFacts {
    pub(crate) size: u64,
    /// Modification time as Windows FILETIME ticks, including subsecond resolution.
    pub(crate) modified: u64,
}

/// The columns of the playlist view, in the order they appear in the menu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Column {
    Name,
    FullPath,
    Width,
    Height,
    Type,
    Size,
    Date,
}

impl Column {
    pub(crate) const ALL: [Self; 7] = [
        Self::Name,
        Self::FullPath,
        Self::Width,
        Self::Height,
        Self::Type,
        Self::Size,
        Self::Date,
    ];

    /// The native header text, the way the details view names these fields.
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::FullPath => "Full path",
            Self::Width => "Width",
            Self::Height => "Height",
            Self::Type => "Type",
            Self::Size => "Size",
            Self::Date => "Date",
        }
    }

    /// True when the column shows a field of `Details` instead of the path.
    pub(crate) fn needs_metadata(self) -> bool {
        !matches!(self, Self::Name | Self::FullPath)
    }

    /// The cell text; fields without details stay blank rather than blocking on IO.
    pub(crate) fn text(self, path: &Path, details: Option<&Details>) -> String {
        match self {
            Self::Name => path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .into_owned(),
            Self::FullPath => absolute_path(path).display().to_string(),
            Self::Width => number(details.and_then(|read| read.width).map(u64::from)),
            Self::Height => number(details.and_then(|read| read.height).map(u64::from)),
            Self::Type => details
                .and_then(|read| read.image_type.as_deref())
                .map(str::to_owned)
                .unwrap_or_default(),
            Self::Size => details
                .and_then(|read| read.size)
                .map(size_text)
                .unwrap_or_default(),
            Self::Date => details
                .and_then(|read| read.modified)
                .map(local_date_text)
                .unwrap_or_default(),
        }
    }

    /// The precomputed sort key, so comparing costs no decoding or IO.
    pub(crate) fn sort_key(self, path: &Path, details: Option<&Details>) -> SortKey {
        match self {
            Self::Name => SortKey::logical(path.file_name().unwrap_or(path.as_os_str())),
            Self::FullPath => SortKey::logical(absolute_path(path).as_os_str()),
            Self::Width => SortKey::Number(details.and_then(|read| read.width).map(u64::from)),
            Self::Height => SortKey::Number(details.and_then(|read| read.height).map(u64::from)),
            Self::Type => SortKey::Text(
                details
                    .and_then(|read| read.image_type.clone())
                    .unwrap_or_default(),
            ),
            Self::Size => SortKey::Number(details.and_then(|read| read.size)),
            Self::Date => SortKey::Number(details.and_then(|read| read.modified)),
        }
    }
}

fn number(value: Option<u64>) -> String {
    value.map_or_else(String::new, |value| value.to_string())
}

fn size_text(bytes: u64) -> String {
    let rounded_kb = bytes / 1024 + u64::from(bytes % 1024 >= 512);
    format!("{} KB", rounded_kb.max(u64::from(bytes != 0)))
}

pub(super) fn absolute_path(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// A key one column sorts by; the compared pair is always the same shape.
pub(crate) enum SortKey {
    /// Explorer's natural name ordering, as the Name column follows on disk.
    Logical(Vec<u16>),
    Text(Cow<'static, str>),
    Number(Option<u64>),
}

impl SortKey {
    fn logical(text: &OsStr) -> Self {
        use std::os::windows::ffi::OsStrExt;
        Self::Logical(text.encode_wide().chain([0]).collect())
    }

    pub(crate) fn compare(&self, other: &Self, descending: bool) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        let missing = self.missing().cmp(&other.missing());
        if missing != Ordering::Equal {
            return missing;
        }
        let order = match (self, other) {
            (Self::Logical(a), Self::Logical(b)) => unsafe {
                StrCmpLogicalW(PCWSTR(a.as_ptr()), PCWSTR(b.as_ptr())).cmp(&0)
            },
            (Self::Text(a), Self::Text(b)) => a.cmp(b),
            (Self::Number(a), Self::Number(b)) => a.cmp(b),
            // The column decides the shape; a mixed pair is never sorted.
            _ => Ordering::Equal,
        };
        if descending {
            order.reverse()
        } else {
            order
        }
    }

    fn missing(&self) -> bool {
        match self {
            Self::Logical(_) => false,
            Self::Text(text) => text.is_empty(),
            Self::Number(number) => number.is_none(),
        }
    }
}

/// The fields the columns show for one file.
#[derive(Clone, Default)]
pub(crate) struct Details {
    pub(crate) width: Option<u32>,
    pub(crate) height: Option<u32>,
    /// Detected image format, or the extension when the header did not say.
    pub(crate) image_type: Option<Cow<'static, str>>,
    pub(crate) size: Option<u64>,
    /// Modification time as Windows FILETIME ticks, including subsecond resolution.
    pub(crate) modified: Option<u64>,
    header_attempted: bool,
}

impl Details {
    /// The stat fields only, as a seeded row looks before its header was read.
    pub(crate) fn from_facts(facts: FileFacts) -> Self {
        Self {
            size: Some(facts.size),
            modified: Some(facts.modified),
            ..Self::default()
        }
    }

    /// The filesystem fields when both were read, for a caller reusing one stat.
    pub(crate) fn facts(&self) -> Option<FileFacts> {
        let size = self.size?;
        let modified = self.modified?;
        Some(FileFacts { size, modified })
    }

    /// True once a header read was tried, even when it found nothing usable.
    pub(crate) fn header_read(&self) -> bool {
        self.header_attempted
    }

    /// The stat fields when given, otherwise one stat of the file; the image header
    /// only when asked, within the same limits Image Info uses.
    pub(crate) fn read_with_facts(
        path: &Path,
        facts: Option<FileFacts>,
        read_header: bool,
    ) -> Self {
        let file = read_header.then(|| File::open(path).ok()).flatten();
        let facts = facts.or_else(|| match &file {
            Some(file) => file.metadata().ok().and_then(file_facts),
            None => fs::metadata(path).ok().and_then(file_facts),
        });
        let mut read = Self {
            image_type: facts.is_some().then(|| image_type::extension(path)),
            size: facts.map(|facts| facts.size),
            modified: facts.map(|facts| facts.modified),
            header_attempted: read_header,
            ..Self::default()
        };
        if let Some(file) = file.filter(|_| facts.is_some()) {
            read.header(path, file);
        }
        read
    }

    /// One bounded read: filesystem fields always, the image header within the same
    /// limits Image Info uses.
    #[cfg(test)]
    pub(crate) fn read(path: &Path) -> Self {
        Self::read_with_facts(path, None, true)
    }
}

/// One stat: the filesystem fields of an existing file.
fn file_facts(stat: fs::Metadata) -> Option<FileFacts> {
    stat.is_file().then(|| FileFacts {
        size: stat.len(),
        modified: stat.last_write_time(),
    })
}

/// Local date and time for the file's modification timestamp.
fn local_date_text(ticks: u64) -> String {
    let file_time = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut utc = SYSTEMTIME::default();
    let mut system = SYSTEMTIME::default();
    let converted = unsafe { FileTimeToSystemTime(&file_time, &mut utc) }.is_ok()
        && unsafe { SystemTimeToTzSpecificLocalTimeEx(None, &utc, &mut system) }.is_ok();
    if !converted {
        return String::new();
    }
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        system.wYear, system.wMonth, system.wDay, system.wHour, system.wMinute
    )
}

#[cfg(test)]
mod tests;
