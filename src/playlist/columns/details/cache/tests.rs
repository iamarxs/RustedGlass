use std::collections::HashSet;

use super::*;

fn facts() -> FileFacts {
    FileFacts {
        size: 1,
        modified: 1,
    }
}

/// A cache-shaped reply entry: stat-only details, as a stat batch delivers.
fn entry(path: &PathBuf) -> (PathBuf, Details) {
    (path.clone(), Details::from_facts(facts()))
}

fn list_of(count: usize) -> PhotoList {
    let paths: Vec<PathBuf> = (0..count)
        .map(|i| PathBuf::from(format!("C:\\photos\\img{i:04}.png")))
        .collect();
    PhotoList::from_files(&paths, vec![".png".to_string()])
}

#[test]
fn interrupted_prefetch_retries_the_same_rows() {
    let photos = list_of(BATCH + 1);
    let revision = photos.rows_token();
    let mut cache = Cache::default();
    cache.sync(&photos);
    let (end, items) = cache.prefetch(&photos, false);
    cache.set_prefetch_in_flight(revision, end);
    // The worker stopped without attempting any row; the pass must not advance.
    assert!(cache.record(revision, revision, Vec::new(), false));
    assert_eq!(cache.prefetch(&photos, false), (end, items));
}

#[test]
fn a_partial_prefetch_reply_caches_its_rows_and_rewinds_the_pass() {
    let photos = list_of(BATCH + 2);
    let revision = photos.rows_token();
    let mut cache = Cache::default();
    cache.sync(&photos);
    let (end, items) = cache.prefetch(&photos, false);
    assert_eq!(items.len(), BATCH);
    let delivered: Vec<(PathBuf, Details)> = items[..2]
        .iter()
        .cloned()
        .map(|(path, _)| entry(&path))
        .collect();
    cache.set_prefetch_in_flight(revision, end);
    assert!(cache.record(revision, revision, delivered, false));

    // The delivered rows are valid partials and stay; the unread tail is asked again.
    let (_, again) = cache.prefetch(&photos, false);
    let asked: Vec<PathBuf> = again.into_iter().map(|(path, _)| path).collect();
    assert!(!asked.iter().any(|path| path == &items[0].0));
    assert!(!asked.iter().any(|path| path == &items[1].0));
    assert!(asked.iter().any(|path| path == &items[2].0));
    assert!(asked.iter().any(|path| path == &items[BATCH - 1].0));
}

#[test]
fn a_complete_reply_advances_the_prefetch_pass() {
    let photos = list_of(2 * BATCH);
    let revision = photos.rows_token();
    let mut cache = Cache::default();
    cache.sync(&photos);
    let (end, _) = cache.prefetch(&photos, false);
    assert_eq!(end, BATCH);
    cache.set_prefetch_in_flight(revision, end);
    assert!(cache.record(
        revision,
        revision,
        (0..BATCH)
            .filter_map(|row| photos.path_at(row))
            .map(entry)
            .collect(),
        true
    ));
    let (_, next) = cache.prefetch(&photos, false);
    assert!(
        next.iter()
            .any(|(path, _)| path == photos.path_at(BATCH).unwrap()),
        "the pass moved on past the read rows"
    );
    assert!(!next
        .iter()
        .any(|(path, _)| path == photos.path_at(0).unwrap()));
}

#[test]
fn sort_fetch_covers_all_rows_in_bounded_batches() {
    let photos = list_of(2 * BATCH + 3);
    let revision = photos.rows_token();
    let mut cache = Cache::default();
    cache.set_sort(Some(SortFetch::new(revision, photos.len(), true)));
    let mut seen = Vec::new();
    while !cache.sort().is_some_and(|fetch| fetch.complete()) {
        let batch = cache.sort_next_batch(&photos);
        assert!(batch.len() <= BATCH);
        let reply: Vec<(PathBuf, Details)> =
            batch.into_iter().map(|(path, _)| entry(&path)).collect();
        seen.extend(reply.iter().map(|(path, _)| path.clone()));
        cache.sort_mut().unwrap().absorb(&reply, true);
    }
    assert_eq!(seen.len(), photos.len());
    for path in photos.visible_rows() {
        assert!(cache.sort().unwrap().entries().contains_key(path));
    }
}

