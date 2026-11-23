//! Tests for `decode.rs`: still decoding, delay bounding, and animation round-trips for
//! GIF, APNG and animated WebP. The `image` crate cannot encode animations, so the APNG and
//! WebP fixtures are hand-assembled around stills its encoders produce.

use super::*;
use image::codecs::png::PngEncoder;
use image::codecs::webp::WebPEncoder;
use image::{ExtendedColorType as Color, ImageEncoder, Rgba, RgbaImage};
use std::fs;
use std::path::Path;

#[test]
fn delays_are_bounded() {
    assert_eq!(frame_delay_ms(100, 1), 100);
    assert_eq!(
        frame_delay_ms(5, 1),
        20,
        "clamped to the shortest usable delay"
    );
    assert_eq!(frame_delay_ms(0, 1), 100, "a missing delay gets a sane one");
    assert_eq!(
        frame_delay_ms(10, 0),
        100,
        "a zero denominator cannot divide"
    );
    assert!(is_animated(Path::new("cat.GIF")));
    assert!(is_animated(Path::new("cat.gifv")));
    assert!(is_animated(Path::new("cat.png")));
    assert!(is_animated(Path::new("cat.webp")));
    assert!(!is_animated(Path::new("cat.jpg")));
}

#[test]
fn a_jpeg_with_exif_rotation_is_decoded_upright() {
    let path = crate::test_path("exif-orientation-test.jpg");
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode_image(&image::RgbImage::new(8, 3))
        .unwrap();
    let exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
    let length = ((exif.len() + 2) as u16).to_be_bytes();
    jpeg.splice(
        2..2,
        [0xff, 0xe1, length[0], length[1]]
            .into_iter()
            .chain(exif.iter().copied()),
    );
    fs::write(&path, jpeg).unwrap();
    let decoded = decode(&path).unwrap();
    assert_eq!((decoded.width, decoded.height), (3, 8));
    let first = crate::cache::store(&path, decoded);
    let second = crate::cache::cached(&path).unwrap();
    assert!(first.shares_pixels(&second));
    fs::remove_file(path).unwrap();
}

#[test]
fn a_still_file_is_not_an_animation() {
    let path = crate::test_path("ig_still_test.png");
    fs::write(&path, b"not even a png").unwrap();
    assert!(open(&path).is_err());
    assert!(decode(&path).is_err());
    fs::remove_file(path).ok();
}

#[test]
fn still_png_and_webp_files_load_as_stills() {
    let image = RgbaImage::from_pixel(4, 2, Rgba([10, 20, 30, 255]));

    let png = crate::test_path("still-test.png");
    fs::write(&png, png_still(&image)).unwrap();
    assert_eq!(decode(&png).unwrap().width, 4);
    match open(&png).unwrap() {
        Opened::Still(still) => {
            assert_eq!((still.width, still.height), (4, 2));
            assert_eq!(pixel(&still, 0, 0), [10, 20, 30, 255]);
        }
        Opened::Animated(..) => panic!("a still PNG opened as an animation"),
    }

    let webp = crate::test_path("still-test.webp");
    fs::write(&webp, webp_still(&image)).unwrap();
    assert_eq!(decode(&webp).unwrap().width, 4);
    match open(&webp).unwrap() {
        Opened::Still(still) => assert_eq!(pixel(&still, 3, 1), [10, 20, 30, 255]),
        Opened::Animated(..) => panic!("a still WebP opened as an animation"),
    }

    fs::remove_file(png).unwrap();
    fs::remove_file(webp).unwrap();
}

#[test]
fn a_single_frame_gif_opens_as_a_still() {
    let path = crate::test_path("still-test.gif");
    let image = RgbaImage::from_pixel(3, 2, Rgba([1, 2, 3, 255]));
    let mut file = fs::File::create(&path).unwrap();
    image::codecs::gif::GifEncoder::new(&mut file)
        .write_image(image.as_raw(), 3, 2, Color::Rgba8)
        .unwrap();
    drop(file);
    match open(&path).unwrap() {
        Opened::Still(still) => {
            assert_eq!((still.width, still.height), (3, 2));
            assert_eq!(pixel(&still, 0, 0), [1, 2, 3, 255]);
        }
        Opened::Animated(..) => panic!("a single-frame GIF opened as an animation"),
    }
    fs::remove_file(path).unwrap();
}

