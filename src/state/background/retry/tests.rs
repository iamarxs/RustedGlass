use super::*;
use std::time::{Duration, Instant};

#[test]
fn an_inaccessible_root_becomes_retryable_and_then_loads_after_recovery() {
    let root = crate::test_path("folder-retry-recovered");
    assert!(!root.exists());
    let mut state = State::interactive(crate::test_path("unused-config.json"), &[]);
    state.open_folder(&root);
    drain_import(&mut state);
    let notice = state.notice.as_deref().unwrap();
    assert!(notice.contains("folder-retry-recovered"), "{notice}");
    assert!(notice.contains("os error"), "{notice}");
    assert!(state.can_retry_folder_scan());

    std::fs::create_dir(&root).unwrap();
    let image = root.join("found.ppm");
    let later = root.join("later.ppm");
    for file in [&image, &later] {
        std::fs::write(file, b"P6\n1 1\n255\n\xff\x00\x00").unwrap();
    }
    state.retry_folder_scan();
    drain_import(&mut state);
    assert_eq!(
        state.photos.visible_rows().collect::<Vec<_>>(),
        [&image, &later]
    );
    assert_eq!(state.photos.current(), Some(&image));
    assert!(!state.can_retry_folder_scan());
    for file in [image, later] {
        std::fs::remove_file(file).unwrap();
    }
    std::fs::remove_dir(root).unwrap();
}

#[test]
fn retry_replaces_only_the_incomplete_append_without_duplicating_its_rows() {
    let root = crate::test_path("folder-retry-partial");
    std::fs::create_dir_all(&root).unwrap();
    let a = root.join("a.ppm");
    let z = root.join("z.ppm");
    for file in [&a, &z] {
        std::fs::write(file, b"P6\n1 1\n255\n\xff\x00\x00").unwrap();
    }
    let mut state = State::interactive(crate::test_path("unused-config.json"), &[]);
    state.photos.files.push(a.clone()); // an intentional duplicate before the append
    state.prep_tag = 1;
    state.pending_import = Some(1);
    state.apply_event(Event::FolderRows {
        tag: 1,
        replacing: false,
        rows: vec![(
            z.clone(),
            photo::FileFacts {
                size: 14,
                modified: 0,
            },
        )],
    });
    state.apply_event(Event::FolderFinished {
        tag: 1,
        order: vec![0],
        seed: None,
        failure: Some((
            photo::FolderScanError {
                folder: root.clone(),
                error: std::io::Error::from_raw_os_error(64),
            },
            Source::Folder {
                path: root.clone(),
                include_hidden: false,
                recurse: false,
                seed: None,
            },
            false,
        )),
    });
    assert_eq!(state.photos.visible_rows().collect::<Vec<_>>(), [&a, &z]);
    state.photos.index = 1;
    state.retry_folder_scan();
    assert_eq!(state.photos.visible_rows().collect::<Vec<_>>(), [&a, &z]);
    drain_import(&mut state);
    assert_eq!(
        state.photos.visible_rows().collect::<Vec<_>>(),
        [&a, &a, &z]
    );
    assert_eq!(state.photos.index, 2);
    for file in [a, z] {
        std::fs::remove_file(file).unwrap();
    }
    std::fs::remove_dir(root).unwrap();
}

fn drain_import(state: &mut State) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while state.importing() {
        state.poll_background();
        assert!(Instant::now() < deadline, "folder scan did not finish");
        std::thread::sleep(Duration::from_millis(5));
    }
}
