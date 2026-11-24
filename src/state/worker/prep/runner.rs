use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::{remote::ask_remote, *};

/// The preparation lane: one source at a time, in the order requested.
pub(in crate::state::worker) fn lane(
    requests: &Lane<PrepRequest>,
    events: &Lane<Event>,
    formats: &[String],
    wake: &Wake,
    latest_replace: &AtomicU64,
    selected_loading: &AtomicBool,
) {
    const PREVIEW_ROWS: usize = 16;
    while let Some(request) = requests.pop() {
        let (tag, replacing, source) = match request {
            PrepRequest::Append { tag, source } => (tag, false, source),
            PrepRequest::Replace { tag, source } => (tag, true, source),
            PrepRequest::Inspect { revision, path } => {
                let text = crate::metadata::describe(&path);
                wake.deliver(
                    events,
                    Event::Inspected {
                        revision,
                        path,
                        text,
                    },
                );
                continue;
            }
            PrepRequest::Deduplicate {
                replace_tag,
                revision,
                paths,
            } => {
                let active =
                    || requests.is_open() && latest_replace.load(Ordering::Acquire) <= replace_tag;
                let rows = photo::duplicate_rows(paths, &active);
                if active() {
                    wake.deliver(
                        events,
                        Event::Deduplicated {
                            replace_tag,
                            revision,
                            rows,
                        },
                    );
                }
                continue;
            }
            PrepRequest::Metadata {
                revision,
                paths,
                read_header,
                background_header,
            } => {
                let requested = paths.len();
                let mut entries = Vec::with_capacity(paths.len());
                for (path, facts) in paths {
                    if requests.has_waiting()
                        || (background_header && selected_loading.load(Ordering::Acquire))
                    {
                        // One file read cannot be interrupted. Yield between reads and
                        // let the UI retry the batch after the foreground work.
                        break;
                    }
                    let details = photo::Details::read_with_facts(&path, facts, read_header);
                    entries.push((path, details));
                }
                let complete = entries.len() == requested;
                wake.deliver(
                    events,
                    Event::Metadata {
                        revision,
                        entries,
                        complete,
                    },
                );
                continue;
            }
        };
        let active = || requests.is_open() && latest_replace.load(Ordering::Acquire) <= tag;
        if !active() {
            continue;
        }
        if let Some(folder) = progressive::folder_source(&source) {
            progressive::scan(folder, formats, &active, replacing, tag, events, wake);
            continue;
        }
        let mut decision = None;
        let mut remote = || *decision.get_or_insert_with(|| ask_remote(tag, events, wake, &active));
        let mut emit = |files| wake.deliver(events, Event::ImportRows { tag, files });
        let mut preview = Vec::new();
        let mut seen = 0;
        let mut found = |path: &Path| {
            if seen < PREVIEW_ROWS {
                seen += 1;
                preview.push(path.to_path_buf());
                if seen == 1 || seen == PREVIEW_ROWS {
                    wake.deliver(
                        events,
                        Event::ImportPreview {
                            tag,
                            replacing,
                            files: std::mem::take(&mut preview),
                        },
                    );
                }
            }
        };
        let ordered = !replacing || matches!(source, Source::PlaylistFile { .. });
        let result = Scan {
            active: &active,
            remote: &mut remote,
            emit: if ordered { Some(&mut emit) } else { None },
            found: &mut found,
        }
        .prepare(&source, formats);
        if !active() {
            continue;
        }
        let event = match result {
            Ok(mut rows) => {
                // A replacing batch arrives in Explorer's name order, as the synchronous
                // picker's `PhotoList::from_files` does.
                if replacing && matches!(source, Source::Batch { .. }) {
                    photo::sort_names(&mut rows.files, false, None);
                }
                let mut files = std::mem::take(&mut rows.files).into_iter();
                while active() && files.len() > IMPORT_BATCH {
                    let files = files.by_ref().take(IMPORT_BATCH).collect();
                    wake.deliver(events, Event::ImportRows { tag, files });
                }
                if !active() {
                    continue;
                }
                rows.files.extend(files);
                if replacing {
                    Event::Replaced { tag, rows }
                } else {
                    Event::Appended { tag, rows }
                }
            }
            Err(error) => Event::PrepFailed { tag, error },
        };
        wake.deliver(events, event);
    }
}
