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
    assert_eq!(
        state.photos.files,
        [good.clone(), good.clone(), bad.clone()]
    );
    assert_eq!(state.photos.index, 1);
    assert!(state
        .notice
        .as_deref()
        .unwrap()
        .contains("Removed from the playlist — b-corrupt.png is unreadable"));
    assert!(state.frame().is_some());

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
fn configured_shuffle_applies_on_folder_and_playlist_load() {
    let dir = import_test_dir("shuffle-import");
    let files = ["a.ppm", "b.ppm", "c.ppm"].map(|name| test_image(&dir, name));
    let playlist = dir.join("images.rgpl");
    std::fs::write(&playlist, "c.ppm\na.ppm\nb.ppm\n").unwrap();
    let config = dir.join("rgconfig.json");
    let shuffle = "-p:EnableRandomDisplayOrder=true".to_owned();
    let assert_shuffled = |state: &State| {
        let mut order = state.photos.order.clone();
        order.sort_unstable();
        assert_eq!(order, [0, 1, 2]);
        assert_eq!(state.photos.index, 0);
    };

    let mut state = State::new(config.clone(), std::slice::from_ref(&shuffle));
    state.open_folder(&dir);
    assert_shuffled(&state);
    state.open_playlist(&playlist).unwrap();
    assert_shuffled(&state);

    for path in [&dir, &playlist] {
        let state = State::new(
            config.clone(),
            &[path.display().to_string(), shuffle.clone()],
        );
        assert_shuffled(&state);
    }
    for file in files.into_iter().chain([playlist]) {
        std::fs::remove_file(file).unwrap();
    }
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn startup_and_sibling_navigation_use_the_folder_settings() {
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
    assert_eq!(state.photos.files, [shown.clone()]);
    assert!(state.step(1));
    assert_eq!(state.photos.current(), Some(&next));
    for file in [shown, hidden, next] {
        std::fs::remove_file(file).unwrap();
    }
    for folder in [current, nested, sibling, dir] {
        std::fs::remove_dir(folder).unwrap();
    }
}
