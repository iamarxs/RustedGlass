use super::*;

#[test]
fn adding_a_folders_images_appends_one_entry_each_and_keeps_the_view() {
    let dir = crate::test_path("rustedglass-add-folder");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("x.png"), b"P6\n1 1\n255\n\xff\x00\x00").unwrap();
    std::fs::write(dir.join("notes.txt"), b"text").unwrap();

    let mut state = State::new(crate::test_path("unused-config.json"), &[]);
    state.photos.files = vec![dir.join("mine.png")];
    assert_eq!(state.add_folder(&dir), 1);
    assert_eq!(state.photos.files.len(), 2);
    assert_eq!(state.photos.current(), Some(&dir.join("mine.png")));

    assert_eq!(
        state.add_folder(&dir),
        1,
        "re-adding the folder repeats its images"
    );
    state.remove_current();
    assert_eq!(
        state.photos.files,
        vec![dir.join("x.png"), dir.join("x.png")]
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn importing_decodes_the_current_image_and_defers_unseen_files() {
    let dir = import_test_dir("batch");
    let bad = dir.join("b-corrupt.png");
    let good = dir.join("a-valid.ppm");
    std::fs::write(&bad, b"broken").unwrap();
    std::fs::write(&good, b"P6\n1 1\n255\n\xff\x00\x00").unwrap();
    let mut state = State::new(dir.join("rgconfig.json"), &[]);

    assert_eq!(
        state.add_files(&[good.clone(), bad.clone(), good.clone()]),
        3
    );
    assert_eq!(
        state.photos.files,
        [good.clone(), bad.clone(), good.clone()]
    );
    assert!(state.notice.is_none());
    state.open_folder(&dir);
    assert_eq!(state.photos.files, [good.clone(), bad.clone()]);
    assert!(state.notice.is_none());
    assert_eq!(state.add_files(std::slice::from_ref(&good)), 1);
    state.photos.files.push(bad.clone());
    state.photos.index = 1;
    state.load_current();
    // an unreadable entry stays in the playlist and only produces a notice
    assert_eq!(
        state.photos.files,
        [good.clone(), bad.clone(), good.clone(), bad.clone()]
    );
    assert_eq!(state.photos.index, 1);
    assert!(state
        .notice
        .as_deref()
        .unwrap()
        .contains("b-corrupt.png is unreadable"));
    assert!(state.frame().is_none());

    std::fs::remove_file(bad).unwrap();
    std::fs::remove_file(good).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn opening_a_playlist_reports_missing_files_without_decoding_unseen_rows() {
    let dir = import_test_dir("playlist");
    let bad = dir.join("corrupt.png");
    let good = dir.join("valid.ppm");
    let playlist = dir.join("images.rgpl");
    std::fs::write(&bad, b"broken").unwrap();
    std::fs::write(&good, b"P6\n1 1\n255\n\xff\x00\x00").unwrap();
    std::fs::write(
        &playlist,
        "missing.png\nvalid.ppm\ncorrupt.png\nmissing2.jpg\nvalid.ppm\n",
    )
    .unwrap();
    let mut state = State::new(dir.join("rgconfig.json"), &[]);

    assert_eq!(state.open_playlist(&playlist), Ok(3));
    assert_eq!(
        state.photos.files,
        [good.clone(), bad.clone(), good.clone()]
    );
    assert_eq!(
        state.notice.as_deref(),
        Some("Playlist skipped 2 missing image(s).")
    );

    for file in [bad, good, playlist] {
        std::fs::remove_file(file).unwrap();
    }
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn pressing_m_moves_the_file_and_drops_its_entry() {
    let from = crate::test_path("rustedglass-move-from");
    let to = crate::test_path("rustedglass-move-to");
    std::fs::create_dir_all(&from).unwrap();
    std::fs::create_dir_all(&to).unwrap();
    std::fs::write(from.join("a.png"), b"P6\n1 1\n255\n\xff\x00\x00").unwrap();
    std::fs::write(from.join("b.png"), b"P6\n1 1\n255\n\xff\x00\x00").unwrap();

    let mut state = State::new(crate::test_path("unused-config.json"), &[]);
    state.open_folder(&from);
    assert_eq!(state.photos.len(), 2);

    // No folder set: the file stays where it is.
    assert!(state.move_current_to_target().is_err());
    assert_eq!(state.photos.len(), 2);

    state.knobs.move_to = to.join("missing");
    assert!(state.move_current_to_target().is_err());
    assert_eq!(state.photos.current(), Some(&from.join("a.png")));
    assert_eq!(state.photos.len(), 2);
    assert!(from.join("a.png").is_file());

    state.knobs.move_to = to.clone();
    let moved = state.move_current_to_target().unwrap();
    assert_eq!(moved, to.join("a.png"));
    assert!(to.join("a.png").exists());
    assert!(!from.join("a.png").exists());
    assert_eq!(
        state.photos.files,
        vec![from.join("b.png")],
        "gone from the playlist"
    );
    assert_eq!(state.photos.index, 0);

    std::fs::remove_dir_all(&from).ok();
    std::fs::remove_dir_all(&to).ok();
}

#[test]
fn deleting_the_shown_file_removes_all_of_its_rows() {
    let dir = crate::test_path("rustedglass-delete");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.png"), b"P6\n1 1\n255\n\xff\x00\x00").unwrap();

    let mut state = State::new(crate::test_path("unused-config.json"), &[]);
    state.open_folder(&dir);
    assert_eq!(state.add_files(&[dir.join("a.png")]), 1);
    assert_eq!(state.delete_current().unwrap(), dir.join("a.png"));
    assert!(state.photos.is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn configured_shuffle_keeps_folder_and_playlist_rows_in_order() {
    let dir = import_test_dir("shuffle-import");
    let files = ["a.ppm", "b.ppm", "c.ppm"].map(|name| test_image(&dir, name));
    let playlist = dir.join("images.rgpl");
    std::fs::write(&playlist, "c.ppm\na.ppm\nb.ppm\n").unwrap();
    let config = dir.join("rgconfig.json");
    let shuffle = "-p:EnableRandomDisplayOrder=true".to_owned();
    let assert_order = |state: &mut State, first: &Path| {
        assert!(state.photos.order.is_empty());
        assert_eq!(state.photos.index, 0);
        assert_eq!(state.photos.current().map(PathBuf::as_path), Some(first));
        assert!(state.knobs.random_order);
        assert!(state.step(1));
        assert_ne!(state.photos.index, 0);
        assert!(state.photos.order.is_empty());
    };

    let mut state = State::new(config.clone(), std::slice::from_ref(&shuffle));
    state.open_folder(&dir);
    assert_order(&mut state, &files[0]);
    state.open_playlist(&playlist).unwrap();
    assert_order(&mut state, &files[2]);

    for (path, first) in [(&dir, &files[0]), (&playlist, &files[2])] {
        let mut state = State::new(
            config.clone(),
            &[path.display().to_string(), shuffle.clone()],
        );
        assert_order(&mut state, first);
    }
    for file in files.into_iter().chain([playlist]) {
        std::fs::remove_file(file).unwrap();
    }
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn opening_an_image_limits_the_playlist_to_its_immediate_folder() {
    let dir = import_test_dir("single-image-scope");
    let subfolder = dir.join("subfolder");
    std::fs::create_dir(&subfolder).unwrap();
    let peer = test_image(&dir, "a-peer.ppm");
    let selected = test_image(&dir, "b-selected.ppm");
    let nested = test_image(&subfolder, "c-nested.ppm");
    let config = dir.join("rgconfig.json");
    let recurse = "-p:EnableSubfoldersLoading=true".to_string();

    let args = [selected.display().to_string(), recurse.clone()];
    let mut state = State::new(config.clone(), &args);
    assert_eq!(state.photos.files, [peer.clone(), selected.clone()]);
    assert_eq!(state.photos.current(), Some(&selected));

    let all_images = [peer, selected, nested];
    state.open_files(std::slice::from_ref(&dir));
    assert_eq!(state.photos.files, all_images);
    let state = State::new(config, &[dir.display().to_string(), recurse]);
    assert_eq!(state.photos.files, all_images);

    for path in all_images {
        std::fs::remove_file(path).unwrap();
    }
    std::fs::remove_dir(subfolder).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn navigation_stays_in_the_opened_folder_with_the_legacy_setting_enabled() {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN};

    let dir = import_test_dir("folder-settings");
    let current = dir.join("a");
    let sibling = dir.join("b");
    let nested = sibling.join("nested");
    std::fs::create_dir(&current).unwrap();
    std::fs::create_dir_all(&nested).unwrap();
    let shown = test_image(&current, "shown.ppm");
    let hidden = test_image(&current, "hidden.ppm");
    let next = test_image(&nested, "next.ppm");
    let hidden_name = crate::w32::wide(&hidden.display().to_string());
    unsafe {
        SetFileAttributesW(PCWSTR(hidden_name.as_ptr()), FILE_ATTRIBUTE_HIDDEN).unwrap();
    }

    let args = [
        current.display().to_string(),
        "-p:EnableSubfoldersLoading=true".into(),
        "-p:EnableAutoSwitchSiblingDir=true".into(),
        "-p:EnableLoopBackNavigation=false".into(),
    ];
    let mut state = State::new(dir.join("rgconfig.json"), &args);
    assert!(!state.step(1));
    assert_eq!(state.photos.current(), Some(&shown));
    assert_eq!(state.photos.files, [shown.clone()]);
    for file in [shown, hidden, next] {
        std::fs::remove_file(file).unwrap();
    }
    for folder in [current, nested, sibling, dir] {
        std::fs::remove_dir(folder).unwrap();
    }
}

#[test]
fn dropping_playlist_files_appends_or_replaces_like_other_drops() {
    let dir = import_test_dir("drop-rgpl");
    let a = dir.join("a.ppm");
    let b = dir.join("b.ppm");
    let c = dir.join("c.ppm");
    for file in [&a, &b, &c] {
        std::fs::write(file, b"P6\n1 1\n255\n\xff\x00\x00").unwrap();
    }
    let list1 = dir.join("one.rgpl");
    let list2 = dir.join("two.rgplz");
    crate::rgpl::write_file(&list1, &[a.clone()]).unwrap();
    crate::rgpl::write_file(&list2, &[b.clone(), c.clone()]).unwrap();

    let mut state = State::new(dir.join("rgconfig.json"), &[]);

    // playlist view: both dropped playlists load and append to the current list
    state.photos.files = vec![dir.join("mine.png")];
    state.append_drop(vec![list1.clone(), list2.clone()]);
    assert_eq!(
        state.photos.files,
        [dir.join("mine.png"), a.clone(), b.clone(), c.clone()]
    );

    // image view: a dropped playlist replaces the list, and is not "unsupported"
    state.open_files(&[list1.clone()]);
    assert_eq!(state.photos.files, [a.clone()]);
    assert!(state.notice.is_none());

    for file in [a, b, c, list1, list2] {
        std::fs::remove_file(file).unwrap();
    }
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn directory_entries_are_refused_by_direct_adds_drops_and_playlists() {
    let dir = import_test_dir("reject-directories");
    let fake = dir.join("fake.png");
    std::fs::create_dir(&fake).unwrap();
    let good = test_image(&dir, "real.ppm");
    let playlist = dir.join("list.rgpl");
    std::fs::write(&playlist, "fake.png\nreal.ppm\n").unwrap();

    let mut state = State::new(dir.join("rgconfig.json"), &[]);
    assert_eq!(state.add_files(&[fake.clone(), good.clone()]), 1);
    assert_eq!(state.photos.files, [good.clone()]);
    assert_eq!(
        state.notice.as_deref(),
        Some("Skipped 1 unsupported image(s).")
    );

    state.notice = None;
    assert_eq!(state.open_playlist(&playlist), Ok(1));
    assert_eq!(state.photos.files, [good.clone()]);
    assert_eq!(
        state.notice.as_deref(),
        Some("Skipped 1 unsupported image(s).")
    );

    // a dropped playlist refuses its directory row the same way
    state.notice = None;
    state.photos.files.clear();
    state.append_drop(vec![playlist.clone()]);
    assert_eq!(state.photos.files, [good.clone()], "rows keep their order");
    assert_eq!(
        state.notice.as_deref(),
        Some("Skipped 1 unsupported image(s).")
    );

    std::fs::remove_dir(&fake).unwrap();
    std::fs::remove_file(&good).unwrap();
    std::fs::remove_file(&playlist).unwrap();
    std::fs::remove_dir(&dir).unwrap();
}

#[test]
fn bulk_delete_drops_every_occurrence_and_keeps_the_failed_rows() {
    let dir = import_test_dir("delete-many");
    let gone = test_image(&dir, "gone.ppm");
    let missing = dir.join("missing.ppm");
    let both = test_image(&dir, "both.ppm");

    let mut state = State::new(dir.join("rgconfig.json"), &[]);
    state.photos.files = vec![gone.clone(), missing.clone(), gone.clone()];
    state.photos.index = 1;
    assert!(
        state.delete_many(&[0, 1, 2]).is_some(),
        "the missing file is reported"
    );
    assert_eq!(
        state.photos.files,
        vec![missing.clone()],
        "a failed path keeps its row; the deleted one loses every occurrence"
    );
    assert!(!gone.exists());

    state.photos.files = vec![both.clone(), missing.clone(), both.clone()];
    state.photos.index = 2;
    assert_eq!(
        state.delete_many(&[0, 2, 0]),
        None,
        "a repeated row counts once"
    );
    assert_eq!(state.photos.files, vec![missing.clone()]);
    assert_eq!(state.photos.current(), Some(&missing));
    assert!(!both.exists());

    std::fs::remove_dir(dir).unwrap();
}

/// Each parity case imports at least one row into a fresh viewer. Wait for its rows
/// and first image instead of treating a briefly empty result queue as completion.
fn settle(state: &mut State) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        state.poll_background();
        if !state.photos.is_empty() && !state.load_pending {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("the import lane did not settle within 10s");
}

#[test]
fn the_import_lane_and_the_direct_methods_prepare_the_same_rows_and_notices() {
    let dir = import_test_dir("parity");
    let a = test_image(&dir, "a.ppm");
    let b = test_image(&dir, "b.ppm");
    let unsupported = dir.join("notes.txt");
    std::fs::write(&unsupported, b"text").unwrap();
    let list = dir.join("rows.rgpl");
    std::fs::write(
        &list,
        format!("missing.ppm\n{}\n{}\n", b.display(), a.display()),
    )
    .unwrap();
    let config = dir.join("rgconfig.json");
    let both = || {
        (
            State::new(config.clone(), &[]),
            State::interactive(config.clone(), &[]),
        )
    };

    // A dropped batch: the playlist's existing rows plus an unsupported file.
    let (mut direct, mut interactive) = both();
    let batch = vec![list.clone(), unsupported.clone()];
    direct.open_files(&batch);
    interactive.open_files(&batch);
    settle(&mut interactive);
    assert_eq!(interactive.photos.files, direct.photos.files);
    assert_eq!(
        interactive.photos.files,
        [a.clone(), b.clone()],
        "a replacing batch takes Explorer's name order"
    );
    assert_eq!(interactive.notice, direct.notice);

    // An append keeps duplicates by occurrence, on either path.
    let (mut direct, mut interactive) = both();
    let appends = [b.clone(), a.clone(), b.clone()];
    assert_eq!(direct.add_files(&appends), 3);
    assert_eq!(interactive.add_files(&appends), 3);
    settle(&mut interactive);
    assert_eq!(interactive.photos.files, direct.photos.files);
    assert_eq!(
        direct.photos.files, appends,
        "duplicates stay as occurrences"
    );
    assert_eq!(interactive.notice, direct.notice);

    // An explicit playlist open keeps its order and reports its missing rows.
    let (mut direct, mut interactive) = both();
    assert_eq!(direct.open_playlist(&list), Ok(2));
    assert_eq!(interactive.open_playlist(&list), Ok(0), "queued");
    settle(&mut interactive);
    assert_eq!(interactive.photos.files, direct.photos.files);
    assert_eq!(
        direct.photos.files,
        [b.clone(), a.clone()],
        "playlist order"
    );
    assert_eq!(interactive.notice, direct.notice);
    assert_eq!(
        interactive.notice.as_deref(),
        Some("Playlist skipped 1 missing image(s).")
    );
    assert_eq!(interactive.message, direct.message);

    for file in [a, b, unsupported, list] {
        std::fs::remove_file(file).unwrap();
    }
    std::fs::remove_dir(dir).unwrap();
}
