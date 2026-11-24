use std::{borrow::Cow, path::Path};

use image::ImageFormat;

pub(super) fn detected(format: ImageFormat) -> Cow<'static, str> {
    Cow::Borrowed(match format {
        ImageFormat::Png => "PNG",
        ImageFormat::Jpeg => "JPEG",
        ImageFormat::Gif => "GIF",
        ImageFormat::WebP => "WEBP",
        ImageFormat::Pnm => "PNM",
        ImageFormat::Tiff => "TIFF",
        ImageFormat::Tga => "TGA",
        ImageFormat::Dds => "DDS",
        ImageFormat::Bmp => "BMP",
        ImageFormat::Ico => "ICO",
        ImageFormat::Hdr => "HDR",
        ImageFormat::OpenExr => "OPENEXR",
        ImageFormat::Farbfeld => "FARBFELD",
        ImageFormat::Qoi => "QOI",
        _ => return Cow::Owned(format!("{format:?}").to_ascii_uppercase()),
    })
}

pub(super) fn extension(path: &Path) -> Cow<'static, str> {
    let Some(extension) = path.extension() else {
        return Cow::Borrowed("");
    };
    let extension = extension.to_string_lossy();
    const LABELS: &[&str] = &[
        "APNG", "BMP", "DDS", "EXR", "FARBFELD", "FF", "GIF", "GIFV", "HDR", "ICO", "JFIF", "JPE",
        "JPEG", "JPG", "PAM", "PBM", "PGM", "PNG", "PNM", "PPM", "QOI", "TGA", "TIF", "TIFF",
        "WEBP",
    ];
    LABELS
        .iter()
        .copied()
        .find(|label| label.eq_ignore_ascii_case(&extension))
        .map_or_else(|| Cow::Owned(extension.to_ascii_uppercase()), Cow::Borrowed)
}
