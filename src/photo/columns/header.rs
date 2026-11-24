//! The image header behind the columns: one bounded read per file.
//!
//! Mirrors the limits `metadata.rs` uses for Image Info, with one exception: a
//! JPEG is asked for its marker segments, because its decoder buffers the whole
//! compressed file before it can answer `dimensions()`.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use image::ImageDecoder;

use super::{image_type, jpeg_header, Details, MAX_METADATA_ALLOC, MAX_METADATA_INPUT};

impl Details {
    /// The header guess is a small read; dimensions are the bounded part. Detected
    /// type wins; the extension answers when the header says nothing.
    pub(super) fn header(&mut self, path: &Path, file: File) {
        let mut input = BufReader::new(file);
        let Ok(prefix) = input.fill_buf() else {
            return;
        };
        let detected = image::guess_format(prefix).ok();
        if let Some(format) = detected {
            self.image_type = Some(image_type::detected(format));
        }
        let Some(format) = detected.or_else(|| image::ImageFormat::from_path(path).ok()) else {
            return;
        };
        let Some(size) = self.size.filter(|&size| size <= MAX_METADATA_INPUT) else {
            return;
        };
        if format == image::ImageFormat::Jpeg {
            // Never fall back to a decode here: no header answer, no dimensions.
            if let Some((width, height)) = jpeg_header::dimensions(&mut input, size) {
                self.width = Some(width);
                self.height = Some(height);
            }
            return;
        }
        let mut reader = image::ImageReader::with_format(input, format);
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(MAX_METADATA_ALLOC);
        reader.limits(limits);
        if let Ok(decoder) = reader.into_decoder() {
            let (width, height) = decoder.dimensions();
            self.width = Some(width);
            self.height = Some(height);
        }
    }
}
