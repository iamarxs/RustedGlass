use super::worker::OpenedOutcome;
use super::*;

#[test]
fn the_scan_notice_expires_once_while_the_import_and_errors_remain() {
    let mut state = State::interactive(crate::test_path("unused-config.json"), &[]);
    let source = || Source::Batch {
        paths: Vec::new(),
        include_hidden: false,
        recurse: false,
    };
    assert!(!state.show_import_notice());
    assert!(state.submit_import(source(), true));
    assert!(state.show_import_notice());
    let old_tag = state.pending_import.unwrap();

    state.import_notice_until = Some(Instant::now());
    state.notice = Some("A separate error.".into());
    assert!(state.poll_background_budget(0), "repaint without results");
    assert!(state.importing(), "the import is still pending");
    assert!(!state.show_import_notice());
    assert_eq!(state.notice.as_deref(), Some("A separate error."));
    assert!(!state.poll_background_budget(0), "expire only once");

    assert!(state.submit_import(source(), true));
    state.import_finished(old_tag);
    assert!(state.show_import_notice(), "a new scan has its own notice");
    state.import_finished(state.pending_import.unwrap());
    assert!(
        !state.show_import_notice(),
        "completion hides it immediately"
    );
}

fn still_decoded() -> crate::decode::Decoded {
    crate::decode::to_decoded(::image::RgbaImage::from_pixel(
        2,
        2,
        ::image::Rgba([9, 8, 7, 255]),
    ))
}

/// A two-frame animated GIF: the smallest file the animation path streams.
fn animated_gif(path: &Path) {
    let mut encoder = ::image::codecs::gif::GifEncoder::new(std::fs::File::create(path).unwrap());
    for colour in [[255, 0, 0, 255], [0, 0, 255, 255]] {
        let buffer = ::image::RgbaImage::from_pixel(2, 2, ::image::Rgba(colour));
        let frame =
            ::image::Frame::from_parts(buffer, 0, 0, ::image::Delay::from_numer_denom_ms(20, 1));
        encoder.encode_frame(frame).unwrap();
    }
}

#[test]
fn a_failed_frame_keeps_the_last_good_frame_and_stops_the_animation() {
    let mut state = State::new(crate::test_path("unused-config.json"), &[]);
    let path = PathBuf::from("animated.gif");
    state.photos.files = vec![path.clone()];
    state.selection = 1;
    state.frame_pending = true;
    state.animated_selection = true;
    state.bitmap = Some(Bitmap::from_decoded(still_decoded()));

    assert!(state.apply_event(Event::FrameFailed { generation: 1 }));
    assert!(state.frame().is_some(), "the last good frame stays");
    assert!(!state.frame_pending, "the pending flag clears");
    assert!(!state.animated_selection, "the cursor clears");
    assert_eq!(
        state.notice.as_deref(),
        Some("animated.gif stopped playing. Showing the last frame."),
        "exactly one notice"
    );
    assert!(
        !state.load_pending,
        "the failed file is not reopened by itself"
    );

    // A running slideshow may advance at its deadline, or right away if it is elapsed.
    state.slideshow_running = true;
    state.slideshow_elapsed = true;
    assert!(state.advance_slideshow_if_ready());
}

