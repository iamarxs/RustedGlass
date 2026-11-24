use super::*;
use std::collections::HashMap;
use std::ops::Range;

type FolderRow = (PathBuf, photo::FileFacts);
pub(crate) type FolderFailure = (photo::FolderScanError, Source, bool);

pub(in crate::state) struct FolderRetry {
    source: Source,
    replacing: bool,
    partial: Option<Range<usize>>,
    revision: u64,
    progress: Option<(u64, Vec<FolderRow>)>,
}

impl State {
    pub(super) fn retrying_folder_scan(&self) -> bool {
        self.folder_retry
            .as_ref()
            .is_some_and(|retry| retry.progress.is_some())
    }

    pub fn can_retry_folder_scan(&self) -> bool {
        !self.importing()
            && self.folder_retry.as_ref().is_some_and(|retry| {
                retry.progress.is_none() && retry.revision == self.photos.rows_token()
            })
    }

    pub fn retry_folder_scan(&mut self) {
        if !self.can_retry_folder_scan() {
            return;
        }
        let mut retry = self.folder_retry.take().expect("retry was available");
        if self.submit_import(retry.source.clone(), retry.replacing) {
            retry.progress = self.pending_import.map(|tag| (tag, Vec::new()));
            self.notice = Some("Retrying folder scan…".into());
        }
        self.folder_retry = Some(retry);
    }

    pub(super) fn stage_retry_rows(
        &mut self,
        tag: u64,
        rows: Vec<FolderRow>,
    ) -> Result<(), Vec<FolderRow>> {
        let Some((_, staged)) = self
            .folder_retry
            .as_mut()
            .and_then(|retry| retry.progress.as_mut())
            .filter(|(retry_tag, _)| *retry_tag == tag)
        else {
            return Err(rows);
        };
        staged.extend(rows);
        Ok(())
    }

    pub(super) fn complete_folder_scan(
        &mut self,
        tag: u64,
        order: Vec<usize>,
        seed: Option<FolderSeed>,
        failure: Option<FolderFailure>,
    ) {
        if self
            .folder_retry
            .as_ref()
            .and_then(|retry| retry.progress.as_ref())
            .is_some_and(|(retry_tag, _)| *retry_tag == tag)
        {
            self.complete_folder_retry(order, seed, failure);
            return;
        }
        let start = self.import_preview.map(|(_, start)| start);
        let empty = order.is_empty() && start.is_none();
        self.finish_folder(order, seed);
        if let Some((error, source, replacing)) = failure {
            self.folder_retry = Some(FolderRetry {
                source,
                replacing,
                partial: start.map(|start| start..self.photos.len()),
                revision: self.photos.rows_token(),
                progress: None,
            });
            self.notice = Some(scan_notice(&error));
        } else if empty {
            self.notice = Some("No images in this folder.".into());
        }
    }

    fn complete_folder_retry(
        &mut self,
        order: Vec<usize>,
        seed: Option<FolderSeed>,
        failure: Option<FolderFailure>,
    ) {
        let mut retry = self.folder_retry.take().expect("folder retry in progress");
        let (_, rows) = retry.progress.take().expect("folder retry rows");
        if retry.revision != self.photos.rows_token() {
            self.notice =
                Some("The playlist changed during the retry. Open the folder again.".into());
            return;
        }
        let mut rows: Vec<_> = rows.into_iter().map(Some).collect();
        let mut discovered: Vec<_> = order
            .into_iter()
            .filter_map(|row| rows[row].take().map(|(path, facts)| (path, Some(facts))))
            .collect();
        if let (
            Some(FolderSeed::InsertAt(position)),
            Source::Folder {
                seed: Some(path), ..
            },
        ) = (seed, &retry.source)
        {
            discovered.insert(position, (path.clone(), None));
        }
        let mut range = retry.partial.clone().unwrap_or_else(|| {
            let len = self.photos.len();
            if retry.replacing {
                0..len
            } else {
                len..len
            }
        });

        if let Some((error, _, _)) = failure {
            self.notice = Some(scan_notice(&error));
            if let Some(partial) = &retry.partial {
                let mut previous = HashMap::new();
                for path in self
                    .photos
                    .visible_rows()
                    .skip(partial.start)
                    .take(partial.len())
                {
                    *previous.entry(path).or_insert(0usize) += 1;
                }
                discovered.retain(|(path, _)| {
                    let Some(count) = previous.get_mut(path).filter(|count| **count > 0) else {
                        return true;
                    };
                    *count -= 1;
                    false
                });
                range = partial.end..partial.end;
            }
            if !discovered.is_empty() {
                let start = retry
                    .partial
                    .as_ref()
                    .map_or(range.start, |partial| partial.start);
                retry.partial = Some(start..range.start + discovered.len());
                self.splice_folder_rows(range, discovered);
            }
            retry.revision = self.photos.rows_token();
            self.folder_retry = Some(retry);
        } else {
            if !discovered.is_empty() || retry.partial.is_some() {
                self.splice_folder_rows(range, discovered);
            }
            self.notice = None;
        }
    }

    fn splice_folder_rows(
        &mut self,
        range: Range<usize>,
        added: Vec<(PathBuf, Option<photo::FileFacts>)>,
    ) {
        let current = self.photos.current().cloned();
        let selected = self.photos.index;
        let mut paths: Vec<_> = self.photos.visible_rows().cloned().collect();
        let occurrence = current
            .as_ref()
            .filter(|_| range.contains(&selected))
            .map(|path| {
                paths[range.start..selected]
                    .iter()
                    .filter(|entry| *entry == path)
                    .count()
            });
        let mut facts = Vec::new();
        let new_paths: Vec<_> = added
            .into_iter()
            .map(|(path, known)| {
                if let Some(known) = known {
                    facts.push((path.clone(), known));
                }
                path
            })
            .collect();
        let index = if current.is_none() {
            range.start
        } else if selected < range.start {
            selected
        } else if selected >= range.end {
            selected - range.len() + new_paths.len()
        } else {
            let matches = new_paths
                .iter()
                .enumerate()
                .filter(|(_, path)| Some(*path) == current.as_ref());
            range.start
                + matches
                    .clone()
                    .nth(occurrence.unwrap_or(0))
                    .or_else(|| matches.into_iter().next())
                    .map_or(0, |(row, _)| row)
        };
        paths.splice(range, new_paths);
        let mut photos = PhotoList::empty();
        photos.index = index.min(paths.len().saturating_sub(1));
        photos.files = paths;
        photos.formats = self.photos.formats.clone();
        self.photos = photos;
        crate::playlist::reset_metadata();
        crate::playlist::seed_metadata(&self.photos, facts);
        if self.photos.current() != current.as_ref() {
            self.load_current();
        }
    }
}

fn scan_notice(error: &photo::FolderScanError) -> String {
    format!("{error} Choose Retry folder scan from the context menu.")
}

#[cfg(test)]
mod tests;
