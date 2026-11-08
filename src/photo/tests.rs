use super::*;
use crate::w32::file_name;

#[test]
fn format_list_covers_enabled_decoders_without_magick_formats() {
    let formats = image_formats();
    assert!(formats.contains(&".jpg".to_string()));
    assert!(!formats.contains(&".heic".to_string()));
    assert_eq!(formats.len(), IMAGE_FORMATS.split(';').count());
}

#[test]
fn names_sort_the_way_explorer_sorts_them() {
    let mut paths = ["img10.png", "img2.png", "img2.png.bak", "a.png"]
        .map(PathBuf::from)
        .to_vec();
    sort_names(&mut paths, false, None);
    let names: Vec<_> = paths.iter().map(|path| file_name(path)).collect();
    assert_eq!(
        names,
        vec!["a.png", "img2.png", "img2.png.bak", "img10.png"]
    );
}

#[test]
fn folder_list_skips_other_files() {
    let dir = crate::test_path("ig_photo_test");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("b.png"), b"not an image").unwrap();
    fs::write(dir.join("a.jpg"), b"not an image").unwrap();
    fs::write(dir.join("notes.txt"), b"skip me").unwrap();

    let list = collect(&dir, false, false, &image_formats());
    assert_eq!(
        list.iter().map(|p| file_name(p)).collect::<Vec<_>>(),
        vec!["a.jpg", "b.png"]
    );

    let mut photos = PhotoList::from_path(&dir.join("a.jpg"), false, false, image_formats());
    assert_eq!(photos.index, 0);
    assert!(photos.step(1, false, false, None, (false, false)));
    assert_eq!(file_name(photos.current().unwrap()), "b.png");
    assert!(!photos.step(1, false, false, None, (false, false))); // no loop, no sibling folder
    assert!(photos.step(-1, true, false, None, (false, false))); // wrap back to the first
    assert_eq!(file_name(photos.current().unwrap()), "a.jpg");

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn shuffle_is_a_permutation_and_browsing_follows_it() {
    let dir = crate::test_path("ig_order_test");
    fs::create_dir_all(&dir).unwrap();
    for name in ["a.png", "b.png", "c.png", "d.png"] {
        fs::write(dir.join(name), b"x").unwrap();
    }

    let mut photos = PhotoList::from_path(&dir.join("a.png"), false, false, image_formats());
    let natural: Vec<String> = photos.files.iter().map(|path| file_name(path)).collect();
    assert_eq!(natural, vec!["a.png", "b.png", "c.png", "d.png"]);

    photos.shuffle(1);
    assert!(!photos.order.is_empty());
    assert_eq!(photos.len(), 4, "a shuffle never changes the count");
    let mut shuffled: Vec<String> = photos.visible().iter().map(|p| file_name(p)).collect();
    shuffled.sort();
    assert_eq!(shuffled, natural, "every entry appears exactly once");

    let mut walked: Vec<String> = Vec::new();
    for _ in 0..3 {
        walked.push(file_name(photos.current().unwrap()));
        assert!(
            photos.step(1, false, false, None, (false, false)),
            "never stuck before the last entry"
        );
    }
    walked.push(file_name(photos.current().unwrap()));
    assert!(
        !photos.step(1, false, false, None, (false, false)),
        "without loop_back the last entry has no next"
    );
    let mut sorted = walked;
    sorted.sort();
    assert_eq!(sorted, natural, "stepping walks the shuffled order");

    photos.order.clear();
    assert_eq!(
        photos
            .visible()
            .iter()
            .map(|p| file_name(p))
            .collect::<Vec<_>>(),
        natural,
        "clearing the order is the plain list again"
    );

    photos.jump_to(2);
    assert_eq!(file_name(photos.current().unwrap()), "c.png");
    assert_eq!(photos.jump_to(99), None, "out of range is refused");

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_row_can_be_moved_and_the_shown_file_follows_it() {
    let mut photos = PhotoList::empty();
    photos.files = vec![
        PathBuf::from("a.png"),
        PathBuf::from("b.png"),
        PathBuf::from("c.png"),
    ];
    photos.index = 1;

    photos.move_row(1, 0);

    assert_eq!(
        photos.visible().into_iter().cloned().collect::<Vec<_>>(),
        vec![
            PathBuf::from("b.png"),
            PathBuf::from("a.png"),
            PathBuf::from("c.png")
        ]
    );
    assert_eq!(
        photos.current(),
        Some(&PathBuf::from("b.png")),
        "the shown file is still the shown file"
    );
    assert_eq!(photos.index, 0);
}

#[test]
fn moving_and_sorting_keep_the_selected_occurrence_of_a_duplicate() {
    let mut photos = PhotoList::empty();
    photos.files = ["a.png", "b.png", "a.png"].map(PathBuf::from).to_vec();
    photos.index = 2;
    photos.move_row(0, 1);
    assert_eq!(photos.index, 2);
    photos.sort_by_name(false);
    assert_eq!(photos.index, 1);
    assert_eq!(photos.current(), Some(&PathBuf::from("a.png")));
}

#[test]
fn names_can_be_sorted_in_both_directions() {
    let mut photos = PhotoList::empty();
    photos.files = vec![
        PathBuf::from("a.png"),
        PathBuf::from("b2.png"),
        PathBuf::from("b10.png"),
    ];
    photos.index = 2;

    photos.sort_by_name(true);
    assert_eq!(
        photos.files,
        vec![
            PathBuf::from("b10.png"),
            PathBuf::from("b2.png"),
            PathBuf::from("a.png")
        ]
    );
    assert_eq!(photos.current(), Some(&PathBuf::from("b10.png")));

    photos.sort_by_name(false);
    assert_eq!(
        photos.files,
        vec![
            PathBuf::from("a.png"),
            PathBuf::from("b2.png"),
            PathBuf::from("b10.png")
        ]
    );
    assert_eq!(
        photos.current(),
        Some(&PathBuf::from("b10.png")),
        "sorting again does not lose the image"
    );
}

#[test]
fn entries_can_be_added_and_removed() {
    let mut photos = PhotoList::empty();
    photos.files = vec![PathBuf::from("dir/b.png")];
    photos.index = 0;

    let added = photos.add_files(&[
        PathBuf::from("dir/a.png"),
        PathBuf::from("dir/b.png"),
        PathBuf::from("dir/readme.txt"),
    ]);
    assert_eq!(added, 2, "duplicates are kept and non-images are skipped");
    assert_eq!(
        photos.files,
        vec![
            PathBuf::from("dir/b.png"),
            PathBuf::from("dir/a.png"),
            PathBuf::from("dir/b.png")
        ]
    );
    assert_eq!(
        photos.current(),
        Some(&PathBuf::from("dir/b.png")),
        "stays on the shown file"
    );

    assert_eq!(photos.remove_at(1), Some(PathBuf::from("dir/a.png")));
    assert_eq!(photos.current(), Some(&PathBuf::from("dir/b.png")));
    assert_eq!(photos.remove_current(), Some(PathBuf::from("dir/b.png")));
    assert_eq!(photos.remove_current(), Some(PathBuf::from("dir/b.png")));
    assert!(photos.is_empty());
    assert_eq!(photos.remove_current(), None, "nothing left to remove");
}

#[test]
fn an_empty_playlist_walks_nowhere() {
    let mut photos = PhotoList::empty();
    assert!(photos.is_empty());
    assert!(
        !photos.step(1, true, false, None, (false, false)),
        "no divide by zero on an empty list"
    );
    assert_eq!(photos.current(), None);
    assert_eq!(photos.visible().len(), 0);
    photos.shuffle(7);
    assert!(photos.order.is_empty(), "nothing to shuffle");
    assert_eq!(photos.jump_to(0), None);
}

#[cfg(windows)]
#[test]
fn recursive_scan_skips_junctions_but_reads_real_subfolders() {
    let dir = crate::test_path("junction-test");
    let nested = dir.join("nested");
    fs::create_dir_all(&nested).unwrap();
    let image = nested.join("a.png");
    fs::write(&image, b"fixture").unwrap();
    let created = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J", "loop", "."])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(created.status.success(), "{created:?}");

    let images = collect(&dir, false, true, &image_formats());
    fs::remove_dir(dir.join("loop")).unwrap();
    fs::remove_file(&image).unwrap();
    fs::remove_dir(nested).unwrap();
    fs::remove_dir(dir).unwrap();
    assert_eq!(images, vec![image]);
}