#[test]
fn a_file_whose_content_does_not_match_its_extension_still_opens() {
    let path = crate::test_path("mislabeled.png");
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode_image(&image::RgbImage::new(2, 1))
        .unwrap();
    fs::write(&path, jpeg).unwrap();
    match open(&path).unwrap() {
        Opened::Still(still) => assert_eq!((still.width, still.height), (2, 1)),
        Opened::Animated(..) => panic!("a JPEG named .png opened as an animation"),
    }
    fs::remove_file(path).unwrap();
}

#[test]
fn an_animated_webp_round_trips() {
    let path = crate::test_path("anim-test.webp");
    let red = RgbaImage::from_pixel(4, 4, Rgba([255, 0, 0, 255]));
    let blue = RgbaImage::from_pixel(2, 2, Rgba([0, 0, 255, 255]));
    fs::write(&path, animated_webp(&red, &blue, (2, 2), 100, 50)).unwrap();

    let (mut animation, first) = animation_of(&path);
    assert_eq!((first.image.width, first.image.height), (4, 4));
    assert_eq!(first.delay_ms, 100);
    assert_eq!(pixel(&first.image, 0, 0), [255, 0, 0, 255]);

    let second = animation.next_frame().unwrap().unwrap();
    // Frame 2 is a 2x2 blue patch at (2, 2): the untouched pixels of frame 1 survive it.
    assert_eq!(pixel(&second.image, 2, 2), [0, 0, 255, 255]);
    assert_eq!(pixel(&second.image, 3, 3), [0, 0, 255, 255]);
    assert_eq!(pixel(&second.image, 0, 0), [255, 0, 0, 255]);
    assert_eq!(second.delay_ms, 50);
    assert!(animation.next_frame().unwrap().is_none());

    animation.rewind().unwrap();
    let again = animation.next_frame().unwrap().unwrap();
    assert_eq!(pixel(&again.image, 2, 2), [255, 0, 0, 255]);
    fs::remove_file(path).unwrap();
}

#[test]
fn an_apng_round_trips() {
    let path = crate::test_path("anim-test.png");
    let red = RgbaImage::from_pixel(2, 2, Rgba([255, 0, 0, 255]));
    let blue = RgbaImage::from_pixel(2, 2, Rgba([0, 0, 255, 255]));
    fs::write(&path, animated_png(&red, &blue, 10, 5)).unwrap();
    assert!(matches!(open(&path), Ok(Opened::Animated(..))));

    let (mut animation, first) = animation_of(&path);
    assert_eq!((first.image.width, first.image.height), (2, 2));
    assert_eq!(first.delay_ms, 100);
    assert_eq!(pixel(&first.image, 0, 0), [255, 0, 0, 255]);

    let second = animation.next_frame().unwrap().unwrap();
    assert_eq!(pixel(&second.image, 1, 1), [0, 0, 255, 255]);
    assert_eq!(second.delay_ms, 50);
    assert!(animation.next_frame().unwrap().is_none());

    animation.rewind().unwrap();
    let again = animation.next_frame().unwrap().unwrap();
    assert_eq!(pixel(&again.image, 0, 0), [255, 0, 0, 255]);
    fs::remove_file(path).unwrap();
}

/// The animation side of `open`, for the round-trip tests.
fn animation_of(path: &Path) -> (Animation, AnimFrame) {
    match open(path).unwrap() {
        Opened::Animated(animation, first) => (animation, first),
        Opened::Still(_) => panic!("expected an animation"),
    }
}

fn pixel(frame: &Decoded, x: u32, y: u32) -> [u8; 4] {
    frame.rgba[(y * frame.width + x) as usize * 4..][..4]
        .try_into()
        .unwrap()
}

/// A still PNG encoded with the `image` crate, for the fixtures to cut chunks out of.
fn png_still(image: &RgbaImage) -> Vec<u8> {
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(image.as_raw(), image.width(), image.height(), Color::Rgba8)
        .unwrap();
    png
}

/// A still lossless WebP encoded with the `image` crate.
fn webp_still(image: &RgbaImage) -> Vec<u8> {
    let mut webp = Vec::new();
    WebPEncoder::new_lossless(&mut webp)
        .write_image(image.as_raw(), image.width(), image.height(), Color::Rgba8)
        .unwrap();
    webp
}

