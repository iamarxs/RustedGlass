//! Turning image files into pixels: full images and animation frames.
//!
//! Uses the `image` crate, so the formats `photo.rs` lists but `image` does not decode
//! (RAW, HEIC, JXL, SVG) are reported as errors.

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

/// Decoders never see an image wider/taller than this, or one whose buffers go over the app
/// budget, so a crafted header cannot ask for a multi-gigabyte allocation. The `image` crate's
/// default limits cap allocations only, not dimensions, and `into_decoder` does not
/// reserve-check the output buffer the way `ImageReader::decode` does — so the dimensions are
/// checked here as well.
const MAX_DIMENSION: u32 = 16384;

fn limits() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(budget::MAX_BUDGET);
    limits
}

/// Rejects dimensions no Win32 bitmap could hold, before any full-size buffer is allocated.
fn check_dimensions(path: &Path, decoder: &impl image::ImageDecoder) -> Result<(), String> {
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(format!(
            "{path:?}: {width}x{height} is too large to display"
        ));
    }
    budget::check(width, height, decoder.total_bytes()).map_err(|error| format!("{path:?}: {error}"))
}

fn load_rgba(path: &Path) -> Result<image::RgbaImage, String> {
    let mut reader = image::ImageReader::open(path)
        .map_err(|e| format!("{path:?}: {e}"))?
        .with_guessed_format()
        .map_err(|e| format!("{path:?}: {e}"))?;
    reader.limits(limits());
    let mut decoder = reader
        .into_decoder()
        .map_err(|e| format!("{path:?}: {e}"))?;
    check_dimensions(path, &decoder)?;

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

/// One frame of an animation (GIF, APNG, animated WebP), already composited onto the full canvas.
pub struct AnimFrame {
    pub image: Decoded,
    pub delay_ms: u32,
}

/// Animated formats: `.gif`, `.gifv`, `.png`/`.apng` (APNG) and `.webp`. A still file of one of
/// these formats opens as `Opened::Still`, so it loads like any other still.
pub fn is_animated(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()),
        Some(ext) if matches!(ext.as_str(), "gif" | "gifv" | "png" | "apng" | "webp")
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

/// A file opened in a single decode: a still, or an animation with its first frame.
pub enum Opened {
    Still(Decoded),
    Animated(Animation, AnimFrame),
}

/// Opens any image file in one decode. The animated formats go through the frame decoders,
/// so a single-frame `.gif`/`.png`/`.webp` arrives as `Still` with its pixels instead of
/// being decoded a second time by the still path. A file whose content does not match its
/// extension falls back to the content-sniffing still decoder, as the still path always could.
pub fn open(path: &Path) -> Result<Opened, String> {
    if !is_animated(path) {
        return decode(path).map(Opened::Still);
    }
    open_animated(path).or_else(|_| decode(path).map(Opened::Still))
}

/// The animated-format path: frame streaming, with a single-frame file as a still.
fn open_animated(path: &Path) -> Result<Opened, String> {
    let (frames, orientation) = anim_frames(path)?;
    let mut animation = Animation {
        path: path.to_owned(),
        frames: frames.peekable(),
    };
    let first = animation
        .next_frame()?
        .ok_or_else(|| format!("{path:?} has no animation frames"))?;
    if animation.frames.peek().is_none() {
        // The one frame is the whole image: it goes back as a still, upright the way the
        // still path would have produced it, instead of being decoded a second time.
        return Ok(Opened::Still(upright(first.image, orientation)));
    }
    Ok(Opened::Animated(animation, first))
}

/// A decoded frame with the orientation the file's metadata carries applied, as the still
/// path does.
fn upright(image: Decoded, orientation: image::metadata::Orientation) -> Decoded {
    let buffer = image::RgbaImage::from_raw(image.width, image.height, image.rgba)
        .expect("a frame buffer matches its own dimensions");
    let mut dynamic = image::DynamicImage::ImageRgba8(buffer);
    dynamic.apply_orientation(orientation);
    to_decoded(dynamic.into_rgba8())
}

/// Streams composited animation frames (GIF, APNG, animated WebP) so long animations do not
/// need a full frame cache.
pub struct Animation {
    path: std::path::PathBuf,
    frames: std::iter::Peekable<image::Frames<'static>>,
}

impl Animation {
    pub fn rewind(&mut self) -> Result<(), String> {
        let (frames, _) = anim_frames(&self.path)?;
        self.frames = frames.peekable();
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

/// The frames of an animated file, behind the decoder the extension calls for, with the
/// orientation the file's metadata carries. All three decoders implement
/// `AnimationDecoder`, so one frame iterator covers every format.
fn anim_frames(
    path: &Path,
) -> Result<(image::Frames<'static>, image::metadata::Orientation), String> {
    use image::AnimationDecoder;

    let file = std::io::BufReader::new(fs::File::open(path).map_err(|e| format!("{path:?}: {e}"))?);
    let error = |e: image::ImageError| format!("{path:?}: {e}");
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    // `GifDecoder`/`PngDecoder` are built with `Limits::no_limits()` inside the crate, so the
    // limits are set here explicitly, and the canvas size is checked before frames stream.
    let (frames, orientation) = match ext.as_deref() {
        Some("png") | Some("apng") => {
            let mut decoder = image::codecs::png::PngDecoder::new(file).map_err(error)?;
            decoder.set_limits(limits()).map_err(error)?;
            check_dimensions(path, &decoder)?;
            let orientation = decoder.orientation().map_err(error)?;
            (decoder.apng().map_err(error)?.into_frames(), orientation)
        }
        Some("webp") => {
            let mut decoder = image::codecs::webp::WebPDecoder::new(file).map_err(error)?;
            decoder.set_limits(limits()).map_err(error)?;
            check_dimensions(path, &decoder)?;
            let orientation = decoder.orientation().map_err(error)?;
            (decoder.into_frames(), orientation)
        }
        _ => {
            let mut decoder = image::codecs::gif::GifDecoder::new(file).map_err(error)?;
            decoder.set_limits(limits()).map_err(error)?;
            check_dimensions(path, &decoder)?;
            let orientation = decoder.orientation().map_err(error)?;
            (decoder.into_frames(), orientation)
        }
    };
    Ok((frames, orientation))
}

/// Sizes and buffers for one image, checked before allocation.
mod budget;

#[cfg(test)]
mod tests;