#[test]
fn a_partial_sort_reply_adopts_the_delivered_rows_and_retries_the_tail() {
    let photos = list_of(BATCH + 2);
    let revision = photos.rows_token();
    let mut cache = Cache::default();
    cache.set_sort(Some(SortFetch::new(revision, photos.len(), false)));
    let batch = cache.sort_next_batch(&photos);
    assert_eq!(batch.len(), BATCH);
    let partial: Vec<(PathBuf, Details)> = batch[..2]
        .iter()
        .cloned()
        .map(|(path, _)| entry(&path))
        .collect();
    cache.sort_mut().unwrap().absorb(&partial, false);

    let again = cache.sort_next_batch(&photos);
    let asked: Vec<PathBuf> = again.into_iter().map(|(path, _)| path).collect();
    assert!(!asked.iter().any(|path| path == &batch[0].0));
    assert!(!asked.iter().any(|path| path == &batch[1].0));
    assert!(asked.iter().any(|path| path == &batch[2].0));
    assert!(
        !cache.sort().unwrap().complete(),
        "the tail is still unread"
    );
}

#[test]
fn a_seeded_row_needs_no_read_for_a_stat_sort() {
    let photos = list_of(4);
    let revision = photos.rows_token();
    let mut cache = Cache::default();
    cache.sync(&photos);
    let seeds: Vec<(PathBuf, FileFacts)> = (0..2)
        .filter_map(|row| photos.path_at(row))
        .cloned()
        .map(|path| (path, facts()))
        .collect();
    cache.seed(&photos, seeds);
    cache.set_sort(Some(SortFetch::new(revision, photos.len(), false)));
    let batch = cache.sort_next_batch(&photos);
    let asked: Vec<PathBuf> = batch.into_iter().map(|(path, _)| path).collect();
    assert_eq!(asked.len(), 2, "the seeded rows were adopted, not asked");
    assert!(!asked.iter().any(|path| path == photos.path_at(0).unwrap()));
    assert!(!asked.iter().any(|path| path == photos.path_at(1).unwrap()));
}

#[test]
fn a_seeded_row_is_re_requested_for_a_header_sort() {
    let photos = list_of(2);
    let revision = photos.rows_token();
    let mut cache = Cache::default();
    cache.sync(&photos);
    cache.seed(
        &photos,
        photos
            .visible_rows()
            .cloned()
            .map(|path| (path, facts()))
            .collect(),
    );
    cache.set_sort(Some(SortFetch::new(revision, photos.len(), true)));
    // Stat-only seeds cannot answer Width; both rows are asked for header reads.
    let requested = cache.sort_next_batch(&photos);
    assert_eq!(requested.len(), photos.len());
    assert!(requested.iter().all(|(_, known)| *known == Some(facts())));
}

#[test]
fn showing_a_header_column_restarts_prefetch_with_the_existing_facts() {
    let photos = list_of(4);
    let mut cache = Cache::default();
    cache.seed(
        &photos,
        photos
            .visible_rows()
            .cloned()
            .map(|path| (path, facts()))
            .collect(),
    );
    assert!(cache.prefetch(&photos, false).1.is_empty());
    let (_, requests) = cache.prefetch(&photos, true);
    assert_eq!(requests.len(), photos.len());
    assert!(requests.iter().all(|(_, known)| *known == Some(facts())));
}

#[test]
fn a_display_reply_recorded_while_a_sort_exists_is_adopted_by_the_sort_fetch() {
    let photos = list_of(4);
    let revision = photos.rows_token();
    let mut cache = Cache::default();
    cache.sync(&photos);
    cache.set_sort(Some(SortFetch::new(revision, photos.len(), true)));
    // A visible-page batch lands while the sort exists but is not awaiting: record
    // keeps it in the display cache, and the sort's next window adopts it instead of
    // reading those rows again.
    let visible: Vec<(PathBuf, Details)> = (0..2)
        .filter_map(|row| photos.path_at(row))
        .map(|path| {
            (
                path.clone(),
                Details::read_with_facts(path, Some(facts()), true),
            )
        })
        .collect();
    cache.set_in_flight(revision);
    assert!(cache.record(revision, revision, visible, true));
    let batch = cache.sort_next_batch(&photos);
    let asked: Vec<&PathBuf> = batch.iter().map(|(path, _)| path).collect();
    assert_eq!(
        asked.len(),
        2,
        "the two read rows were adopted, not re-asked"
    );
    assert!(asked.contains(&photos.path_at(2).unwrap()));
    assert!(asked.contains(&photos.path_at(3).unwrap()));
}

#[test]
fn refused_batch_is_rewound_and_retried() {
    let photos = list_of(BATCH + 1);
    let revision = photos.rows_token();
    let mut cache = Cache::default();
    cache.set_sort(Some(SortFetch::new(revision, photos.len(), false)));
    let before = cache.sort().unwrap().next_row();
    let batch = cache.sort_next_batch(&photos);
    assert_eq!(batch.len(), BATCH);
    cache.sort_mut().unwrap().rewind(before);
    assert!(!cache.sort().unwrap().awaiting);
    assert_eq!(cache.sort().unwrap().next_row(), before);
    assert_eq!(
        cache.sort_next_batch(&photos),
        batch,
        "the same window is retried"
    );
}

