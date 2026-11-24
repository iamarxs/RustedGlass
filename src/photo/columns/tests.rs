use super::*;
use crate::photo::PhotoList;
use crate::w32::file_name;
use std::collections::HashMap;

fn details(width: Option<u32>, modified: Option<u64>) -> Details {
    Details {
        width,
        height: None,
        image_type: None,
        size: None,
        modified,
        ..Default::default()
    }
}

#[test]
fn columns_cover_the_list_in_menu_order() {
    assert_eq!(
        Column::ALL.map(Column::title),
        [
            "Name",
            "Full path",
            "Width",
            "Height",
            "Type",
            "Size",
            "Date"
        ]
    );
    for column in Column::ALL {
        assert_eq!(
            column.needs_metadata(),
            !matches!(column, Column::Name | Column::FullPath)
        );
    }
}

#[test]
fn full_path_displays_a_relative_path_as_absolute() {
    let relative = Path::new("album").join("kuva ä.png");
    let absolute = std::path::absolute(&relative).unwrap();
    let shown = Column::FullPath.text(&relative, None);
    assert!(Path::new(&shown).is_absolute());
    assert_eq!(Path::new(&shown), absolute);
}

#[test]
fn a_real_image_yields_header_fields_and_sizes() {
    let path = crate::test_path("ig_columns_test.png");
    let image = image::ImageBuffer::from_fn(2, 1, |_, _| image::Rgba([10, 20, 30, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    fs::write(&path, bytes.into_inner()).unwrap();

    let read = Details::read(&path);
    assert_eq!(read.width, Some(2));
    assert_eq!(read.height, Some(1));
    assert_eq!(read.image_type.as_deref(), Some("PNG"));
    assert_eq!(read.size, Some(fs::metadata(&path).unwrap().len()));
    assert!(
        read.modified.is_some(),
        "the list shows a modification date"
    );
    assert_eq!(read.facts().map(|facts| facts.size), read.size);
    assert!(read.header_read(), "a header read was attempted");

    assert_eq!(
        Column::Width.text(&path, Some(&read)),
        "2",
        "headers, not a decode, answer the Width column"
    );
    assert_eq!(Column::Type.text(&path, None), "", "no details, no wait");
    assert_eq!(
        Column::Name.text(&path, None),
        "ig_columns_test.png",
        "the name never needs details"
    );
    let date = Column::Date.text(&path, Some(&read));
    assert_eq!(
        (date.as_bytes()[4], date.as_bytes()[10]),
        (b'-', b' '),
        "a local date like 2026-11-23 18:25 is shown"
    );

    fs::remove_file(&path).ok();
}

#[test]
fn an_unreadable_file_reports_the_extension_type_and_nothing_else() {
    let path = crate::test_path("ig_columns_test.gif");
    fs::write(&path, b"not an image").unwrap();
    let read = Details::read(&path);
    assert_eq!(read.width, None);
    assert_eq!(read.height, None);
    assert_eq!(
        read.image_type.as_deref(),
        Some("GIF"),
        "the extension guesses when the header cannot"
    );
    assert_eq!(read.size, Some(b"not an image".len() as u64));
    assert!(read.modified.is_some());
    assert!(
        read.header_read(),
        "an invalid file still had its header read tried"
    );
    fs::remove_file(&path).ok();
}

#[test]
fn facts_move_between_details_and_back_without_a_header() {
    let facts = FileFacts {
        size: 123,
        modified: 456,
    };
    let seeded = Details::from_facts(facts);
    assert_eq!(seeded.size, Some(123));
    assert_eq!(seeded.modified, Some(456));
    assert!(
        !seeded.header_read(),
        "a stat-only row never tried the header"
    );
    assert_eq!(seeded.facts(), Some(facts));
    assert!(seeded.width.is_none() && seeded.height.is_none());
    assert!(!Details::default().header_read());
}

#[test]
fn read_with_facts_reuses_the_stat_and_honours_the_header_request() {
    let path = crate::test_path("ig_columns_facts.png");
    let image = image::ImageBuffer::from_fn(2, 1, |_, _| image::Rgba([10, 20, 30, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    fs::write(&path, bytes.into_inner()).unwrap();

    let stats = file_facts(fs::metadata(&path).unwrap()).unwrap();
    // Facts that cannot have come from this file prove they are taken as given.
    let given = FileFacts {
        size: 1,
        modified: 1,
    };
    let stat_only = Details::read_with_facts(&path, Some(given), false);
    assert_eq!(
        stat_only.size,
        Some(1),
        "given facts are used, not a restat"
    );
    assert_eq!(stat_only.modified, Some(1));
    assert_eq!(stat_only.width, None, "no header was asked for");
    assert!(!stat_only.header_read());
    assert_eq!(
        stat_only.image_type.as_deref(),
        Some("PNG"),
        "the extension answers the Type column"
    );

    let full = Details::read_with_facts(&path, None, true);
    assert_eq!(full.width, Some(2));
    assert_eq!(full.facts(), Some(stats));
    assert!(full.header_read());

    let reused = Details::read_with_facts(&path, Some(stats), true);
    assert_eq!(
        reused.width,
        Some(2),
        "the header still reads on reused facts"
    );
    assert!(reused.header_read());

    fs::remove_file(&path).ok();
}

#[test]
fn a_missing_file_finishes_its_header_request() {
    let read = Details::read(&crate::test_path("metadata-missing.png"));
    assert!(read.header_read());
    assert!(read.facts().is_none());
    assert!(read.width.is_none());
}

#[test]
fn dates_sort_numerically_and_the_direction_toggles() {
    let mut photos = PhotoList::empty();
    photos.files = ["a.png", "b.png", "c.png"].map(PathBuf::from).to_vec();
    photos.index = 1;
    // Names say a, b, c but the dates run c, b, a.
    let now = 1_700_000_000u64;
    let mut by_path = HashMap::new();
    by_path.insert(PathBuf::from("a.png"), details(None, Some(now - 200)));
    by_path.insert(PathBuf::from("b.png"), details(None, Some(now)));
    by_path.insert(PathBuf::from("c.png"), details(None, Some(now - 100)));

    photos.sort_by_column(Column::Date, false, &by_path);
    assert_eq!(
        photos.files,
        ["a.png", "c.png", "b.png"].map(PathBuf::from).to_vec(),
        "the calendar, not the name, decides"
    );
    assert_eq!(photos.index, 2, "the shown row follows its picture");
    assert_eq!(photos.current(), Some(&PathBuf::from("b.png")));

    photos.sort_by_column(Column::Date, true, &by_path);
    assert_eq!(
        photos.files,
        ["b.png", "c.png", "a.png"].map(PathBuf::from).to_vec()
    );
    assert_eq!(photos.current(), Some(&PathBuf::from("b.png")));
}

#[test]
fn sizes_sort_numerically_not_as_text() {
    let mut photos = PhotoList::empty();
    photos.files = ["1.png", "2.png", "10.png", "3.png"]
        .map(PathBuf::from)
        .to_vec();
    photos.index = 0;
    let with_size = |size| Details {
        width: None,
        height: None,
        image_type: None,
        size: Some(size),
        modified: None,
        ..Default::default()
    };
    let sizes = [100u64, 20, 9, 12];
    assert_eq!(sizes.map(size_text), ["1 KB", "1 KB", "1 KB", "1 KB"]);
    assert_eq!([0, 1535, 1536].map(size_text), ["0 KB", "1 KB", "2 KB"]);
    let mut by_path = HashMap::new();
    for (name, size) in photos.files.iter().zip(sizes) {
        by_path.insert(name.clone(), with_size(size));
    }

    photos.sort_by_column(Column::Size, false, &by_path);
    assert_eq!(
        photos
            .files
            .iter()
            .map(|path| file_name(path))
            .collect::<Vec<_>>(),
        vec!["10.png", "3.png", "2.png", "1.png"],
        "sizes with the same displayed value still sort by bytes"
    );
    assert_eq!(photos.current(), Some(&PathBuf::from("1.png")));
}

#[test]
fn sorting_by_width_keeps_the_selected_occurrence_of_a_duplicate() {
    let mut photos = PhotoList::empty();
    photos.files = ["a.png", "b.png", "a.png"].map(PathBuf::from).to_vec();
    photos.index = 2;
    let mut by_path = HashMap::new();
    by_path.insert(PathBuf::from("a.png"), details(Some(10), None));
    by_path.insert(PathBuf::from("b.png"), details(Some(9), None));

    photos.sort_by_column(Column::Width, false, &by_path);
    // The sort is stable, so the shown occurrence stays the shown one.
    assert_eq!(photos.index, 2);
    assert_eq!(photos.current(), Some(&PathBuf::from("a.png")));
}

#[test]
fn duplicate_removal_keeps_the_first_occurrence_and_the_picture() {
    let mut photos = PhotoList::empty();
    photos.files = ["a.png", "b.png", "a.png", "c.png", "a.png"]
        .map(PathBuf::from)
        .to_vec();
    photos.index = 4; // the last a.png is shown

    assert_eq!(photos.remove_duplicates(), 2, "later rows of a path go");
    assert_eq!(
        photos.files,
        ["a.png", "b.png", "c.png"].map(PathBuf::from).to_vec()
    );
    assert_eq!(
        photos.current(),
        Some(&PathBuf::from("a.png")),
        "the same picture is still shown, by the kept row"
    );
    assert_eq!(photos.index, 0);
    assert_eq!(photos.remove_duplicates(), 0, "nothing left to dedupe");
}

#[test]
fn removal_keeps_the_row_when_the_picture_was_already_first() {
    let mut photos = PhotoList::empty();
    photos.files = ["a.png", "a.png", "b.png"].map(PathBuf::from).to_vec();
    photos.index = 0;

    assert_eq!(photos.remove_duplicates(), 1);
    assert_eq!(photos.files, ["a.png", "b.png"].map(PathBuf::from).to_vec());
    assert_eq!(photos.index, 0);
    assert_eq!(photos.current(), Some(&PathBuf::from("a.png")));
}
