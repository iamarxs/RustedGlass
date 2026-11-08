use std::path::Path;

use image::ImageDecoder;
use windows::core::{w, PCWSTR};
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};

pub fn show(hwnd: windows::Win32::Foundation::HWND) {
    let path = crate::state::with_state(|state| state.photos.current().cloned()).flatten();
    if let Some(path) = path {
        let text = crate::w32::wide(&describe(&path));
        let _ = unsafe {
            MessageBoxW(
                Some(hwnd),
                PCWSTR(text.as_ptr()),
                w!("Image information"),
                MB_OK | MB_ICONINFORMATION,
            )
        };
    }
}

fn describe(path: &Path) -> String {
    let full_path = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
    let mut info = vec![format!("Path: {}", full_path.display())];
    let size = path
        .metadata()
        .map(|metadata| format!("{} bytes", metadata.len()));
    info.push(format!(
        "Size: {}",
        size.unwrap_or_else(|_| "unavailable".into())
    ));

    let Ok(mut reader) = image::ImageReader::open(path) else {
        info.push("Format: unknown".into());
        return info.join("\n");
    };
    let extension_format = reader.format();
    // Only report a format when the file contents identify it.
    reader.clear_format();
    let Ok(mut reader) = reader.with_guessed_format() else {
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
    if let Some(format) = detected.or(extension_format) {
        reader.set_format(format);
    }
    let Ok(mut decoder) = reader.into_decoder() else {
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
        std::fs::remove_file(&path).unwrap();
        assert!(info.contains(&std::path::absolute(&path).unwrap().display().to_string()));
        assert!(info.contains(&format!("{} bytes", jpeg.len())));
        assert!(info.contains("Format: JPEG"));
        assert!(info.contains("Dimensions: 9 × 4"));
    }

    #[test]
    fn camera_model_is_read_and_truncated_offsets_are_ignored() {
        let mut exif = b"II\x2a\x00\x08\x00\x00\x00\x01\x00\x10\x01\x02\x00\x05\x00\x00\x00\x16\x00\x00\x00Test\0".to_vec();
        assert_eq!(camera_fields(&exif), ["Test"]);
        exif.pop();
        assert!(camera_fields(&exif).is_empty());
    }
}
