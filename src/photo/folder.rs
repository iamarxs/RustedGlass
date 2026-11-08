use super::*;
use std::os::windows::ffi::OsStrExt;
use windows::core::PCWSTR;
use windows::Win32::UI::Shell::StrCmpLogicalW;
pub fn collect(
    dir: &Path,
    include_hidden: bool,
    recurse: bool,
    formats: &[String],
) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_dir() {
                if recurse && !is_directory_link(&path) && !is_skipped(&path, include_hidden) {
                    stack.push(path);
                }
                continue;
            }

            if is_skipped(&path, include_hidden) || !is_image_file(&path, formats) {
                continue;
            }
            files.push(path);
        }
    }

    sort_names(&mut files, false, None);
    files
}

/// The next sibling folder of `dir` (in sorted order) that contains images.
pub(super) fn sibling_images(
    dir: Option<&Path>,
    step: i32,
    include_hidden: bool,
    recurse: bool,
    formats: &[String],
) -> Option<Vec<PathBuf>> {
    let dir = dir?;
    let parent = dir.parent()?;
    let mut siblings: Vec<PathBuf> = fs::read_dir(parent)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();

    sort_names(&mut siblings, false, None);

    let current = dir.to_string_lossy().to_lowercase();
    let start = siblings
        .iter()
        .position(|s| s.to_string_lossy().to_lowercase() == current)?;

    let mut index = start as i32 + step;
    while index >= 0 && (index as usize) < siblings.len() {
        let candidate = &siblings[index as usize];
        if !is_skipped(candidate, include_hidden) {
            let files = collect(candidate, include_hidden, recurse, formats);
            if !files.is_empty() {
                return Some(files);
            }
        }
        index += step;
    }

    None
}

pub(super) fn sort_names(
    paths: &mut Vec<PathBuf>,
    descending: bool,
    selected: Option<usize>,
) -> Option<usize> {
    let mut keyed: Vec<_> = paths
        .drain(..)
        .enumerate()
        .map(|(index, path)| {
            let name = path.file_name().unwrap_or(path.as_os_str());
            (
                name.encode_wide().chain([0]).collect::<Vec<_>>(),
                index,
                path,
            )
        })
        .collect();
    keyed.sort_by(|(a, ..), (b, ..)| {
        let order = unsafe { StrCmpLogicalW(PCWSTR(a.as_ptr()), PCWSTR(b.as_ptr())) }.cmp(&0);
        if descending {
            order.reverse()
        } else {
            order
        }
    });
    let selected =
        selected.and_then(|selected| keyed.iter().position(|(_, index, _)| *index == selected));
    paths.extend(keyed.into_iter().map(|(_, _, path)| path));
    selected
}

fn is_directory_link(path: &Path) -> bool {
    let Ok(metadata) = path.symlink_metadata() else {
        return true;
    };
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    metadata.is_symlink()
}

/// Windows: system entries are always skipped, hidden ones only when hidden browsing is off.
fn is_skipped(path: &Path, include_hidden: bool) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;

        let Ok(meta) = path.metadata() else {
            return false;
        };
        let attributes = meta.file_attributes();

        return attributes & FILE_ATTRIBUTE_SYSTEM != 0
            || (!include_hidden && attributes & FILE_ATTRIBUTE_HIDDEN != 0);
    }

    #[cfg(not(windows))]
    {
        let _ = include_hidden;
        false
    }
}
