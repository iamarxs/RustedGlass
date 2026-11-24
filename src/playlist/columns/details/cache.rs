//! The display cache and the all-rows fetch behind a metadata-column sort.
//!
//! Display keeps every read detail — the playlist's own rows bound it. A sort
//! by a metadata column is the one exception: it gathers every row's details
//! in bounded batches into a temporary map proportional to the row count,
//! released as soon as the sort has run.
//!
//! A row whose details the display cache already answers is never read again: a
//! sort adopts cached entries, and stat-only reads fill in without the header
//! when no shown column needs it.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::photo::{Column, Details, FileFacts, PhotoList};

mod sort_batch;

/// Rows per background batch: bounded work in flight, small enough to stay responsive.
pub(super) const BATCH: usize = 16;

/// True when the column's sort key needs the image header, not only the file stat.
pub(super) fn header_column(column: Column) -> bool {
    matches!(column, Column::Width | Column::Height | Column::Type)
}

/// An entry the display needs no further read for: blank records stay, stat-only
/// ones stay while no shown column reads the header.
pub(super) fn display_complete(read: &Details, header_columns: bool) -> bool {
    !header_columns || read.header_read()
}

/// The all-rows fetch behind a pending sort by a metadata column.
pub(super) struct SortFetch {
    revision: u64,
    /// True when the sort column's key comes from the image header.
    needs_header: bool,
    /// Every row's details, proportional to the row count; released after the sort.
    entries: HashMap<PathBuf, Details>,
    row_count: usize,
    /// First row position not sent for reading yet.
    next: usize,
    /// Where the in-flight batch started, so a partial reply rewinds the unread tail.
    sent_from: usize,
    /// The current batch of this fetch is on the lane.
    awaiting: bool,
}

impl SortFetch {
    pub(super) fn new(revision: u64, row_count: usize, needs_header: bool) -> Self {
        Self {
            revision,
            needs_header,
            entries: HashMap::new(),
            row_count,
            next: 0,
            sent_from: 0,
            awaiting: false,
        }
    }

    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    pub(super) fn entries(&self) -> &HashMap<PathBuf, Details> {
        &self.entries
    }

    pub(super) fn next_row(&self) -> usize {
        self.next
    }

    pub(super) fn complete(&self) -> bool {
        self.next >= self.row_count && !self.awaiting
    }

    /// A refused lane leaves the window unread; the caller rewinds `next`.
    pub(super) fn rewind(&mut self, next: usize) {
        self.next = next;
        self.awaiting = false;
    }

    /// Takes a batch's reply: every delivered entry counts, and an incomplete batch
    /// rewinds `next` so the rows it never reached are requested again — the delivered
    /// ones are already adopted, so only the unread tail is re-sent.
    pub(super) fn absorb(&mut self, entries: &[(PathBuf, Details)], complete: bool) {
        for (path, details) in entries {
            self.entries.insert(path.clone(), details.clone());
        }
        self.awaiting = false;
        if !complete {
            self.next = self.sent_from;
        }
    }
}

#[derive(Default)]
pub(super) struct Cache {
    revision: Option<u64>,
    layout: Option<u64>,
    row_count: usize,
    entries: HashMap<PathBuf, Details>,
    in_flight: Option<u64>,
    prefetch_next: usize,
    prefetch_end: Option<usize>,
    sort: Option<SortFetch>,
    prefetch_header: bool,
}

impl Cache {
    pub(super) fn sync(&mut self, photos: &PhotoList) {
        let revision = photos.rows_token();
        if self.revision == Some(revision) {
            return;
        }
        if photos.len() < self.row_count {
            let paths: HashSet<_> = photos.files.iter().map(PathBuf::as_path).collect();
            self.entries
                .retain(|path, _| paths.contains(path.as_path()));
        }
        self.row_count = photos.len();
        let layout = photos.layout_token();
        if self.layout != Some(layout) {
            self.prefetch_next = 0;
            self.layout = Some(layout);
        }
        if self
            .sort
            .as_ref()
            .is_some_and(|fetch| fetch.revision != revision)
        {
            self.sort = None;
        }
        self.revision = Some(revision);
    }

