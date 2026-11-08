//! Turning image files into pixels: full images and GIF animation frames.
//!
//! Uses the `image` crate rather than Magick.NET/Skia, so the formats `photo.rs`
//! lists but `image` does not decode (RAW, HEIC, JXL, SVG) are reported as errors.

use image::ImageDecoder;
use std::fs;
use std::path::Path;

/// Decoded pixels, top-down RGBA8, the form the Win32 scaler needs.
pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn decode(path: &Path) -> Result<Decoded, String> {
    Ok(to_decoded(load_rgba(path)?))
}

fn load_rgba(path: &Path) -> Result<image::RgbaImage, String> {
    let mut decoder = image::ImageReader::open(path)
        .map_err(|e| format!("{path:?}: {e}"))?
        .with_guessed_format()
        .map_err(|e| format!("{path:?}: {e}"))?
        .into_decoder()
        .map_err(|e| format!("{path:?}: {e}"))?;

    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut image =
        image::DynamicImage::from_decoder(decoder).map_err(|e| format!("{path:?}: {e}"))?;
    image.apply_orientation(orientation);
    Ok(image.into_rgba8())
}

pub fn to_decoded(rgba: image::RgbaImage) -> Decoded {
    Decoded {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    }
}

/// One frame of an animated GIF, already composited onto the full canvas.
pub struct AnimFrame {
    pub image: Decoded,
    pub delay_ms: u32,
}

/// ImageGlass animates `.gif` and `.gifv`.
pub fn is_animated(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()),
        Some(ext) if ext == "gif" || ext == "gifv"
    )
}

/// A GIF delay arrives as a fraction of milliseconds. A frame with no delay is shown
/// for 100ms, and 20ms is the shortest delay that gets honoured.
pub fn frame_delay_ms(numer: u32, denom: u32) -> u32 {
    if numer == 0 || denom == 0 {
        100
    } else {
        (numer / denom).max(20)
    }
}

/// Streams composited GIF frames so long animations do not need a full frame cache.
pub struct Gif {
    path: std::path::PathBuf,
    frames: std::iter::Peekable<image::Frames<'static>>,
}

impl Gif {
    pub fn open(path: &Path) -> Result<(Self, AnimFrame), String> {
        let mut gif = Self {
            path: path.to_owned(),
            frames: gif_frames(path)?.peekable(),
        };
        let first = gif
            .next_frame()?
            .ok_or_else(|| format!("{path:?} has no GIF frames"))?;
        if gif.frames.peek().is_none() {
            return Err(format!("{path:?} is not animated"));
        }
        Ok((gif, first))
    }

    pub fn rewind(&mut self) -> Result<(), String> {
        self.frames = gif_frames(&self.path)?.peekable();
        Ok(())
    }

    pub fn next_frame(&mut self) -> Result<Option<AnimFrame>, String> {
        self.frames
            .next()
            .map(|frame| {
                let frame = frame.map_err(|e| e.to_string())?;
                let (numer, denom) = frame.delay().numer_denom_ms();
                Ok(AnimFrame {
                    image: to_decoded(frame.into_buffer()),
                    delay_ms: frame_delay_ms(numer, denom),
                })
            })
            .transpose()
    }
}

fn gif_frames(path: &Path) -> Result<image::Frames<'static>, String> {
    use image::AnimationDecoder;

    let file = std::io::BufReader::new(fs::File::open(path).map_err(|e| format!("{path:?}: {e}"))?);
    let decoder =
        image::codecs::gif::GifDecoder::new(file).map_err(|e| format!("{path:?}: {e}"))?;
    Ok(decoder.into_frames())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gif_delays_are_bounded() {
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
        assert!(!is_animated(Path::new("cat.png")));
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
        let first = crate::cache::load(&path).unwrap();
        let second = crate::cache::load(&path).unwrap();
        assert!(first.shares_pixels(&second));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_still_file_is_not_an_animation() {
        let path = crate::test_path("ig_still_test.png");
        fs::write(&path, b"not even a png").unwrap();
        assert!(Gif::open(&path).is_err());
        assert!(decode(&path).is_err());
        fs::remove_file(&path).ok();
    }
}
