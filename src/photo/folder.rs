use super::*;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
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
            // The enumeration already knows the type, so a plain entry costs no stat.
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                // Preserve the exclusion for every directory reparse point.
                if recurse
                    && entry.metadata().is_ok_and(|metadata| {
                        !is_reparse_point(&metadata) && !skip_attributes(&metadata, include_hidden)
                    })
                {
                    stack.push(path);
                }
                continue;
            }

            if !is_image_file(&path, formats) {
                continue;
            }
            let metadata = entry.metadata().and_then(|metadata| {
                if is_reparse_point(&metadata) {
                    path.metadata()
                } else {
                    Ok(metadata)
                }
            });
            if metadata.is_ok_and(|metadata| {
                metadata.is_file() && !skip_attributes(&metadata, include_hidden)
            }) {
                files.push(path);
            }
        }
    }

    sort_names(&mut files, false, None);
    files
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

fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

fn skip_attributes(metadata: &fs::Metadata, include_hidden: bool) -> bool {
    const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    let attributes = metadata.file_attributes();
    attributes & FILE_ATTRIBUTE_SYSTEM != 0
        || (!include_hidden && attributes & FILE_ATTRIBUTE_HIDDEN != 0)
}