#[test]
fn display_cache_retains_metadata_past_the_old_bound_without_rereading_rows() {
    // One batch past the retired 50,000 cap: every read detail must stay, and
    // the prefetch pass must finish without asking any row twice.
    let photos = list_of(50_000 + BATCH);
    let revision = photos.rows_token();
    let mut cache = Cache::default();
    cache.sync(&photos);
    let mut asked: HashSet<PathBuf> = HashSet::new();
    loop {
        let (end, items) = cache.prefetch(&photos, false);
        if items.is_empty() {
            let (_, again) = cache.prefetch(&photos, false);
            assert!(again.is_empty(), "the finished pass reads nothing more");
            break;
        }
        cache.set_prefetch_in_flight(revision, end);
        for (path, _) in &items {
            assert!(asked.insert(path.clone()), "no row is read twice");
        }
        let reply: Vec<(PathBuf, Details)> = items.iter().map(|(path, _)| entry(path)).collect();
        assert!(cache.record(revision, revision, reply, true));
    }
    assert_eq!(
        cache.entries.len(),
        photos.len(),
        "every detail past the old cap is retained"
    );
    assert!(cache
        .entries
        .contains_key(photos.path_at(0).unwrap().as_path()));
    assert!(cache
        .entries
        .contains_key(photos.path_at(photos.len() - 1).unwrap().as_path()));
}

#[test]
fn a_header_read_upgrades_a_seeded_entry_and_is_not_downgraded_afterwards() {
    let path = crate::test_path("playlist-columns-header.png");
    image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 255]))
        .save(&path)
        .unwrap();
    let photos = PhotoList::from_files(&[path.clone()], vec![".png".to_string()]);
    let revision = photos.rows_token();
    let mut cache = Cache::default();
    cache.sync(&photos);
    cache.seed(&photos, vec![(path.clone(), facts())]);
    assert!(
        cache
            .entries
            .get(path.as_path())
            .is_some_and(|read| read.facts().is_some()),
        "the seed answers the stat columns"
    );
    assert!(
        !display_complete(cache.entries.get(path.as_path()).unwrap(), true),
        "a shown header column still asks for the header"
    );

    cache.set_in_flight(revision);
    assert!(cache.record(
        revision,
        revision,
        vec![(path.clone(), Details::read_with_facts(&path, None, true))],
        true
    ));
    assert!(
        cache
            .entries
            .get(path.as_path())
            .is_some_and(|read| read.header_read()),
        "the header read replaced the seed"
    );

    cache.set_in_flight(revision);
    assert!(cache.record(
        revision,
        revision,
        vec![(path.clone(), Details::from_facts(facts()))],
        true
    ));
    assert!(
        cache
            .entries
            .get(photos.path_at(0).unwrap().as_path())
            .is_some_and(|read| read.header_read()),
        "a stat read cannot downgrade a header-read entry"
    );
    std::fs::remove_file(&path).ok();
}

#[test]
fn stale_result_frees_the_lane_but_is_not_cached() {
    let photos = list_of(4);
    let mut cache = Cache::default();
    cache.sync(&photos);
    let revision = photos.rows_token();
    cache.set_in_flight(revision);
    let entries: Vec<(PathBuf, Details)> = photos
        .visible_rows()
        .cloned()
        .map(|path| entry(&path))
        .collect();
    // A newer row set arrived before the reply: the reply must not fill the cache.
    assert!(cache.record(revision, revision + 1, entries, true));
    assert!(cache.entries.is_empty());
    assert_eq!(
        cache.in_flight(),
        None,
        "the lane is free for the next batch"
    );
}

#[test]
fn removing_rows_releases_details_only_for_paths_that_left_the_playlist() {
    let mut photos = list_of(2);
    let kept = photos.path_at(0).unwrap().clone();
    let removed = photos.path_at(1).unwrap().clone();
    photos.extend_files(vec![kept.clone()]);
    let mut cache = Cache::default();
    cache.seed(
        &photos,
        vec![(kept.clone(), facts()), (removed.clone(), facts())],
    );

    photos.remove_rows(&[0, 1]);
    cache.sync(&photos);
    assert!(cache.entries.contains_key(&kept));
    assert!(!cache.entries.contains_key(&removed));
    assert!(cache.prefetch(&photos, false).1.is_empty());
}
