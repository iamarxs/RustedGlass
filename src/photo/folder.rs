use super::*;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use windows::core::PCWSTR;
use windows::Win32::UI::Shell::StrCmpLogicalW;

#[cfg(test)]
mod tests;

/// A fatal folder-scan failure: the directory whose enumeration failed and the OS error.
pub(crate) struct FolderScanError {
    pub folder: PathBuf,
    pub error: std::io::Error,
}

impl std::fmt::Display for FolderScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.folder.display(), self.error)
    }
}

pub fn collect(
    dir: &Path,
    include_hidden: bool,
    recurse: bool,
    formats: &[String],
) -> Vec<PathBuf> {
    collect_reporting(dir, include_hidden, recurse, formats, &|| true, &mut |_| {}).0
}

pub(crate) fn collect_reporting(
    dir: &Path,
    include_hidden: bool,
    recurse: bool,
    formats: &[String],
    active: &impl Fn() -> bool,
    found: &mut dyn FnMut(&Path),
) -> (Vec<PathBuf>, Option<FolderScanError>) {
    let mut files = Vec::new();
    let result = visit_folder(
        dir,
        include_hidden,
        recurse,
        formats,
        active,
        &mut |path, _| {
            found(&path);
            files.push(path);
        },
    );
    if active() {
        sort_names(&mut files, false, None);
    }
    (files, result.err())
}

pub(crate) fn visit_folder(
    dir: &Path,
    include_hidden: bool,
    recurse: bool,
    formats: &[String],
    active: &dyn Fn() -> bool,
    found: &mut dyn FnMut(PathBuf, FileFacts),
) -> Result<(), FolderScanError> {
    let root = fs::read_dir(dir).map_err(|error| FolderScanError {
        folder: dir.to_path_buf(),
        error,
    })?;
    let mut stack: Vec<_> = vec![(dir.to_path_buf(), root)];
    while active() {
        let Some((folder, entries)) = stack.last_mut() else {
            break;
        };
        match entries.next() {
            None => {
                stack.pop();
            }
            Some(Err(error)) => {
                return Err(FolderScanError {
                    folder: folder.clone(),
                    error,
                });
            }
            Some(Ok(entry)) => {
                let path = entry.path();
                // The enumeration already knows the type, so a plain entry costs no stat.
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                if kind.is_dir() {
                    // Preserve the exclusion for every directory reparse point.
                    if recurse
                        && entry.metadata().is_ok_and(|metadata| {
                            !is_reparse_point(&metadata)
                                && !skip_attributes(&metadata, include_hidden)
                        })
                    {
                        match fs::read_dir(&path) {
                            Ok(entries) => stack.push((path, entries)),
                            Err(error) => {
                                return Err(FolderScanError {
                                    folder: path,
                                    error,
                                })
                            }
                        }
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
                if let Ok(metadata) = metadata {
                    if metadata.is_file() && !skip_attributes(&metadata, include_hidden) {
                        found(
                            path,
                            FileFacts {
                                size: metadata.len(),
                                modified: metadata.last_write_time(),
                            },
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn sort_names(
    paths: &mut Vec<PathBuf>,
    descending: bool,
    selected: Option<usize>,
) -> Option<usize> {
    let mut keys = NameOrder::default();
    keys.starts.reserve(paths.len());
    for path in paths.iter() {
        keys.push(path);
    }
    let order = keys.sorted(descending);
    let selected = selected.and_then(|selected| order.iter().position(|&index| index == selected));
    let mut slots: Vec<Option<PathBuf>> = paths.drain(..).map(Some).collect();
    paths.extend(
        order
            .into_iter()
            .map(|index| slots[index].take().expect("each occurrence moves once")),
    );
    selected
}

/// Stores only the leaf names while discovery streams full paths to the viewer.
#[derive(Default)]
pub(crate) struct NameOrder {
    arena: Vec<u16>,
    starts: Vec<usize>,
}

impl NameOrder {
    pub(crate) fn push(&mut self, path: &Path) {
        self.starts.push(self.arena.len());
        self.arena
            .extend(path.file_name().unwrap_or(path.as_os_str()).encode_wide());
        self.arena.push(0);
    }

    pub(crate) fn sorted(self, descending: bool) -> Vec<usize> {
        let mut order: Vec<_> = (0..self.starts.len()).collect();
        order.sort_unstable_by(|&a, &b| {
            let compared = unsafe {
                StrCmpLogicalW(
                    PCWSTR(self.arena[self.starts[a]..].as_ptr()),
                    PCWSTR(self.arena[self.starts[b]..].as_ptr()),
                )
            }
            .cmp(&0);
            let compared = if descending {
                compared.reverse()
            } else {
                compared
            };
            compared.then_with(|| a.cmp(&b))
        });
        order
    }
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
