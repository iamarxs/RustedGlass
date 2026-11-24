//! Draws bitmaps into a window with GDI: a cached 32-bit DIB section plus a scaled blit.
//!
//! The pixels are packed as BGRA32 (the GDI DIB layout) and scaled with `StretchBlt`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

#[cfg(test)]
use windows::Win32::Graphics::Gdi::HBITMAP;
use windows::Win32::Graphics::Gdi::{
    SetStretchBltMode, StretchBlt, BITMAPINFOHEADER, HDC, SRCCOPY, STRETCH_BLT_MODE,
};

mod surface;
use surface::Dib;

use crate::decode::Decoded;

/// Background used to flatten transparent pixels, matching the viewer's dark theme.
const BACKGROUND: (u8, u8, u8) = (0x11, 0x11, 0x11);

/// RGBA8 pixels plus a lazily created DIB section, drawn scaled into a window.
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    bgra: Arc<Vec<u8>>,
    /// The GDI surface uploaded from `bgra`, created on the first draw and shared by every
    /// bitmap on the same pixels, so a cache hit needs no new handles. A new pixel buffer
    /// always starts without one, so no holder ever draws another bitmap's pixels.
    surface: Rc<RefCell<Option<Dib>>>,
}

impl Bitmap {
    /// Reuse the decoded allocation, compositing alpha onto the dark background.
    pub fn from_decoded(mut decoded: Decoded) -> Bitmap {
        pack(&mut decoded.rgba);
        Bitmap {
            width: decoded.width,
            height: decoded.height,
            bgra: Arc::new(decoded.rgba),
            surface: Rc::new(RefCell::new(None)),
        }
    }

    /// Swaps in the next decoded frame. A frame of the same size rewrites a surface this bitmap
    /// alone owns; any other change drops it, so a stretch never reads a stale surface.
    pub fn replace_decoded(&mut self, mut decoded: Decoded) {
        pack(&mut decoded.rgba);
        let same_size = (decoded.width, decoded.height) == (self.width, self.height);
        let mut surface = if same_size {
            self.unique_surface()
        } else {
            None
        };
        if let Some(dib) = surface.as_mut() {
            unsafe { dib.upload(&decoded.rgba) };
        }
        self.width = decoded.width;
        self.height = decoded.height;
        self.bgra = Arc::new(decoded.rgba);
        self.surface = Rc::new(RefCell::new(surface));
    }

    /// The surface, taken out only when no other bitmap shares these pixels.
    fn unique_surface(&mut self) -> Option<Dib> {
        Rc::get_mut(&mut self.surface)?.get_mut().take()
    }

    pub(crate) fn clone_pixels(&self) -> Self {
        Self {
            width: self.width,
            height: self.height,
            bgra: Arc::clone(&self.bgra),
            surface: Rc::clone(&self.surface),
        }
    }

    pub(crate) fn pixel_bytes(&self) -> usize {
        self.bgra.len()
    }

    #[cfg(test)]
    pub(crate) fn shares_pixels(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.bgra, &other.bgra)
    }

    #[cfg(test)]
    pub(crate) fn surface_handle(&self) -> Option<HBITMAP> {
        self.surface.borrow().as_ref().map(|dib| dib.bitmap)
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

        self.surface = Rc::new(RefCell::new(None));
    }

    /// The source DC with this bitmap's DIB selected, uploading the pixels on first use.
    unsafe fn source_dc(&self, reference: HDC) -> Result<HDC, String> {
        let mut surface = self
            .surface
            .try_borrow_mut()
            .map_err(|_| "The bitmap surface is already in use".to_string())?;
        if let Some(dib) = surface.as_ref() {
            return Ok(dib.dc);
        }
        let dib = unsafe { Dib::create(reference, self.width, self.height, &self.bgra)? };
        Ok(surface.insert(dib).dc)
    }
}

