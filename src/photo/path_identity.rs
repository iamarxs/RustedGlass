use super::*;
use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{FindClose, FindFirstFileW, WIN32_FIND_DATAW};

/// Resolve directory aliases and ask the directory for the spelling of the final entry.
/// Looking up the entry itself preserves separate hard links and final-component symlinks.
pub(crate) fn path_identity(path: &Path) -> PathBuf {
    PathIdentities::default().resolve(path)
}

#[derive(Default)]
pub(super) struct PathIdentities {
    parents: HashMap<PathBuf, PathBuf>,
}

impl PathIdentities {
    pub(super) fn resolve(&mut self, path: &Path) -> PathBuf {
        self.entry(path).0
    }

    fn entry(&mut self, path: &Path) -> (PathBuf, Option<OsString>) {
        let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
        let (Some(parent), Some(name)) = (absolute.parent(), absolute.file_name()) else {
            return (absolute, None);
        };
        let mut query: Vec<_> = absolute.as_os_str().encode_wide().collect();
        if name.encode_wide().any(|code| matches!(code, 0 | 42 | 63)) || query.contains(&0) {
            return (absolute, None);
        }
        query.push(0);
        let mut data = WIN32_FIND_DATAW::default();
        let Ok(find) = (unsafe { FindFirstFileW(PCWSTR(query.as_ptr()), &mut data) }) else {
            return (absolute, None);
        };
        let _ = unsafe { FindClose(find) };
        let canonical = self
            .parents
            .entry(parent.to_owned())
            .or_insert_with(|| parent.canonicalize().unwrap_or_else(|_| parent.to_owned()))
            .join(entry_name(&data.cFileName));
        let alternate =
            (data.cAlternateFileName[0] != 0).then(|| entry_name(&data.cAlternateFileName));
        (canonical, alternate)
    }
}

fn entry_name(buffer: &[u16]) -> OsString {
    let len = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
    OsString::from_wide(&buffer[..len])
}

fn ascii_name(name: &OsStr) -> Option<String> {
    let name = name.to_str()?.trim_end_matches(['.', ' ']);
    name.is_ascii().then(|| name.to_ascii_lowercase())
}

impl PhotoList {
    /// Resolve possible aliases before changing their directory entries. An ASCII long
    /// or short leaf cannot alias a different ASCII leaf, even through a parent junction.
    /// Non-ASCII names are passed through to Windows for its case rules.
    pub(crate) fn row_identities<'a>(
        &self,
        targets: impl IntoIterator<Item = &'a Path>,
    ) -> Vec<Option<PathBuf>> {
        let mut keys = HashMap::new();
        let mut parents = PathIdentities::default();
        let mut names = HashSet::new();
        let mut unfiltered = false;
        for path in targets {
            if keys.contains_key(path) {
                continue;
            }
            let (key, alternate) = parents.entry(path);
            for name in key.file_name().into_iter().chain(alternate.as_deref()) {
                if let Some(name) = ascii_name(name) {
                    names.insert(name);
                } else {
                    unfiltered = true;
                }
            }
            keys.insert(path.to_owned(), key);
        }
        self.visible_rows()
            .map(|path| {
                let possible = unfiltered
                    || path
                        .file_name()
                        .and_then(ascii_name)
                        .is_none_or(|name| names.contains(&name));
                possible.then(|| {
                    keys.entry(path.clone())
                        .or_insert_with(|| parents.resolve(path))
                        .clone()
                })
            })
            .collect()
    }

    pub(crate) fn rows_for_path(&self, path: &Path) -> Vec<usize> {
        let key = path_identity(path);
        self.row_identities([path])
            .into_iter()
            .enumerate()
            .filter_map(|(row, identity)| (identity.as_ref() == Some(&key)).then_some(row))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_case_variants_match_but_separate_directory_entries_do_not() {
        let dir = crate::test_path("path-identity-names");
        fs::create_dir_all(&dir).unwrap();
        let original = dir.join("Trip.png");
        let variant = dir.join("trip.png");
        let hard_link = dir.join("Hard.png");
        fs::write(&original, b"original").unwrap();
        fs::hard_link(&original, &hard_link).unwrap();
        assert_eq!(path_identity(&original), path_identity(&variant));
        assert_ne!(path_identity(&original), path_identity(&hard_link));
        for file in [original, hard_link] {
            fs::remove_file(file).unwrap();
        }
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn unresolved_names_are_not_folded_together() {
        let dir = crate::test_path("path-identity-missing");
        assert_ne!(
            path_identity(&dir.join("Image.png")),
            path_identity(&dir.join("image.png"))
        );
    }
}
