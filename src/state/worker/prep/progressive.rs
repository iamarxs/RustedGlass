use std::time::{Duration, Instant};

use super::*;
use crate::state::worker::FolderSeed;

type FolderSource<'a> = (&'a [PathBuf], bool, bool, Option<&'a Path>);

pub(super) fn folder_source(source: &Source) -> Option<FolderSource<'_>> {
    match source {
        Source::Folder {
            path,
            include_hidden,
            recurse,
            seed,
        } => Some((
            std::slice::from_ref(path),
            *include_hidden,
            *recurse,
            seed.as_deref(),
        )),
        Source::Folders {
            paths,
            include_hidden,
            recurse,
        } => Some((paths, *include_hidden, *recurse, None)),
        Source::Batch {
            paths,
            include_hidden,
            recurse,
        } if !paths.is_empty() && paths.iter().all(|path| path.is_dir()) => {
            Some((paths, *include_hidden, *recurse, None))
        }
        _ => None,
    }
}

pub(super) fn scan(
    (roots, include_hidden, recurse, seed): FolderSource<'_>,
    formats: &[String],
    active: &dyn Fn() -> bool,
    replacing: bool,
    tag: u64,
    events: &Lane<Event>,
    wake: &Wake,
) {
    let mut names = photo::NameOrder::default();
    let mut batch = Vec::new();
    let mut delivered = false;
    let mut started = Instant::now();
    let emit = |rows| {
        wake.deliver(
            events,
            Event::FolderRows {
                tag,
                replacing,
                rows,
            },
        )
    };
    let per_root = !replacing && roots.len() > 1;
    let mut order = Vec::new();
    let mut error = None;
    let seed_identity = seed.map(photo::path_identity);
    let mut seed_row = None;
    let mut count = 0;
    for root in roots {
        let result = photo::visit_folder(
            root,
            include_hidden,
            recurse,
            formats,
            active,
            &mut |path, facts| {
                if seed_row.is_none()
                    && seed_identity
                        .as_ref()
                        .is_some_and(|seed| seed.file_name() == path.file_name())
                {
                    seed_row = Some(count);
                }
                count += 1;
                names.push(&path);
                batch.push((path, facts));
                if !delivered
                    || batch.len() == IMPORT_BATCH
                    || started.elapsed() >= Duration::from_millis(100)
                {
                    emit(std::mem::take(&mut batch));
                    delivered = true;
                    started = Instant::now();
                }
            },
        );
        // Keep the appending order of this root even when its enumeration failed.
        if per_root {
            let start = order.len();
            order.extend(
                std::mem::take(&mut names)
                    .sorted(false)
                    .into_iter()
                    .map(|row| start + row),
            );
        }
        if !active() {
            return;
        }
        if let Err(scan_error) = result {
            error = Some(scan_error);
            break;
        }
    }
    if !batch.is_empty() {
        emit(batch);
    }
    let mut seed_position = seed_row.map(FolderSeed::Discovered);
    if !per_root {
        let missing_seed = seed.filter(|_| seed_position.is_none());
        if let Some(path) = missing_seed {
            // The folder filter can exclude an explicitly opened image.
            names.push(path);
        }
        order = names.sorted(false);
        if missing_seed.is_some() {
            if let Some(position) = order.iter().position(|&row| row == count) {
                order.remove(position);
                seed_position = Some(FolderSeed::InsertAt(position));
            }
        }
    }
    if active() {
        let failure = error.map(|error| {
            let source = if let [path] = roots {
                Source::Folder {
                    path: path.clone(),
                    include_hidden,
                    recurse,
                    seed: seed.map(Path::to_path_buf),
                }
            } else {
                Source::Folders {
                    paths: roots.to_vec(),
                    include_hidden,
                    recurse,
                }
            };
            (error, source, replacing)
        });
        wake.deliver(
            events,
            Event::FolderFinished {
                tag,
                order,
                seed: seed_position,
                failure,
            },
        );
    }
}
