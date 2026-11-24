use super::*;

fn png(path: &Path, width: u32) {
    ::image::RgbaImage::from_pixel(width, 1, ::image::Rgba([9, 8, 7, 255]))
        .save(path)
        .unwrap();
}

fn facts() -> photo::FileFacts {
    photo::FileFacts {
        size: 1,
        modified: 0,
    }
}

fn occurrences(state: &State, path: &Path) -> usize {
    state
        .photos
        .files
        .iter()
        .filter(|file| *file == path)
        .count()
}

#[test]
fn a_direct_image_shows_before_its_scan_and_seeds_natural_order_once() {
    let dir = crate::test_path("import-direct-early");
    std::fs::create_dir_all(&dir).unwrap();
    let initial = dir.join("m.png");
    let first = dir.join("a.png");
    let last = dir.join("z.png");
    png(&initial, 5);
    png(&first, 1);
    png(&last, 3);

    let mut state = State::new(dir.join("config.json"), &[]);
    let tag = 1;
    state.replace_tag = tag;
    state.pending_import = Some(tag);
    state.photos = PhotoList::empty();
    state.photos.files.push(initial.clone());
    state.import_preview = Some((tag, 0));
    state.direct_seed = Some(initial.clone());
    state.load_current();

    assert_eq!(state.frame().unwrap().width, 5, "the direct image decodes");
    let revision = state.slide_revision;

    assert!(state.apply_event(Event::FolderRows {
        tag,
        replacing: true,
        rows: vec![(first.clone(), facts()), (last.clone(), facts())],
    }));
    assert_eq!(state.photos.len(), 3);
    assert_eq!(state.photos.current(), Some(&initial));
    assert_eq!(state.frame().unwrap().width, 5);
    assert_eq!(
        state.slide_revision, revision,
        "early scan rows do not decode the shown file again"
    );

    assert!(state.apply_event(Event::FolderRows {
        tag,
        replacing: true,
        rows: vec![(initial.clone(), facts())],
    }));
    assert_eq!(
        state.photos.len(),
        4,
        "the provisional row is still separate"
    );

    assert!(state.apply_event(Event::FolderFinished {
        tag,
        order: vec![0, 2, 1],
        seed: None,
        failure: None,
    }));
    let visible: Vec<_> = state.photos.visible_rows().cloned().collect();
    assert_eq!(visible, [first, initial.clone(), last]);
    assert_eq!(occurrences(&state, &initial), 1, "the seed occurs once");
    assert_eq!(state.photos.current(), Some(&initial));
    assert_eq!(state.frame().unwrap().width, 5, "its decoded frame stays");
    assert_eq!(
        state.slide_revision, revision,
        "finish does not decode the same file again"
    );
    assert!(!state.importing());

    drop(state);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn navigation_before_the_direct_image_is_discovered_survives_the_finish() {
    let dir = crate::test_path("import-direct-navigated");
    std::fs::create_dir_all(&dir).unwrap();
    let initial = dir.join("m.png");
    let first = dir.join("a.png");
    png(&initial, 5);
    png(&first, 1);

    let mut state = State::new(dir.join("config.json"), &[]);
    let tag = 1;
    state.replace_tag = tag;
    state.pending_import = Some(tag);
    state.photos = PhotoList::empty();
    state.photos.files.push(initial.clone());
    state.import_preview = Some((tag, 0));
    state.direct_seed = Some(initial.clone());
    state.load_current();

    assert!(state.apply_event(Event::FolderRows {
        tag,
        replacing: true,
        rows: vec![(first.clone(), facts())],
    }));
    state.photos.jump_to(1);
    state.load_current();
    assert_eq!(state.photos.current(), Some(&first));
    let revision = state.slide_revision;

    // The initial image is only discovered after the user moved to another row.
    assert!(state.apply_event(Event::FolderRows {
        tag,
        replacing: true,
        rows: vec![(initial.clone(), facts())],
    }));
    assert_eq!(
        state.photos.current(),
        Some(&first),
        "discovery does not jump"
    );

    assert!(state.apply_event(Event::FolderFinished {
        tag,
        order: vec![0, 1],
        seed: None,
        failure: None,
    }));
    let visible: Vec<_> = state.photos.visible_rows().cloned().collect();
    assert_eq!(visible, [first.clone(), initial.clone()]);
    assert_eq!(
        state.photos.current(),
        Some(&first),
        "the current row stays"
    );
    assert_eq!(state.frame().unwrap().width, 1, "and so does its picture");
    assert_eq!(
        state.slide_revision, revision,
        "finish does not decode the current file again"
    );
    assert_eq!(occurrences(&state, &initial), 1);

    drop(state);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_startup_image_is_provisional_before_its_folder_scan_finishes() {
    let dir = crate::test_path("import-direct-startup");
    std::fs::create_dir_all(&dir).unwrap();
    let image = dir.join("Only.png");
    let argument = dir.join("only.PNG");
    png(&image, 4);
    let args = [
        argument.to_string_lossy().into_owned(),
        "-p:EnableSlideshow=true".to_string(),
    ];

    let mut state = State::interactive(dir.join("test.json"), &args);
    assert_eq!(
        state.photos.files,
        [argument.clone()],
        "the file is row zero before its folder scan delivers rows"
    );
    assert_eq!(state.photos.current(), Some(&argument));
    assert!(state.load_pending, "the decode was submitted at startup");
    assert!(state.startup_slideshow);

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while (state.frame().is_none() || state.importing()) && std::time::Instant::now() < deadline {
        state.poll_background();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(state.frame().is_some());
    assert_eq!(
        state.photos.files,
        [image.clone()],
        "the scan occurrence replaces the provisional row"
    );
    assert_eq!(state.photos.current(), Some(&image));
    assert_eq!(occurrences(&state, &image), 1);
    assert!(state.slideshow_running);

    drop(state);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_explicitly_opened_hidden_image_gets_its_natural_position() {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN};

    let dir = crate::test_path("hidden-direct-seed");
    std::fs::create_dir_all(&dir).unwrap();
    let first = dir.join("a.png");
    let hidden = dir.join("m.png");
    let last = dir.join("z.png");
    for path in [&first, &hidden, &last] {
        png(path, 2);
    }
    let name = crate::w32::wide(&hidden.to_string_lossy());
    unsafe { SetFileAttributesW(PCWSTR(name.as_ptr()), FILE_ATTRIBUTE_HIDDEN) }.unwrap();
    let args = [
        hidden.to_string_lossy().into_owned(),
        "-p:EnableHiddenImagesLoading=false".into(),
    ];
    let mut state = State::interactive(dir.join("config.json"), &args);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while state.importing() {
        state.poll_background();
        assert!(
            std::time::Instant::now() < deadline,
            "folder scan did not finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        state.photos.visible_rows().collect::<Vec<_>>(),
        [&first, &hidden, &last]
    );
    assert_eq!(state.photos.current(), Some(&hidden));
    drop(state);
    std::fs::remove_dir_all(dir).unwrap();
}