/// A RIFF chunk: tag, little-endian size, payload, and a pad byte to an even length.
fn riff_chunk(tag: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut chunk = tag.to_vec();
    chunk.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    chunk.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        chunk.push(0);
    }
    chunk
}

/// The VP8L bitstream of a lossless still WebP, cut out of its RIFF container.
fn vp8l_of(image: &RgbaImage) -> Vec<u8> {
    let webp = webp_still(image);
    let start = webp.windows(4).position(|w| w == b"VP8L").unwrap() + 8;
    let size = u32::from_le_bytes(webp[start - 4..start].try_into().unwrap()) as usize;
    webp[start..start + size].to_vec()
}

/// One ANMF chunk: `image` placed at `offset`, shown for `delay_ms`, overwriting its rectangle.
fn anmf_chunk(offset: (u32, u32), delay_ms: u32, image: &RgbaImage) -> Vec<u8> {
    let vp8l = vp8l_of(image);
    let mut payload = Vec::new();
    for value in [
        offset.0 / 2, // frame offsets are stored halved
        offset.1 / 2,
        image.width() - 1,
        image.height() - 1,
        delay_ms,
    ] {
        payload.extend_from_slice(&value.to_le_bytes()[..3]);
    }
    payload.push(0b10); // do not blend: the frame overwrites its rectangle
    payload.extend_from_slice(b"VP8L");
    payload.extend_from_slice(&(vp8l.len() as u32).to_le_bytes());
    payload.extend_from_slice(&vp8l);
    if vp8l.len() % 2 == 1 {
        payload.push(0); // the pad byte rides inside the declared chunk size
    }
    riff_chunk(b"ANMF", &payload)
}

/// A two-frame animated WebP: `first` covers the canvas, `second` is a partial update.
fn animated_webp(
    first: &RgbaImage,
    second: &RgbaImage,
    offset: (u32, u32),
    first_ms: u32,
    second_ms: u32,
) -> Vec<u8> {
    let mut vp8x = vec![0b0001_0010, 0, 0, 0]; // flags: alpha and animation
    vp8x.extend_from_slice(&(first.width() - 1).to_le_bytes()[..3]);
    vp8x.extend_from_slice(&(first.height() - 1).to_le_bytes()[..3]);

    let mut body = b"WEBP".to_vec();
    body.extend(riff_chunk(b"VP8X", &vp8x));
    body.extend(riff_chunk(b"ANIM", &[0; 6])); // background color, loop count 0 = forever
    body.extend(anmf_chunk((0, 0), first_ms, first));
    body.extend(anmf_chunk(offset, second_ms, second));

    let mut webp = b"RIFF".to_vec();
    webp.extend_from_slice(&(body.len() as u32).to_le_bytes());
    webp.extend(body);
    webp
}

/// A PNG chunk: big-endian size, tag, payload, and the CRC of tag and payload.
fn png_chunk(tag: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut chunk = (payload.len() as u32).to_be_bytes().to_vec();
    chunk.extend_from_slice(tag);
    chunk.extend_from_slice(payload);
    chunk.extend_from_slice(&crc32(&chunk[4..]).to_be_bytes());
    chunk
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// The payload of the first PNG chunk with this tag, cut out of an encoded still.
fn png_chunk_payload<'a>(png: &'a [u8], tag: &[u8; 4]) -> &'a [u8] {
    let mut pos = 8; // the PNG signature
    while pos + 12 <= png.len() {
        let size = u32::from_be_bytes(png[pos..pos + 4].try_into().unwrap()) as usize;
        if &png[pos + 4..pos + 8] == tag {
            return &png[pos + 8..pos + 8 + size];
        }
        pos += 12 + size;
    }
    panic!("no {tag:?} chunk");
}

/// A frame control chunk: sequence number, placement, and a delay in ticks of 1/100 second.
/// The delays of the frames are `10` and `5` ticks, so 100ms and 50ms.
fn fctl_chunk(sequence: u32, image: &RgbaImage, delay_ticks: u16) -> Vec<u8> {
    let mut payload = sequence.to_be_bytes().to_vec();
    payload.extend_from_slice(&image.width().to_be_bytes());
    payload.extend_from_slice(&image.height().to_be_bytes());
    payload.extend_from_slice(&0u32.to_be_bytes()); // x offset
    payload.extend_from_slice(&0u32.to_be_bytes()); // y offset
    payload.extend_from_slice(&delay_ticks.to_be_bytes());
    payload.extend_from_slice(&100u16.to_be_bytes()); // delay denominator
    payload.push(0); // dispose: keep the canvas
    payload.push(0); // blend: source over
    png_chunk(b"fcTL", &payload)
}