/// Composite alpha onto the dark background and pack the result as BGRA32, in place.
fn pack(rgba: &mut [u8]) {
    for pixel in rgba.chunks_exact_mut(4) {
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
        (((screen.0 as f64 - width) / 2.0) as i32).saturating_add(offset.0),
        (((screen.1 as f64 - height) / 2.0) as i32).saturating_add(offset.1),
        width as i32,
        height as i32,
    )
}

/// Keep a fitted image inside the canvas and let an oversized image pan until its edge meets
/// the corresponding canvas edge. An edge moved past that limit by zoom can only move back in.
pub fn pan_offset(
    image: (u32, u32),
    screen: (u32, u32),
    scale: f64,
    offset: (i32, i32),
    delta: (i32, i32),
) -> (i32, i32) {
    let (x, y, width, height) = dest_rect(image, screen, scale, (0, 0));
    (
        pan_axis(x, width, screen.0 as i32, offset.0, delta.0),
        pan_axis(y, height, screen.1 as i32, offset.1, delta.1),
    )
}

/// The offsets a `canvas`-sized area accepts for an image `extent` wide drawn at `centered`.
fn axis_bounds(centered: i32, extent: i32, canvas: i32) -> (i32, i32) {
    let slack = canvas - extent;
    (
        slack.min(0).saturating_sub(centered),
        slack.max(0).saturating_sub(centered),
    )
}

fn pan_axis(centered: i32, extent: i32, canvas: i32, offset: i32, delta: i32) -> i32 {
    let (min, max) = axis_bounds(centered, extent, canvas);
    offset
        .saturating_add(delta)
        .clamp(min.min(offset), max.max(offset))
}

/// The offset that keeps the image point under `anchor`, a client point, on the same spot while
/// the factor goes from `from` to `to`, as far as the pan limits of `to` allow. `from` or `to`
/// of `0.0` mean the fitted factor, like `dest_rect`; an unknown size keeps `offset`.
pub fn zoom_offset(
    image: (u32, u32),
    screen: (u32, u32),
    from: f64,
    to: f64,
    offset: (i32, i32),
    anchor: (i32, i32),
) -> (i32, i32) {
    if image.0 == 0 || image.1 == 0 || screen.0 == 0 || screen.1 == 0 {
        return offset;
    }
    let fitted = |scale: f64| {
        if scale > 0.0 {
            scale
        } else {
            fit_factor(image, screen)
        }
    };
    let (from, to) = (fitted(from), fitted(to));
    if !(from.is_finite() && to.is_finite() && from > 0.0 && to > 0.0) {
        return offset;
    }

    let (x, y, old_width, old_height) = dest_rect(image, screen, from, offset);
    let (centered_x, centered_y, width, height) = dest_rect(image, screen, to, (0, 0));
    let axis = |was: i32, old_extent: i32, centered: i32, extent: i32, canvas: i32, at: i32| {
        let ratio = f64::from(extent) / f64::from(old_extent);
        let wanted = f64::from(at)
            - ((f64::from(at) - f64::from(was)) * ratio).round()
            - f64::from(centered);
        let (min, max) = axis_bounds(centered, extent, canvas);
        (wanted as i32).clamp(min, max)
    };
    (
        axis(x, old_width, centered_x, width, screen.0 as i32, anchor.0),
        axis(y, old_height, centered_y, height, screen.1 as i32, anchor.1),
    )
}

/// The offset that keeps the picture on the canvas: an offset within the pan limits is kept
/// as is, so an in-bounds pan is never disturbed. Unknown sizes keep the offset.
pub fn clamped_offset(
    image: (u32, u32),
    screen: (u32, u32),
    scale: f64,
    offset: (i32, i32),
) -> (i32, i32) {
    if image.0 == 0 || image.1 == 0 || screen.0 == 0 || screen.1 == 0 {
        return offset;
    }
    let (x, y, width, height) = dest_rect(image, screen, scale, (0, 0));
    let (min_x, max_x) = axis_bounds(x, width, screen.0 as i32);
    let (min_y, max_y) = axis_bounds(y, height, screen.1 as i32);
    (offset.0.clamp(min_x, max_x), offset.1.clamp(min_y, max_y))
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
#[cfg(test)]
mod zoom_tests;
