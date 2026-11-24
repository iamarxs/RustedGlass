use super::*;
use std::fs;

fn temp_dir(name: &str) -> PathBuf {
    crate::test_path(&format!("ig_fileops_{name}"))
}

#[test]
fn save_as_copies_the_original_bytes_to_the_selected_name() {
    let dir = temp_dir("pending");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("cat.png");
    fs::write(&file, b"pixels").unwrap();

    let target = dir.join("kit.png");
    fs::write(&target, b"previous").unwrap();
    crate::atomic_file::copy(&file, &target).unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"pixels");
    assert_eq!(fs::read(&file).unwrap(), b"pixels");

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn rename_and_move_keep_the_extension_and_refuse_bad_names() {
    let dir = temp_dir("rename");
    let other = temp_dir("other");
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(&other).unwrap();
    let file = dir.join("cat.png");
    fs::write(&file, b"pixels").unwrap();

    assert_eq!(prefill(&file).0, "cat.png");
    assert_eq!(prefill(&file).1, 3, "only the stem is pre-selected");
    assert_eq!(extension(&file), ".png");

    // An unchanged full name is a no-op, not a mangled rename.
    let dotted = dir.join("trip.v2.jpg");
    fs::write(&dotted, b"pixels").unwrap();
    assert_eq!(rename(&dotted, "trip.v2.jpg").unwrap(), dotted);
    assert!(dotted.exists());

    assert!(rename(&file, "").is_err());
    assert!(rename(&file, "no/path").is_err());
    assert!(rename(&file, "NUL.png").is_err());
    assert!(rename(&file, "name.").is_err());
    let capitalized = rename(&file, "Cat.png").unwrap();
    assert!(fs::read_dir(&dir)
        .unwrap()
        .any(|entry| entry.unwrap().file_name() == "Cat.png"));
    let file = rename(&capitalized, "cat.png").unwrap();
    assert_eq!(
        rename(&file, "kit").unwrap(),
        dir.join("kit.png"),
        "the extension is kept"
    );

    // Moving keeps the name and leaves the old path gone.
    let moved = move_to(&dir.join("kit.png"), &other).unwrap();
    assert_eq!(moved, other.join("kit.png"));
    assert!(!dir.join("kit.png").exists());

    delete(&moved).unwrap();
    assert!(!moved.exists(), "moved to the recycle bin");
    assert!(
        delete(&moved).is_err(),
        "a second delete reports the missing file"
    );

    fs::remove_dir_all(&dir).ok();
    fs::remove_dir_all(&other).ok();
}

#[test]
fn a_name_without_an_extension_is_kept_whole() {
    let path = PathBuf::from("notes");
    assert_eq!(prefill(&path).0, "notes");
    assert_eq!(
        prefill(&path).1,
        5,
        "an extensionless name is selected whole"
    );
    assert_eq!(extension(&path), "");
    assert_eq!(file_name(&path), "notes");
}

#[test]
fn prefill_selects_the_dotted_stem_and_counts_utf16_units() {
    // Typing over the selection turns "trip.v2.jpg" into "holiday.jpg".
    let (name, stem_len) = prefill(Path::new("trip.v2.jpg"));
    assert_eq!(name, "trip.v2.jpg");
    assert_eq!(
        stem_len, 7,
        "the dotted stem is selected, not the whole name"
    );

    // A leading dot does not split the name, exactly like `extension`.
    assert_eq!(prefill(Path::new(".hidden")).1, 7);

    // A non-BMP character is two UTF-16 units, matching edit-control offsets.
    assert_eq!(prefill(Path::new("🖼café.png")).1, 6);
}

#[test]
fn transfers_number_collisions_and_rename_preserves_the_destination() {
    let dir = crate::test_path("file-collisions");
    let target = dir.join("target");
    fs::create_dir_all(&target).unwrap();
    let source = dir.join("cat.png");
    fs::write(&source, b"source").unwrap();
    fs::write(target.join("cat.png"), b"original").unwrap();
    fs::write(dir.join("other.png"), b"keep").unwrap();

    assert!(rename(&source, "other.png").is_err());
    assert_eq!(fs::read(dir.join("other.png")).unwrap(), b"keep");
    assert_eq!(
        move_to(&source, &target).unwrap(),
        target.join("cat (2).png")
    );
    assert_eq!(fs::read(target.join("cat.png")).unwrap(), b"original");
    assert_eq!(fs::read(target.join("cat (2).png")).unwrap(), b"source");
    assert!(!source.exists());

    for file in ["cat.png", "cat (2).png"] {
        fs::remove_file(target.join(file)).unwrap();
    }
    fs::remove_file(dir.join("other.png")).unwrap();
    fs::remove_dir(target).unwrap();
    fs::remove_dir(dir).unwrap();
}

#[test]
fn directories_are_refused_at_the_rename_and_move_boundary() {
    let dir = temp_dir("reject-dirs");
    let target = dir.join("target");
    let sub = dir.join("sub.png");
    fs::create_dir_all(&target).unwrap();
    fs::create_dir_all(&sub).unwrap();
    fs::write(dir.join("dog.png"), b"keep").unwrap();

    assert!(rename(&sub, "Renamed").is_err());
    assert!(move_to(&sub, &target).is_err());
    assert!(sub.is_dir(), "the directory stays");

    // a directory shelved in the target under the file's name is numbered past
    fs::create_dir(target.join("dog.png")).unwrap();
    let moved = move_to(&dir.join("dog.png"), &target).unwrap();
    assert_eq!(moved, target.join("dog (2).png"));

    fs::remove_dir(target.join("dog.png")).unwrap();
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_reported_move_is_complete_only_when_the_original_is_gone() {
    let dir = temp_dir("partial-outcome");
    let source = dir.join("src.png");
    let target = dir.join("remote").join("src.png");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&source, b"x").unwrap();

    fs::write(&target, b"x").unwrap();
    let error = verify_move(&source, &target).unwrap_err();
    assert!(error.contains(&source.display().to_string()));
    assert!(error.contains(&target.display().to_string()));
    assert!(source.is_file() && target.is_file());
    fs::remove_file(&source).unwrap();
    assert!(verify_move(&source, &target).is_ok());

    fs::remove_dir_all(&dir).ok();
}
