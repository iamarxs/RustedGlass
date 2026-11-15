//! Tests for the EXR and farbfeld still-image formats.

use super::*;
use crate::decode;

#[test]
fn exr_and_farbfeld_are_browsable() {
    let formats = image_formats();
    for ext in [".exr", ".farbfeld", ".ff"] {
        assert!(formats.contains(&ext.to_string()), "{ext} missing");
    }
}

#[test]
fn a_hand_written_farbfeld_decodes() {
    let path = crate::test_path("ig_farbfeld_test.ff");
    // magic, then u32 BE width, u32 BE height, then RGBA16 BE per pixel.
    let mut data = b"farbfeld".to_vec();
    data.extend_from_slice(&2u32.to_be_bytes());
    data.extend_from_slice(&1u32.to_be_bytes());
    data.extend_from_slice(&[0xff; 8]); // white, opaque
    data.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0xff, 0xff]); // black, opaque
    fs::write(&path, data).unwrap();

    let decoded = decode::decode(&path).unwrap();
    assert_eq!((decoded.width, decoded.height), (2, 1));
    assert_eq!(decoded.rgba, vec![255, 255, 255, 255, 0, 0, 0, 255]);
    fs::remove_file(&path).ok();
}

#[test]
fn a_written_exr_decodes() {
    let path = crate::test_path("ig_exr_test.exr");
    let hdr = image::ImageBuffer::from_fn(2, 1, |x, _| image::Rgba([x as f32, 0.0, 0.0, 1.0]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba32F(hdr)
        .write_to(&mut bytes, image::ImageFormat::OpenExr)
        .unwrap();
    fs::write(&path, bytes.into_inner()).unwrap();

    let decoded = decode::decode(&path).unwrap();
    assert_eq!((decoded.width, decoded.height), (2, 1));
    assert_eq!(decoded.rgba, vec![0, 0, 0, 255, 255, 0, 0, 255]);
    fs::remove_file(&path).ok();
}

#[test]
fn apng_files_are_browsable() {
    assert!(image_formats().contains(&".apng".to_string()));
    assert!(is_image_file(
        std::path::Path::new("anim.APNG"),
        &image_formats()
    ));
}