#[test]
fn a_synchronous_rewind_failure_keeps_the_last_frame_without_reloading() {
    let path = crate::test_path("background-sync-rewind-failure.gif");
    animated_gif(&path);
    let mut state = State::new(crate::test_path("unused-config.json"), &[]);
    state.photos.files = vec![path.clone()];
    state.load_current();
    assert!(state.is_animated(), "the GIF streams synchronously");

    // Rust opens files for delete-sharing, so the vanished file makes the rewind fail.
    std::fs::remove_file(&path).unwrap();
    state.rewind_animation();
    assert!(state.frame().is_some(), "the last good frame stays");
    assert!(!state.is_animated(), "the cursor is gone");
    assert!(state.notice.is_some(), "the failure says so once");
    assert!(!state.load_pending, "the file is not reopened by itself");

    state.slideshow_running = true;
    state.slideshow_elapsed = true;
    assert!(
        state.advance_slideshow_if_ready(),
        "the slideshow may move on"
    );
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_stale_generation_cannot_touch_the_current_selection() {
    let mut state = State::new(crate::test_path("unused-config.json"), &[]);
    let path = crate::test_path("generation-stale.png");
    std::fs::write(&path, b"stale-bytes").unwrap();
    state.photos.files = vec![path.clone(), path.clone()];
    state.photos.index = 1;
    state.selection = 2;
    state.load_pending = true;

    let stale = Event::Opened {
        generation: 1,
        identity: None,
        outcome: OpenedOutcome::Still {
            image: still_decoded(),
            eligible: true,
        },
    };
    assert!(!state.apply_event(stale), "an old reply is not work");
    assert!(state.bitmap.is_none(), "an old reply shows nothing");
    assert!(state.load_pending, "the loading state is untouched");

    std::fs::remove_file(&path).ok();
}

#[test]
fn the_live_generation_is_associated_exactly_once() {
    let mut state = State::new(crate::test_path("unused-config.json"), &[]);
    state.selection = 1;
    state.load_pending = true;
    let path = crate::test_path("generation-live.png");
    std::fs::write(&path, b"stale-bytes").unwrap();
    state.photos.files = vec![path.clone()];
    state.photos.index = 0;

    let live = Event::Opened {
        generation: 1,
        identity: Some((11, std::time::SystemTime::UNIX_EPOCH)),
        outcome: OpenedOutcome::Still {
            image: still_decoded(),
            eligible: true,
        },
    };
    assert!(state.apply_event(live));
    assert_eq!(state.file_size, Some(11));
    assert!(state.frame().is_some(), "the live reply shows");
    assert!(!state.load_pending, "the load is over");

    std::fs::remove_file(&path).ok();
}

#[test]
fn a_decode_invalidated_by_the_worker_displays_without_seeding_the_cache() {
    let path = crate::test_path("worker-invalidated-still.png");
    std::fs::write(&path, b"changed while decoding").unwrap();
    let mut state = State::new(crate::test_path("unused-config.json"), &[]);
    state.photos.files = vec![path.clone()];
    state.selection = 3;
    state.load_pending = true;
    assert!(state.apply_event(Event::Opened {
        generation: 3,
        identity: crate::cache::stat(&path),
        outcome: OpenedOutcome::Still {
            image: still_decoded(),
            eligible: false,
        },
    }));
    assert!(state.frame().is_some());
    assert!(crate::cache::candidate(&path).is_none());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn only_the_latest_replacement_reaches_the_playlist() {
    let mut state = State::new(crate::test_path("unused-config.json"), &[]);
    state.replace_tag = 8;
    let old = Event::Replaced {
        tag: 7,
        rows: PreparedImport {
            files: vec![PathBuf::from("old.png")],
            ..PreparedImport::default()
        },
    };
    assert!(!state.apply_event(old), "an older replacement is not work");
    assert!(state.photos.is_empty(), "no old rows appear");
}

#[test]
fn folder_preview_is_browsable_before_the_sorted_playlist_arrives() {
    let mut state = State::new(crate::test_path("unused-config.json"), &[]);
    state.replace_tag = 8;
    state.pending_import = Some(8);
    let early = crate::test_path("folder-preview-z.png");
    let first = crate::test_path("folder-preview-a.png");
    for (path, width) in [(&early, 7), (&first, 2)] {
        ::image::RgbaImage::from_pixel(width, 1, ::image::Rgba([9, 8, 7, 255]))
            .save(path)
            .unwrap();
    }
    assert!(!state.apply_event(Event::ImportPreview {
        tag: 7,
        replacing: true,
        files: vec![early.clone()],
    }));
    assert!(state.apply_event(Event::ImportPreview {
        tag: 8,
        replacing: true,
        files: vec![early.clone()],
    }));
    assert_eq!(state.photos.current(), Some(&early));
    assert_eq!(state.frame().unwrap().width, 7);
    assert!(state.browsing_import_preview());
    assert!(state.apply_event(Event::Replaced {
        tag: 8,
        rows: PreparedImport {
            files: vec![first.clone(), early.clone()],
            ..PreparedImport::default()
        },
    }));
    assert_eq!(state.photos.len(), 2);
    assert_eq!(state.photos.current(), Some(&first));
    assert_eq!(state.frame().unwrap().width, 2);
    assert!(!state.importing());
    assert!(!state.browsing_import_preview());
    std::fs::remove_file(early).unwrap();
    std::fs::remove_file(first).unwrap();
}

#[test]
fn an_appended_folder_replaces_provisional_rows_once() {
    let mut state = State::new(crate::test_path("unused-config.json"), &[]);
    let original = PathBuf::from("original.png");
    let early = PathBuf::from("z.png");
    let first = PathBuf::from("a.png");
    state.photos.files = vec![original.clone()];
    assert!(state.apply_event(Event::ImportPreview {
        tag: 1,
        replacing: false,
        files: vec![early.clone()],
    }));
    assert_eq!(state.photos.files, [original.clone(), early.clone()]);
    assert!(state.apply_event(Event::Appended {
        tag: 1,
        rows: PreparedImport {
            files: vec![first.clone(), early.clone()],
            ..PreparedImport::default()
        },
    }));
    assert_eq!(state.photos.files, [original.clone(), first, early]);
    assert_eq!(state.photos.current(), Some(&original));
}

#[test]
fn streamed_folders_keep_discovery_order_for_metadata_and_the_selected_occurrence() {
    let mut state = State::new(crate::test_path("unused-config.json"), &[]);
    let same = PathBuf::from("z.png");
    let first = PathBuf::from("a.png");
    let facts = photo::FileFacts {
        size: 20,
        modified: 40,
    };
    state.replace_tag = 1;
    state.pending_import = Some(1);
    assert!(state.apply_event(Event::FolderRows {
        tag: 1,
        replacing: true,
        rows: vec![(same.clone(), facts)],
    }));
    assert_eq!(state.photos.len(), 1);
    assert!(state.apply_event(Event::FolderRows {
        tag: 1,
        replacing: true,
        rows: vec![(first.clone(), facts), (same.clone(), facts)],
    }));
    state.photos.jump_to(2);
    assert!(state.apply_event(Event::FolderFinished {
        tag: 1,
        order: vec![1, 0, 2],
        seed: None,
        failure: None,
    }));
    assert_eq!(state.photos.path_at(0), Some(&first));
    assert_eq!(state.photos.metadata_path_at(0), Some(&same));
    assert_eq!(state.photos.index, 2);
    assert_eq!(state.photos.file_index(state.photos.index), Some(2));
    assert!(!state.importing());
}

#[test]
fn startup_playlist_keeps_file_order_and_starts_the_slideshow_after_import() {
    let dir = crate::test_path("background-startup-list");
    std::fs::create_dir_all(&dir).unwrap();
    let second = dir.join("z.png");
    let first = dir.join("a.png");
    for path in [&second, &first] {
        ::image::RgbaImage::from_pixel(2, 2, ::image::Rgba([10, 20, 30, 255]))
            .save(path)
            .unwrap();
    }
    let list = dir.join("ordered.rgpl");
    std::fs::write(&list, "z.png\na.png\n").unwrap();
    let args = [
        list.to_string_lossy().into_owned(),
        "-p:EnableSlideshow=true".to_string(),
    ];
    let mut state = State::interactive(dir.join("test.json"), &args);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while state.frame().is_none() && std::time::Instant::now() < deadline {
        state.poll_background();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(state.frame().is_some());
    assert_eq!(state.photos.files, [second, first]);
    assert_eq!(state.photos.index, 0);
    assert!(state.slideshow_running);
    drop(state);
    std::fs::remove_dir_all(dir).ok();
}
