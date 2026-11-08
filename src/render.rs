//! Draws bitmaps into a window with GDI: a cached 32-bit DIB section plus a scaled blit.
//!
//! The C# viewer draws with `Graphics.DrawImageUnscaledEx` from a `Format32bppPArgb` bitmap. Here
//! the pixels are packed as BGRA32 (the GDI DIB layout) and scaled with `StretchBlt`.

use std::sync::Arc;

use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, SetStretchBltMode,
    StretchBlt, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, SRCCOPY,
    STRETCH_BLT_MODE,
};

use crate::decode::Decoded;

/// Background used to flatten transparent pixels, matching the viewer's dark theme.
const BACKGROUND: (u8, u8, u8) = (0x11, 0x11, 0x11);

/// RGBA8 pixels plus a lazily created DIB section, drawn scaled into a window.
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    bgra: Arc<Vec<u8>>,
    dib: Option<Dib>,
}

impl Bitmap {
    /// Reuse the decoded allocation, compositing alpha onto the dark background.
    pub fn from_decoded(mut decoded: Decoded) -> Bitmap {
        for pixel in decoded.rgba.chunks_exact_mut(4) {
            if pixel[3] == 255 {
                pixel.swap(0, 2);
                pixel[3] = 0;
                continue;
            }
            let [r, g, b, a] = [pixel[0], pixel[1], pixel[2], pixel[3]];
            let blend = |c: u8, bg: u8| {
                (u16::from(c) * u16::from(a) + u16::from(bg) * u16::from(255 - a)) / 255
            };
            pixel.copy_from_slice(&[
                blend(b, BACKGROUND.2) as u8,
                blend(g, BACKGROUND.1) as u8,
                blend(r, BACKGROUND.0) as u8,
                0,
            ]);
        }
        Bitmap {
            width: decoded.width,
            height: decoded.height,
            bgra: Arc::new(decoded.rgba),
            dib: None,
        }
    }

    pub(crate) fn clone_pixels(&self) -> Self {
        Self {
            width: self.width,
            height: self.height,
            bgra: Arc::clone(&self.bgra),
            dib: None,
        }
    }

    pub(crate) fn pixel_bytes(&self) -> usize {
        self.bgra.len()
    }

    #[cfg(test)]
    pub(crate) fn shares_pixels(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.bgra, &other.bgra)
    }

    /// Rotates the pixels 90° clockwise. EXIF/IFD orientation is applied by the decoders, so this
    /// is the manual `R` rotation only.
    pub fn rotate_90_clockwise(&mut self) {
        let (w, h) = (self.width as usize, self.height as usize);
        let mut rotated = vec![0; self.bgra.len()];

        for y in 0..h {
            for x in 0..w {
                let dest = (x * h + (h - 1 - y)) * 4;
                let source = (y * w + x) * 4;
                rotated[dest..dest + 4].copy_from_slice(&self.bgra[source..source + 4]);
            }
        }

        self.bgra = Arc::new(rotated);
        self.width = h as u32;
        self.height = w as u32;

        self.dib = None;
    }

    /// Reuse a source DC with its DIB already selected.
    unsafe fn source_dc(&mut self, reference: HDC) -> Result<HDC, String> {
        if let Some(dib) = &self.dib {
            return Ok(dib.dc);
        }

        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: self.width as i32,
                biHeight: -(self.height as i32), // negative height = top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0,
                ..Default::default()
            },
            bmiColors: [Default::default()],
        };

        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let dib = CreateDIBSection(Some(reference), &info, DIB_RGB_COLORS, &mut bits, None, 0)
            .map_err(|e| format!("CreateDIBSection failed: {e}"))?;

        let dc = CreateCompatibleDC(Some(reference));
        if bits.is_null() || dc.is_invalid() {
            let _ = DeleteObject(dib.into());
            if !dc.is_invalid() {
                let _ = DeleteDC(dc);
            }
            return Err("Could not create a bitmap drawing surface".into());
        }
        let previous = SelectObject(dc, dib.into());
        if previous.is_invalid() {
            let _ = DeleteDC(dc);
            let _ = DeleteObject(dib.into());
            return Err("Could not select the bitmap for drawing".into());
        }
        std::ptr::copy_nonoverlapping(self.bgra.as_ptr(), bits.cast::<u8>(), self.bgra.len());
        self.dib = Some(Dib {
            dc,
            bitmap: dib,
            previous,
        });
        Ok(dc)
    }
}

struct Dib {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
}

impl Drop for Dib {
    fn drop(&mut self) {
        unsafe {
            let _ = SelectObject(self.dc, self.previous);
            let _ = DeleteDC(self.dc);
            let _ = DeleteObject(self.bitmap.into());
        }
    }
}

/// The factor used when the image is fitted to the window: the largest factor that still fits,
/// so small images are enlarged and large ones are shrunk.
pub fn fit_factor(image: (u32, u32), screen: (u32, u32)) -> f64 {
    (screen.0 as f64 / image.0 as f64).min(screen.1 as f64 / image.1 as f64)
}

/// Where the image is drawn: `scale <= 0.0` fits it to the window, otherwise `scale` is used.
pub fn dest_rect(
    image: (u32, u32),
    screen: (u32, u32),
    scale: f64,
    offset: (i32, i32),
) -> (i32, i32, i32, i32) {
    let factor = if scale > 0.0 {
        scale
    } else {
        fit_factor(image, screen)
    };

    let width = (image.0 as f64 * factor).max(1.0);
    let height = (image.1 as f64 * factor).max(1.0);

    (
        ((screen.0 as f64 - width) / 2.0) as i32 + offset.0,
        ((screen.1 as f64 - height) / 2.0) as i32 + offset.1,
        width as i32,
        height as i32,
    )
}

/// Scales and draws `bitmap` centred in `screen`, shifted by `offset`.
pub unsafe fn draw(
    hdc: HDC,
    bitmap: &mut Bitmap,
    screen: (u32, u32),
    scale: f64,
    offset: (i32, i32),
    stretch_mode: STRETCH_BLT_MODE,
) -> Result<(), String> {
    let source = bitmap.source_dc(hdc)?;
    let _ = unsafe { SetStretchBltMode(hdc, stretch_mode) };

    let (x, y, width, height) = dest_rect((bitmap.width, bitmap.height), screen, scale, offset);
    let result = StretchBlt(
        hdc,
        x,
        y,
        width,
        height,
        Some(source),
        0,
        0,
        bitmap.width as i32,
        bitmap.height as i32,
        SRCCOPY,
    );

    result.ok().map_err(|e| format!("StretchBlt failed: {e}"))
}

mod clipboard;

#[cfg(test)]
mod tests;
