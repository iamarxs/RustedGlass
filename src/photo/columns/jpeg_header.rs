//! JPEG dimensions from the marker segments alone.
//!
//! The `image` JPEG decoder buffers the whole compressed file before it can
//! answer `dimensions()` (`JpegDecoder::new` reads the stream to its end), which
//! is far too much for a playlist column. This probe skips segment payloads
//! and stops at the frame header.

use std::io::{BufReader, Read, Seek};

/// Bounds bytes inspected and separately bounds seeks past segment payloads.
const MAX_READ: u64 = 64 * 1024;
const MAX_MARKERS: u32 = 512;

/// The frame headers that carry dimensions: SOF0..SOF15 except DHT (C4), JPG (C8)
/// and DAC (CC), which reserve markers out of the same range.
fn is_sof(marker: u8) -> bool {
    matches!(
        marker,
        0xC0 | 0xC1 | 0xC2 | 0xC3 | 0xC5 | 0xC6 | 0xC7 | 0xC9 | 0xCA | 0xCB | 0xCD | 0xCE | 0xCF
    )
}

/// The frame size of an otherwise readable JPEG, or `None` for a bad, truncated,
/// scan-first or excessively malformed header. Callers must not decode instead.
pub(super) fn dimensions<R: Read + Seek>(
    reader: &mut BufReader<R>,
    len: u64,
) -> Option<(u32, u32)> {
    let mut probe = Probe {
        reader,
        len,
        position: 0,
        read: 0,
    };
    // SOI: anything else is not a JPEG, whatever its name says.
    if probe.word()? != 0xFFD8 {
        return None;
    }
    for _ in 0..MAX_MARKERS {
        let marker = probe.marker()?;
        let size = match marker {
            // Standalone markers carry no length field.
            0x01 | 0xD8 | 0xD0..=0xD7 => continue,
            // Past the image end or the scan there is no frame header to find.
            0xD9 | 0xDA => return None,
            _ => probe.word()?,
        };
        if size < 2 {
            return None;
        }
        let payload = u64::from(size - 2);
        if probe.position.checked_add(payload)? > len {
            return None;
        }
        if is_sof(marker) {
            // Precision, height, width, component count, then three bytes per component.
            if payload < 9 {
                return None;
            }
            let _precision = probe.byte()?;
            let height = u32::from(probe.word()?);
            let width = u32::from(probe.word()?);
            let components = u64::from(probe.byte()?);
            if components == 0 || payload != 6 + components * 3 {
                return None;
            }
            // A zero side is a deferred (DNL) or broken frame, not a size to show.
            return (width > 0 && height > 0).then_some((width, height));
        }
        // Small segments already buffered need no filesystem seek or extra read.
        probe.reader.seek_relative(payload as i64).ok()?;
        probe.position += payload;
    }
    None
}

/// The bounded byte source: every read is counted, every seek is not.
struct Probe<'a, R> {
    reader: &'a mut BufReader<R>,
    len: u64,
    position: u64,
    read: u64,
}

impl<R: Read + Seek> Probe<'_, R> {
    fn byte(&mut self) -> Option<u8> {
        if self.read >= MAX_READ || self.position >= self.len {
            return None;
        }
        let mut byte = [0u8; 1];
        self.reader.read_exact(&mut byte).ok()?;
        self.read += 1;
        self.position += 1;
        Some(byte[0])
    }

    fn word(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes([self.byte()?, self.byte()?]))
    }

    /// The marker code after its 0xFF, with any fill bytes skipped.
    fn marker(&mut self) -> Option<u8> {
        if self.byte()? != 0xFF {
            return None;
        }
        loop {
            match self.byte()? {
                0xFF => continue,
                // Stuffed zero bytes only live inside a scan, which is never walked.
                0x00 => return None,
                marker => return Some(marker),
            }
        }
    }
}
