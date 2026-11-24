//! The JPEG marker probe: a frame size without reading the payload.

use super::jpeg_header::dimensions;
use super::Details;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};

/// A reader that counts the bytes read, so a skip cannot hide behind the buffer.
struct Counted<R> {
    inner: R,
    read: u64,
}

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let count = self.inner.read(buf)?;
        self.read += count as u64;
        Ok(count)
    }
}

impl<R: Seek> Seek for Counted<R> {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(position)
    }
}

fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0xFF, marker];
    let size = u16::try_from(payload.len() + 2).unwrap();
    bytes.extend_from_slice(&size.to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

/// A frame header: precision, height, width and one 1x1 component.
fn frame(marker: u8, width: u16, height: u16) -> Vec<u8> {
    let mut payload = vec![8];
    payload.extend_from_slice(&height.to_be_bytes());
    payload.extend_from_slice(&width.to_be_bytes());
    payload.extend_from_slice(&[1, 1, 0x11, 0]);
    segment(marker, &payload)
}

fn scan() -> Vec<u8> {
    segment(0xDA, &[1, 1, 0, 0, 0x3F, 0])
}

fn jpeg(marker: u8, width: u16, height: u16) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xD8];
    bytes.extend(segment(0xE0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0"));
    bytes.extend(frame(marker, width, height));
    bytes.extend(scan());
    bytes.extend(vec![0x12; 4]);
    bytes.extend([0xFF, 0xD9]);
    bytes
}

fn probe(bytes: &[u8]) -> (Option<(u32, u32)>, u64) {
    let counted = Counted {
        inner: Cursor::new(bytes),
        read: 0,
    };
    let mut reader = BufReader::with_capacity(32, counted);
    (
        dimensions(&mut reader, bytes.len() as u64),
        reader.get_ref().read,
    )
}

#[test]
fn a_baseline_frame_answers_without_reading_the_payload() {
    let mut bytes = vec![0xFF, 0xD8];
    // 2.4 MB of EXIF-like segments: skipped by seek, never read.
    for _ in 0..40 {
        bytes.extend(segment(0xE1, &[0x42; 60_000]));
    }
    bytes.extend(frame(0xC0, 1600, 900));
    bytes.extend(scan());
    // A scan tail far larger than any header the walk could need.
    bytes.extend(vec![0xA5; 4 << 20]);

    let (size, read) = probe(&bytes);
    assert_eq!(size, Some((1600, 900)));
    assert!(
        read < 2_048,
        "the walk reads {read} of {} bytes, so it cannot scale with the payload",
        bytes.len()
    );
}

#[test]
fn every_legal_frame_header_answers_and_the_rest_of_the_range_does_not() {
    for marker in [
        0xC0, 0xC1, 0xC2, 0xC3, 0xC5, 0xC6, 0xC7, 0xC9, 0xCA, 0xCB, 0xCD, 0xCE, 0xCF,
    ] {
        let label = format!("SOF {marker:#04X}");
        assert_eq!(probe(&jpeg(marker, 7, 5)).0, Some((7, 5)), "{label}");
    }
    // DHT, JPG and DAC share the marker range but carry no frame size.
    for shared in [0xC4, 0xC8, 0xCC] {
        let mut bytes = vec![0xFF, 0xD8];
        bytes.extend(segment(shared, &[0; 20]));
        bytes.extend(scan());
        assert_eq!(probe(&bytes).0, None, "{shared:#04X} is not a frame header");
    }
}

#[test]
fn a_progressive_frame_answers_with_fill_bytes_before_it() {
    let mut bytes = vec![0xFF, 0xD8];
    bytes.extend([0xFF; 5]); // padding between the SOI and the next marker
    bytes.extend(frame(0xC2, 12, 34));
    bytes.extend(scan());
    assert_eq!(probe(&bytes).0, Some((12, 34)));
}

#[test]
fn bad_truncated_and_scan_first_headers_answer_nothing() {
    assert_eq!(probe(b"not a jpeg at all").0, None, "no SOI");
    assert_eq!(probe(&[0xFF, 0xD8]).0, None, "SOI alone");
    assert_eq!(probe(&[0xFF, 0xD8, 0xFF, 0xD9]).0, None, "end of image");

    let mut past_end = vec![0xFF, 0xD8, 0xFF, 0xE1];
    past_end.extend_from_slice(&40u16.to_be_bytes());
    past_end.extend_from_slice(b"exi");
    assert_eq!(
        probe(&past_end).0,
        None,
        "a segment past the end of the file"
    );

    let mut sos_first = vec![0xFF, 0xD8];
    sos_first.extend(scan());
    sos_first.extend(vec![0; 64]);
    assert_eq!(probe(&sos_first).0, None, "no frame header before the scan");

    let mut cut = vec![0xFF, 0xD8, 0xFF, 0xC0];
    cut.extend_from_slice(&11u16.to_be_bytes());
    cut.extend_from_slice(&[8, 0, 5]);
    assert_eq!(probe(&cut).0, None, "a frame header cut short");

    assert_eq!(probe(&jpeg(0xC0, 0, 5)).0, None, "a zero side is no size");

    // A marker run without end must stop at the bound rather than spin.
    let mut endless = vec![0xFF, 0xD8];
    endless.extend(std::iter::repeat([0xFF, 0x01]).take(8_000).flatten());
    assert_eq!(probe(&endless).0, None, "too many markers");
}

#[test]
fn a_written_jpeg_shows_its_size_in_the_columns() {
    let path = crate::test_path("ig_columns_test.jpg");
    let image = image::ImageBuffer::from_fn(3, 2, |_, _| image::Rgb([10, 20, 30]));
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut bytes, image::ImageFormat::Jpeg)
        .unwrap();
    std::fs::write(&path, bytes.into_inner()).unwrap();

    let read = Details::read(&path);
    assert_eq!((read.width, read.height), (Some(3), Some(2)));
    assert_eq!(read.image_type.as_deref(), Some("JPEG"));
    assert!(read.header_read());
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_broken_jpeg_file_answers_no_size_and_no_decode() {
    let path = crate::test_path("ig_columns_broken.jpg");
    // A JPEG start with one segment cut off: the extension says JPEG, the header
    // cannot answer, and the columns stay blank instead of decoding the file.
    let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE1, 0xFF, 0xFF];
    bytes.extend(vec![7; 128]);
    std::fs::write(&path, bytes).unwrap();

    let read = Details::read(&path);
    assert_eq!((read.width, read.height), (None, None));
    assert_eq!(read.image_type.as_deref(), Some("JPEG"));
    assert!(read.header_read(), "the header was tried, not skipped");
    std::fs::remove_file(&path).ok();
}
