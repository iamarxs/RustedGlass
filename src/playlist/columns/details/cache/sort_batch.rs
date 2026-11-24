use super::*;

impl Cache {
    /// The next window of up to `BATCH` rows the sort fetch does not hold. A row whose
    /// cached details answer the sort column is adopted into the fetch map instead of
    /// being read again, so a re-sort of a known list costs no IO.
    pub(in crate::playlist::columns::details) fn sort_next_batch(
        &mut self,
        photos: &PhotoList,
    ) -> Vec<(PathBuf, Option<FileFacts>)> {
        let Cache { entries, sort, .. } = self;
        let Some(fetch) = sort.as_mut() else {
            return Vec::new();
        };
        fetch.sent_from = fetch.next;
        let mut items = Vec::new();
        while fetch.next < fetch.row_count && items.len() < BATCH {
            let position = fetch.next;
            fetch.next += 1;
            let Some(path) = photos.path_at(position) else {
                continue;
            };
            if fetch.entries.contains_key(path) {
                continue;
            }
            if let Some(read) = entries.get(path.as_path()) {
                let usable = if fetch.needs_header {
                    read.header_read()
                } else {
                    read.facts().is_some()
                };
                if usable {
                    fetch.entries.insert(path.clone(), read.clone());
                    continue;
                }
            }
            if items.iter().any(|(asked, _)| asked == path) {
                continue;
            }
            items.push((
                path.clone(),
                entries.get(path.as_path()).and_then(Details::facts),
            ));
        }
        if !items.is_empty() {
            fetch.awaiting = true;
        }
        items
    }
}
