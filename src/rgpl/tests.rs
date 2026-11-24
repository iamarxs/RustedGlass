use super::*;

#[test]
fn comments_and_blank_lines_are_not_entries() {
    let dir = crate::test_path("rgpl_comments");
    fs::create_dir_all(&dir).unwrap();
    for name in ["a.png", "b.png"] {
        fs::write(dir.join(name), b"x").unwrap();
    }

    let list_file = dir.join("notes.rgpl");
    let text = "\u{feff}# a comment\na.png\n\n# another\nb.png\n\t\n";
    assert_eq!(
        parse(text, &list_file),
        vec![dir.join("a.png"), dir.join("b.png")]
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_relative_path_is_taken_from_the_playlist_folder() {
    let dir = crate::test_path("rgpl_relative");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("a.png"), b"x").unwrap();

    let list_file = dir.join("list.rgpl");
    fs::write(&list_file, "a.png\nmissing.png\n").unwrap();

    assert_eq!(
        read_file(&list_file).unwrap(),
        vec![dir.join("a.png"), dir.join("missing.png")],
        "missing paths are returned for the viewer to report"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn bom_crlf_and_comments_survive_the_streamed_read() {
    let dir = crate::test_path("rgpl_stream_format");
    fs::create_dir_all(&dir).unwrap();
    let list_file = dir.join("crlf.rgpl");
    fs::write(&list_file, "\u{feff}# c\r\na.png\r\n\r\nb.png\r\n").unwrap();

    assert_eq!(
        read_file(&list_file).unwrap(),
        vec![dir.join("a.png"), dir.join("b.png")]
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_non_utf8_file_is_reported() {
    let dir = crate::test_path("rgpl_bad_utf8");
    fs::create_dir_all(&dir).unwrap();
    let list_file = dir.join("bad.rgpl");
    fs::write(&list_file, b"a.png\n\xff\xfe\n").unwrap();

    let error = read_file(&list_file).unwrap_err();
    assert!(error.contains("not valid UTF-8"), "{error}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn both_playlist_formats_write_absolute_paths_and_preserve_order() {
    use std::io::Read;

    let dir = crate::test_path("rgpl_round_trip");
    let outside = crate::test_path("rgpl_round_trip_outside");
    fs::create_dir_all(dir.join("sub")).unwrap();
    fs::create_dir_all(&outside).unwrap();
    for name in ["a.png", "sub/b.png"] {
        fs::write(dir.join(name), b"x").unwrap();
    }
    let away = outside.join("c.png");
    fs::write(&away, b"x").unwrap();

    let list_file = dir.join("saved.rgpl");
    let files = vec![
        dir.join("a.png"),
        dir.join("sub/b.png"),
        away,
        dir.join("a.png"),
        PathBuf::from("another folder").join("relative image.png"),
    ];
    let expected: Vec<_> = files
        .iter()
        .map(|file| std::path::absolute(file).unwrap())
        .collect();
    let package_file = dir.join("saved.rgplz");
    for path in [&list_file, &package_file] {
        assert_eq!(write_file(path, &files).unwrap(), expected.len());
        assert_eq!(
            read_file(path).unwrap(),
            expected,
            "the same list comes back"
        );
    }
    let mut archive = zip::ZipArchive::new(fs::File::open(&package_file).unwrap()).unwrap();
    let mut manifest = String::new();
    archive
        .by_name("playlist.rgpl")
        .unwrap()
        .read_to_string(&mut manifest)
        .unwrap();
    drop(archive);
    for text in [fs::read_to_string(&list_file).unwrap(), manifest] {
        assert_eq!(
            text.lines().map(PathBuf::from).collect::<Vec<_>>(),
            expected
        );
        assert!(text.lines().all(|line| Path::new(line).is_absolute()));
    }

    fs::remove_dir_all(&dir).ok();
    fs::remove_dir_all(&outside).ok();
}

#[test]
fn a_playlist_over_the_old_caps_round_trips() {
    let dir = crate::test_path("rgpl_huge");
    fs::create_dir_all(&dir).unwrap();
    let list_file = dir.join("huge.rgpl");

    // 125,781 entries (over the old 100k cap) with long names, so the text is over 16 MiB too.
    let files: Vec<PathBuf> = (0..125_781)
        .map(|i| dir.join(format!("photo-{i:0200}.png")))
        .collect();
    let count = write_file(&list_file, &files).unwrap();
    assert_eq!(count, 125_781);
    let bytes = fs::metadata(&list_file).unwrap().len();
    assert!(
        bytes > 16 * 1024 * 1024,
        "the test file is {bytes} bytes, over the old 16 MiB cap"
    );

    let read_back = read_file(&list_file).unwrap();
    assert_eq!(read_back, files, "every entry comes back in order");
    drop(read_back);

    let packed_file = dir.join("huge.rgplz");
    assert_eq!(write_file(&packed_file, &files).unwrap(), files.len());
    assert!(fs::metadata(&packed_file).unwrap().len() < bytes / 4);
    assert_eq!(read_file(&packed_file).unwrap(), files);

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_save_that_fails_midway_keeps_the_old_file_and_no_staging_file() {
    let dir = crate::test_path("rgpl_failed_save");
    fs::create_dir_all(&dir).unwrap();
    let list_file = dir.join("keep.rgpl");
    fs::write(&list_file, "keep\n").unwrap();

    // A path that is not valid UTF-8, so turning it into a line fails after the save started.
    let bad_name = {
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt;
            std::ffi::OsString::from_wide(&[0xD800, b'p' as u16, b'.' as u16])
        }
        #[cfg(unix)]
        {
            std::os::unix::ffi::OsStringExt::from_vec(vec![0xFF, b'p'])
        }
    };
    let files = vec![dir.join("good.png"), dir.join(bad_name)];
    let error = write_file(&list_file, &files).unwrap_err();
    assert!(error.contains("not valid UTF-8"), "{error}");

    assert_eq!(fs::read_to_string(&list_file).unwrap(), "keep\n");
    let remaining: Vec<_> = fs::read_dir(&dir).unwrap().collect();
    assert_eq!(remaining.len(), 1, "the staging file is cleaned up");

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn only_plain_or_compressed_playlists_are_recognized() {
    assert!(is_playlist_file(Path::new("my.rgpl")));
    assert!(is_playlist_file(Path::new("my.RGPLZ")));
    assert!(!is_playlist_file(Path::new("my.zip")));
    assert!(!is_playlist_file(Path::new("my.png")));
    assert!(!is_playlist_file(Path::new("no extension")));
}

#[test]
fn a_compressed_playlist_must_have_the_canonical_manifest() {
    let dir = crate::test_path("rgpl_invalid_package");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("wrong.rgplz");
    for names in [
        vec!["nested/playlist.rgpl"],
        vec!["playlist.rgpl", "extra.txt"],
    ] {
        let mut archive = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        for name in names {
            archive
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(b"image.png\n").unwrap();
        }
        drop(archive.finish().unwrap());
        let error = read_file(&path).unwrap_err();
        assert!(error.contains("must contain just playlist.rgpl"), "{error}");
    }
    fs::write(&path, b"broken archive").unwrap();
    assert!(read_file(&path).is_err());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn streamed_rows_keep_order_comments_and_duplicates_in_both_formats() {
    let dir = crate::test_path("rgpl_streamed_order");
    fs::create_dir_all(&dir).unwrap();
    let plain = dir.join("rows.rgpl");
    fs::write(&plain, "# head\nz.png\na.png\n\nz.png\n").unwrap();
    let packed = dir.join("rows.rgplz");
    let absolute: Vec<_> = ["z.png", "a.png", "z.png"]
        .map(|name| std::path::absolute(dir.join(name)).unwrap())
        .into();
    write_file(&packed, &absolute).unwrap();

    for (path, expected) in [
        (
            &plain,
            vec![dir.join("z.png"), dir.join("a.png"), dir.join("z.png")],
        ),
        (&packed, absolute),
    ] {
        let mut rows = Vec::new();
        visit_file(path, &|| true, &mut |row| rows.push(row)).unwrap();
        assert_eq!(rows, expected);
    }
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn cancelling_inside_a_long_row_stops_without_rows_and_reports_success() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let dir = crate::test_path("rgpl_cancel_long_row");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("long.rgpl");
    let mut text = "x".repeat(256 * 1024);
    text.push_str("\nnext.png\n");
    fs::write(&path, text).unwrap();

    let calls = AtomicUsize::new(0);
    let active = || calls.fetch_add(1, Ordering::Relaxed) < 3;
    let mut rows = Vec::new();
    visit_file(&path, &active, &mut |row| rows.push(row)).unwrap();
    assert!(rows.is_empty(), "the long row never completed");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn cancelling_while_decompressing_stops_without_the_remaining_rows() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let dir = crate::test_path("rgpl_cancel_decompress");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("large.rgplz");
    let mut archive = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    archive.start_file("playlist.rgpl", options).unwrap();
    for index in 0..50_000 {
        archive
            .write_all(format!("photo-{index:05}.png\n").as_bytes())
            .unwrap();
    }
    drop(archive.finish().unwrap());

    let rows_seen = AtomicUsize::new(0);
    let active = || rows_seen.load(Ordering::Relaxed) == 0;
    let mut rows = Vec::new();
    visit_file(&path, &active, &mut |row| {
        rows_seen.fetch_add(1, Ordering::Relaxed);
        rows.push(row);
    })
    .unwrap();
    assert_eq!(
        rows.len(),
        1,
        "the first decompressed row arrived, then stopped"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_compressed_playlist_with_damaged_contents_reports_the_checksum() {
    let dir = crate::test_path("rgpl_bad_checksum");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("damaged.rgplz");
    let mut archive = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    archive.start_file("playlist.rgpl", options).unwrap();
    let entry = b"image.png\n";
    archive.write_all(entry).unwrap();
    drop(archive.finish().unwrap());

    let mut bytes = fs::read(&path).unwrap();
    let start = bytes
        .windows(entry.len())
        .position(|data| data == entry)
        .unwrap();
    bytes[start] = b'I';
    fs::write(&path, bytes).unwrap();
    let error = read_file(&path).unwrap_err();
    assert!(error.contains("checksum"), "{error}");
    fs::remove_dir_all(&dir).unwrap();
}