    pub(super) fn reset(&mut self) {
        self.revision = None;
        self.layout = None;
        self.row_count = 0;
        self.prefetch_next = 0;
        self.prefetch_header = false;
        self.entries = HashMap::new();
        self.sort = None;
        // Keep the outstanding request's marker until its old reply frees the lane.
    }

    /// Seeds just-published rows with their enumeration facts, so the Size and Date
    /// columns answer without further reads. Richer entries already held stay.
    pub(super) fn seed(&mut self, photos: &PhotoList, entries: Vec<(PathBuf, FileFacts)>) {
        self.sync(photos);
        for (path, facts) in entries {
            if let Some(held) = self.entries.get_mut(path.as_path()) {
                if held.facts() != Some(facts) {
                    *held = Details::from_facts(facts);
                }
            } else {
                self.insert(path, Details::from_facts(facts));
            }
        }
    }

    /// A header read upgrades a stat-only or seeded entry; a later stat-only read
    /// never downgrades.
    fn insert(&mut self, path: PathBuf, details: Details) {
        match self.entries.get_mut(path.as_path()) {
            Some(held) => {
                if details.header_read() && !held.header_read() {
                    *held = details;
                }
            }
            None => {
                self.entries.insert(path, details);
            }
        }
    }

    pub(super) fn record(
        &mut self,
        revision: u64,
        current: u64,
        entries: Vec<(PathBuf, Details)>,
        complete: bool,
    ) -> bool {
        if self.in_flight != Some(revision) {
            return false;
        }
        self.in_flight = None;
        let prefetch_end = self.prefetch_end.take();
        if self.revision == Some(revision) && current == revision {
            // An empty or cut-short reply keeps the pass before the unread rows; the
            // delivered entries below make the next pass skip what did arrive.
            if complete {
                self.prefetch_next = prefetch_end.unwrap_or(self.prefetch_next);
            }
            if let Some(fetch) = self
                .sort
                .as_mut()
                .filter(|fetch| fetch.revision == revision && fetch.awaiting)
            {
                fetch.absorb(&entries, complete);
            }
            for (path, details) in entries {
                self.insert(path, details);
            }
        }
        // A stale result can still free the lane for the current rows.
        true
    }

    pub(super) fn entries(&self) -> &HashMap<PathBuf, Details> {
        &self.entries
    }

    pub(super) fn revision(&self) -> Option<u64> {
        self.revision
    }

    pub(super) fn in_flight(&self) -> Option<u64> {
        self.in_flight
    }

    pub(super) fn set_in_flight(&mut self, revision: u64) {
        self.in_flight = Some(revision);
    }

    /// Visit every row once in metadata traversal order.
    pub(super) fn prefetch(
        &mut self,
        photos: &PhotoList,
        header_columns: bool,
    ) -> (usize, Vec<(PathBuf, Option<FileFacts>)>) {
        if header_columns && !self.prefetch_header {
            self.prefetch_next = 0;
        }
        self.prefetch_header = header_columns;
        let mut end = self.prefetch_next;
        let mut items = Vec::new();
        while end < photos.len() && items.len() < BATCH {
            if let Some(path) = photos.metadata_path_at(end).or_else(|| photos.path_at(end)) {
                let read = self.entries.get(path.as_path());
                let incomplete = read.map_or(true, |read| !display_complete(read, header_columns));
                if incomplete && !items.iter().any(|(asked, _)| asked == path) {
                    items.push((path.clone(), read.and_then(Details::facts)));
                }
            }
            end += 1;
        }
        if items.is_empty() {
            self.prefetch_next = end;
        }
        (end, items)
    }

    pub(super) fn set_prefetch_in_flight(&mut self, revision: u64, end: usize) {
        self.set_in_flight(revision);
        self.prefetch_end = Some(end);
    }

    pub(super) fn sort_reads_header(&self) -> bool {
        self.sort.as_ref().is_some_and(|fetch| fetch.needs_header)
    }

    pub(super) fn sort(&self) -> Option<&SortFetch> {
        self.sort.as_ref()
    }

    pub(super) fn sort_mut(&mut self) -> Option<&mut SortFetch> {
        self.sort.as_mut()
    }

    pub(super) fn set_sort(&mut self, fetch: Option<SortFetch>) {
        self.sort = fetch;
    }
}

#[cfg(test)]
mod tests;