/// A two-frame APNG: `first` in the IDAT, `second` in an fdAT reusing a still's data.
fn animated_png(
    first: &RgbaImage,
    second: &RgbaImage,
    first_ticks: u16,
    second_ticks: u16,
) -> Vec<u8> {
    let still = png_still(first);
    let other = png_still(second);

    let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    png.extend(png_chunk(b"IHDR", png_chunk_payload(&still, b"IHDR")));
    png.extend(png_chunk(
        b"acTL",
        &[0, 0, 0, 2, 0, 0, 0, 0], // 2 frames, loop forever
    ));
    png.extend(fctl_chunk(0, first, first_ticks));
    png.extend(png_chunk(b"IDAT", png_chunk_payload(&still, b"IDAT")));
    png.extend(fctl_chunk(1, second, second_ticks));
    let mut fdat = 2u32.to_be_bytes().to_vec(); // fdAT adds a sequence number to the frame data
    fdat.extend_from_slice(png_chunk_payload(&other, b"IDAT"));
    png.extend(png_chunk(b"fdAT", &fdat));
    png.extend(png_chunk(b"IEND", &[]));
    png
}

#[test]
fn absurd_dimensions_are_rejected_before_any_big_allocation() {
    // farbfeld header: magic + u32 BE width + u32 BE height; no pixel data needed,
    // check_dimensions must reject on the header alone.
    let header = |w: u32, h: u32| {
        let mut bytes = b"farbfeld".to_vec();
        bytes.extend_from_slice(&w.to_be_bytes());
        bytes.extend_from_slice(&h.to_be_bytes());
        bytes
    };
    let zero = crate::test_path("ff-zero-width.ff");
    fs::write(&zero, header(0, 5)).unwrap();
    assert!(decode(&zero).is_err());

    let huge = crate::test_path("ff-huge.ff");
    fs::write(&huge, header(100_000, 100_000)).unwrap();
    assert!(decode(&huge).is_err());

    let animated = crate::test_path("anim-huge.gif");
    // minimal GIF87a header declaring a 65535x65535 logical screen
    let mut gif = b"GIF87a".to_vec();
    gif.extend_from_slice(&65535u16.to_le_bytes());
    gif.extend_from_slice(&65535u16.to_le_bytes());
    gif.push(0);
    gif.extend_from_slice(&[0, 0]);
    fs::write(&animated, gif).unwrap();
    assert!(open(&animated).is_err());

    for file in [zero, huge, animated] {
        fs::remove_file(file).unwrap();
    }
}

#[test]
fn an_apng_extension_file_opens_as_an_animation() {
    let path = crate::test_path("anim-test.apng");
    let red = RgbaImage::from_pixel(2, 2, Rgba([255, 0, 0, 255]));
    let blue = RgbaImage::from_pixel(2, 2, Rgba([0, 0, 255, 255]));
    fs::write(&path, animated_png(&red, &blue, 10, 5)).unwrap();
    assert!(is_animated(&path));
    assert!(matches!(open(&path), Ok(Opened::Animated(..))));
    fs::remove_file(path).unwrap();
}

#[test]
fn the_image_budget_counts_every_buffer_with_checked_arithmetic() {
    assert_eq!(budget::buffer_bytes(1, 1), Some(4));
    assert_eq!(
        budget::peak_bytes(100, 50, 100 * 50 * 4),
        Some(100 * 50 * 16),
        "decoder output, RGBA copy, BGRA buffer and DIB are all counted"
    );
    assert_eq!(
        budget::peak_bytes(u32::MAX, u32::MAX, 0),
        None,
        "an overflowed size is never accepted"
    );

    // 8192x4096 pixels peaks at exactly the 512 MiB budget; one more row is refused.
    assert_eq!(budget::MAX_BUDGET % 16, 0);
    assert!(budget::check(8192, 4096, 8192 * 4096).is_ok());
    let error = budget::check(8192, 4097, 8192 * 4097).unwrap_err();
    assert!(error.contains("512 MiB"), "{error}");
    assert!(budget::check(MAX_DIMENSION, MAX_DIMENSION, 0).is_err());
    assert!(budget::check(8192, 4096, 8192 * 4096 * 16).is_err());
}
