use std::{fs::File, io::BufReader, path::Path};

use image::ImageDecoder;
use windows::core::{w, PCWSTR};
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};

const MAX_METADATA_INPUT: u64 = 64 * 1024 * 1024;
const MAX_METADATA_ALLOC: u64 = 32 * 1024 * 1024;

pub fn show(hwnd: windows::Win32::Foundation::HWND, text: &str) {
    let text = crate::w32::wide(text);
    let _ = unsafe {
        MessageBoxW(
            Some(hwnd),
            PCWSTR(text.as_ptr()),
            w!("Image information"),
            MB_OK | MB_ICONINFORMATION,
        )
    };
}

pub(crate) fn describe(path: &Path) -> String {
    describe_with_limit(path, MAX_METADATA_INPUT)
}

fn describe_with_limit(path: &Path, limit: u64) -> String {
    let full_path = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
    let mut info = vec![format!("Path: {}", full_path.display())];
    let Ok(file) = File::open(path) else {
        info.push("Size: unavailable\nFormat: unknown".into());
        return info.join("\n");
    };
    let Ok(stat) = file.metadata() else {
        info.push("Size: unavailable\nFormat: unknown".into());
        return info.join("\n");
    };
    let size = stat.len();
    info.push(format!("Size: {size} bytes"));
    if !stat.is_file() {
        info.push("Format: unknown".into());
        return info.join("\n");
    }

    // Only report a format when the small header read identifies it.
    let Ok(mut reader) = image::ImageReader::new(BufReader::new(file)).with_guessed_format() else {
        info.push("Format: unknown".into());
        return info.join("\n");
    };
    let detected = reader.format();
    info.push(format!(
        "Format: {}",
        detected.map_or_else(
            || "unknown".into(),
            |format| format!("{format:?}").to_ascii_uppercase()
        )
    ));
    if size > limit {
        info.push(format!(
            "Details skipped for files larger than {limit} bytes."
        ));
        return info.join("\n");
    }
    if let Some(format) = detected.or_else(|| image::ImageFormat::from_path(path).ok()) {
        reader.set_format(format);
    }
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_METADATA_ALLOC);
    reader.limits(limits);
    let Ok(mut decoder) = reader.into_decoder() else {
        info.push("Image details are unavailable.".into());
        return info.join("\n");
    };
    let (width, height) = decoder.dimensions();
    info.push(format!("Dimensions: {width} × {height}"));
    if let Ok(Some(exif)) = decoder.exif_metadata() {
        let fields = camera_fields(&exif);
        info.extend(fields);
        if let Some(orientation) = image::metadata::Orientation::from_exif_chunk(&exif) {
            info.push(format!("EXIF orientation {}", orientation.to_exif()));
        }
    }
    info.join("\n")
}

fn camera_fields(exif: &[u8]) -> Vec<String> {
    let little = match exif.get(..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return Vec::new(),
    };
    let short = |at: usize| -> Option<u16> {
        let bytes = exif.get(at..)?.get(..2)?.try_into().ok()?;
        Some(if little {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        })
    };
    let long = |at: usize| -> Option<usize> {
        let bytes = exif.get(at..)?.get(..4)?.try_into().ok()?;
        Some(
            (if little {
                u32::from_le_bytes(bytes)
            } else {
                u32::from_be_bytes(bytes)
            }) as usize,
        )
    };
    let field = |tag| -> Option<String> {
        if short(2)? != 42 {
            return None;
        }
        let start = long(4)?;
        let count = short(start)? as usize;
        let entries = exif.get(start.checked_add(2)?..)?.chunks_exact(12);
        for (index, entry) in entries.take(count).enumerate() {
            let offset = start + 2 + index * 12;
            if short(offset)? != tag || short(offset + 2)? != 2 {
                continue;
            }
            let len = long(offset + 4)?;
            let bytes = if len <= 4 {
                &entry[8..][..len]
            } else {
                exif.get(long(offset + 8)?..)?.get(..len)?
            };
            let text = std::str::from_utf8(bytes.split(|byte| *byte == 0).next()?)
                .ok()?
                .trim();
            if !text.is_empty() {
                return Some(text.chars().filter(|c| !c.is_control()).take(100).collect());
            }
        }
        None
    };
    [0x010f, 0x0110, 0x0132]
        .into_iter()
        .filter_map(field)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_information_detects_jpeg_bytes_in_a_png_named_file() {
        let path = crate::test_path("metadata-format.png");
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode_image(&image::RgbImage::new(9, 4))
            .unwrap();
        std::fs::write(&path, &jpeg).unwrap();
        let info = describe(&path);
        let limited = describe_with_limit(&path, 1);
        std::fs::remove_file(&path).unwrap();
        assert!(info.contains(&std::path::absolute(&path).unwrap().display().to_string()));
        assert!(info.contains(&format!("{} bytes", jpeg.len())));
        assert!(info.contains("Format: JPEG"));
        assert!(info.contains("Dimensions: 9 × 4"));
        assert!(limited.contains("Format: JPEG"));
        assert!(limited.contains("Details skipped"));
        assert!(!limited.contains("Dimensions:"));
    }

    #[test]
    fn camera_model_is_read_and_truncated_offsets_are_ignored() {
        let mut exif = b"II\x2a\x00\x08\x00\x00\x00\x01\x00\x10\x01\x02\x00\x05\x00\x00\x00\x16\x00\x00\x00Test\0".to_vec();
        assert_eq!(camera_fields(&exif), ["Test"]);
        exif.pop();
        assert!(camera_fields(&exif).is_empty());
    }
}
