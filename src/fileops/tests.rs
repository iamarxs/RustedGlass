use super::*;
use std::fs;

fn temp_dir(name: &str) -> PathBuf {
    crate::test_path(&format!("ig_fileops_{name}"))
}

#[test]
fn a_pending_operation_runs_with_what_the_user_typed() {
    let dir = temp_dir("pending");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("cat.png");
    fs::write(&file, b"pixels").unwrap();

    assert_eq!(Pending::Rename.prefill(&file), "cat");
    assert_eq!(
        Pending::Rename.apply(&file, "kit").unwrap(),
        dir.join("kit.png")
    );

    let target = temp_dir("pending-target");
    fs::create_dir_all(&target).unwrap();
    let copied = Pending::CopyTo(PathBuf::new())
        .apply(&dir.join("kit.png"), &target.display().to_string())
        .unwrap();
    assert_eq!(copied, target.join("kit.png"));
    assert!(
        dir.join("kit.png").exists(),
        "a copy leaves the original alone"
    );

    fs::remove_dir_all(&dir).ok();
    fs::remove_dir_all(&target).ok();
}

#[test]
fn rename_and_move_keep_the_extension_and_refuse_bad_names() {
    let dir = temp_dir("rename");
    let other = temp_dir("other");
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(&other).unwrap();
    let file = dir.join("cat.png");
    fs::write(&file, b"pixels").unwrap();

    assert_eq!(stem(&file), "cat");
    assert_eq!(extension(&file), ".png");

    assert!(rename(&file, "").is_err());
    assert!(rename(&file, "no/path").is_err());
    assert!(rename(&file, "NUL.png").is_err());
    assert!(rename(&file, "name.").is_err());
    assert!(copy_to(&file, Path::new("")).is_err());
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

    // Copying leaves the original in place.
    let copy = copy_to(&moved, &dir).unwrap();
    assert_eq!(copy, dir.join("kit.png"));
    assert!(moved.exists());

    delete(&copy).unwrap();
    assert!(!copy.exists(), "moved to the recycle bin");
    assert!(
        delete(&copy).is_err(),
        "a second delete reports the missing file"
    );

    fs::remove_dir_all(&dir).ok();
    fs::remove_dir_all(&other).ok();
}

#[test]
fn a_name_without_an_extension_is_kept_whole() {
    let path = PathBuf::from("notes");
    assert_eq!(stem(&path), "notes");
    assert_eq!(extension(&path), "");
    assert_eq!(file_name(&path), "notes");
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
        copy_to(&source, &target).unwrap(),
        target.join("cat (2).png")
    );
    assert_eq!(
        move_to(&source, &target).unwrap(),
        target.join("cat (3).png")
    );
    assert_eq!(fs::read(target.join("cat.png")).unwrap(), b"original");
    assert_eq!(fs::read(target.join("cat (2).png")).unwrap(), b"source");
    assert_eq!(fs::read(target.join("cat (3).png")).unwrap(), b"source");
    assert!(!source.exists());

    for file in ["cat.png", "cat (2).png", "cat (3).png"] {
        fs::remove_file(target.join(file)).unwrap();
    }
    fs::remove_file(dir.join("other.png")).unwrap();
    fs::remove_dir(target).unwrap();
    fs::remove_dir(dir).unwrap();
}
