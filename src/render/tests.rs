use super::*;
use windows::Win32::Graphics::Gdi::{GetDC, ReleaseDC, COLORONCOLOR, HALFTONE};

/// A real DIB section to draw into and read back, so surface checks never go through a mock.
struct Canvas {
    reference: HDC,
    hdc: HDC,
    bitmap: HBITMAP,
    bits: *mut u32,
    width: u32,
    height: u32,
}

impl Canvas {
    unsafe fn new(width: u32, height: u32) -> Canvas {
        use std::ffi::c_void;
        use std::mem::size_of;
        use std::ptr::null_mut;

        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: DIB_RGB_COLORS.0 as u32,
                ..Default::default()
            },
            ..Default::default()
        };
        let reference = unsafe { GetDC(None) };
        let mut bits: *mut c_void = null_mut();
        let bitmap =
            unsafe { CreateDIBSection(Some(reference), &info, DIB_RGB_COLORS, &mut bits, None, 0) }
                .unwrap();
        let hdc = unsafe { CreateCompatibleDC(Some(reference)) };
        let _ = unsafe { SelectObject(hdc, bitmap.into()) };
        assert!(!bits.is_null() && !hdc.is_invalid());
        Canvas {
            reference,
            hdc,
            bitmap,
            bits: bits.cast::<u32>(),
            width,
            height,
        }
    }

    fn at(&self, x: u32, y: u32) -> u32 {
        unsafe { *(self.bits.add((y * self.width + x) as usize)) }
    }

    fn clear(&self) {
        let pixels = (self.width * self.height) as usize;
        unsafe { std::slice::from_raw_parts_mut(self.bits, pixels) }.fill(0);
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(self.hdc);
            let _ = DeleteObject(self.bitmap.into());
            let _ = ReleaseDC(None, self.reference);
        }
    }
}

#[test]
fn opaque_and_transparent_pixels_pack_as_bgra() {
    let decoded = Decoded {
        width: 2,
        height: 1,
        rgba: vec![255, 0, 0, 255, 0, 255, 0, 0],
    };
    let source = decoded.rgba.as_ptr();
    let bitmap = Bitmap::from_decoded(decoded);
    assert_eq!(
        bitmap.bgra.as_ptr(),
        source,
        "conversion reuses the decoded allocation"
    );

    assert_eq!(&bitmap.bgra[..4], &[0, 0, 255, 0]); // red in bits 16-23: GDI's DIB is B, G, R, A in memory
    assert_eq!(&bitmap.bgra[4..], &[17, 17, 17, 0]); // transparent green, flattened onto the background
}

#[test]
fn rotation_swaps_dimensions_and_turns_the_image_clockwise() {
    let decoded = Decoded {
        width: 2,
        height: 1,
        rgba: vec![1, 0, 0, 255, 2, 0, 0, 255],
    };
    let mut bitmap = Bitmap::from_decoded(decoded);
    bitmap.rotate_90_clockwise();

    assert_eq!((bitmap.width, bitmap.height), (1, 2));
    assert_eq!(bitmap.bgra.as_slice(), &[0, 0, 1, 0, 0, 0, 2, 0]); // left pixel moves to the top row
}

#[test]
fn fitting_enlarges_small_images_and_shrinks_large_ones() {
    assert!((fit_factor((10, 10), (100, 100)) - 10.0).abs() < 1e-9);
    assert!((fit_factor((1000, 500), (100, 100)) - 0.1).abs() < 1e-9);
}

/// Draws into a DIB section of a memory DC, so this checks the pixels `draw` really
/// produces, without a window.
#[test]
fn draw_places_the_picture_by_scale_and_offset() {
    use std::ffi::c_void;
    use std::ptr::null_mut;
    use windows::Win32::Graphics::Gdi::{GetDC, ReleaseDC};

    let (screen_w, screen_h) = (200u32, 100u32);
    let decoded = Decoded {
        width: 10,
        height: 10,
        rgba: (0..10 * 10).flat_map(|_| [255u8, 0, 0, 255]).collect(),
    };
    let mut source = Bitmap::from_decoded(decoded);
    let red = 0x00FF_0000u32;

    unsafe {
        let reference = GetDC(None);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: screen_w as i32,
                biHeight: -(screen_h as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: DIB_RGB_COLORS.0 as u32,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = null_mut();
        let dib =
            CreateDIBSection(Some(reference), &info, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
        let hdc = CreateCompatibleDC(Some(reference));
        let _ = SelectObject(hdc, dib.into());
        let pixels = (screen_w * screen_h) as usize;
        let at = |x: usize, y: usize| *(bits as *const u32).add(y * screen_w as usize + x);
        let clear = || std::slice::from_raw_parts_mut(bits as *mut u32, pixels).fill(0);

        clear();
        draw(
            hdc,
            &mut source,
            (screen_w, screen_h),
            1.0,
            (0, 0),
            HALFTONE,
        )
        .unwrap();
        assert_eq!(at(100, 50), red, "100% draws the picture at its own size");
        assert_ne!(at(40, 50), red, "the area around it stays untouched");

        clear();
        draw(
            hdc,
            &mut source,
            (screen_w, screen_h),
            1.0,
            (50, 0),
            HALFTONE,
        )
        .unwrap();
        assert_eq!(at(150, 50), red, "the offset moves the picture");
        assert_ne!(at(100, 50), red, "and leaves its old place behind");

        clear();
        draw(
            hdc,
            &mut source,
            (screen_w, screen_h),
            0.0,
            (0, 0),
            HALFTONE,
        )
        .unwrap();
        assert_eq!(
            at(51, 5),
            red,
            "fit fills the shorter axis: 100x100 at x=50"
        );
        assert_ne!(at(49, 5), red, "but keeps the aspect ratio");

        for (scale, offset, delta, left, right) in [
            (1.0, (0, 0), (-500, 0), 0, 9),
            (1.0, (0, 0), (500, 0), 190, 199),
            (30.0, (0, 0), (-500, 0), 0, 199),
            (30.0, (0, 0), (500, 0), 0, 199),
            (1.0, (-100, 0), (-500, 0), 0, 4),
            (1.0, (-100, 0), (500, 0), 190, 199),
        ] {
            clear();
            let screen = (screen_w, screen_h);
            let offset = pan_offset((10, 10), screen, scale, offset, delta);
            draw(hdc, &mut source, screen, scale, offset, HALFTONE).unwrap();
            let painted: Vec<_> = (0..screen_w as usize)
                .filter(|&x| at(x, screen_h as usize / 2) == red)
                .collect();
            assert_eq!(painted, (left..=right).collect::<Vec<_>>());
        }

        let _ = DeleteDC(hdc);
        let _ = DeleteObject(dib.into());
        let _ = ReleaseDC(None, reference);
    }
}

#[test]
fn drawing_is_centred_and_shifted_by_the_pan_offset() {
    assert_eq!(
        dest_rect((100, 100), (1000, 800), 0.0, (0, 0)),
        (100, 0, 800, 800)
    );
    assert_eq!(
        dest_rect((1000, 800), (1000, 800), 0.5, (0, 0)),
        (250, 200, 500, 400)
    );
    assert_eq!(
        dest_rect((100, 100), (1000, 800), 1.0, (5, -5)),
        (455, 345, 100, 100)
    );
}

fn square(width: u32, height: u32, rgba: [u8; 4]) -> Decoded {
    Decoded {
        width,
        height,
        rgba: rgba.repeat((width * height) as usize),
    }
}

const RED: u32 = 0x00FF_0000;
const BLUE: u32 = 0x0000_00FF;

#[test]
fn a_same_size_frame_rewrites_the_uploaded_surface() {
    let mut bitmap = Bitmap::from_decoded(square(2, 2, [255, 0, 0, 255]));
    let canvas = unsafe { Canvas::new(8, 8) };
    let screen = (8, 8);
    unsafe { draw(canvas.hdc, &mut bitmap, screen, 1.0, (0, 0), COLORONCOLOR) }.unwrap();
    let surface = bitmap.surface_handle().unwrap();
    assert_eq!(canvas.at(3, 3), RED);

    bitmap.replace_decoded(square(2, 2, [0, 0, 255, 255]));
    assert_eq!(
        bitmap.surface_handle(),
        Some(surface),
        "a same-size frame reuses the GDI surface instead of allocating another"
    );

    canvas.clear();
    unsafe { draw(canvas.hdc, &mut bitmap, screen, 1.0, (0, 0), COLORONCOLOR) }.unwrap();
    assert_eq!(
        canvas.at(3, 3),
        BLUE,
        "and the reused surface carries the new frame"
    );
}

#[test]
fn a_resized_or_rotated_frame_drops_the_uploaded_surface() {
    let mut bitmap = Bitmap::from_decoded(square(2, 2, [255, 0, 0, 255]));
    let canvas = unsafe { Canvas::new(16, 16) };
    let screen = (16, 16);
    unsafe { draw(canvas.hdc, &mut bitmap, screen, 1.0, (0, 0), COLORONCOLOR) }.unwrap();
    let first = bitmap.surface_handle().unwrap();

    bitmap.replace_decoded(square(4, 4, [0, 0, 255, 255]));
    assert_eq!(
        bitmap.surface_handle(),
        None,
        "a new size invalidates the surface rather than stretching stale pixels"
    );

    canvas.clear();
    unsafe { draw(canvas.hdc, &mut bitmap, screen, 1.0, (0, 0), COLORONCOLOR) }.unwrap();
    assert_ne!(bitmap.surface_handle().unwrap(), first);
    assert_eq!(canvas.at(8, 8), BLUE);
    assert_ne!(
        canvas.at(1, 1),
        RED,
        "nothing of the old frame is left behind"
    );

    bitmap.rotate_90_clockwise();
    assert_eq!(
        bitmap.surface_handle(),
        None,
        "rotation invalidates it as well"
    );
}

#[test]
fn a_cache_hit_reuses_the_uploaded_surface_of_the_still() {
    let path = crate::test_path("render-surface-test.png");
    std::fs::write(&path, b"only the cache key reads this file").unwrap();
    let mut live = crate::cache::store(&path, square(2, 2, [255, 0, 0, 255]));
    let mut hit = crate::cache::cached(&path).unwrap();
    assert!(live.shares_pixels(&hit));

    let canvas = unsafe { Canvas::new(16, 16) };
    let screen = (16, 16);
    unsafe { draw(canvas.hdc, &mut live, screen, 1.0, (0, 0), COLORONCOLOR) }.unwrap();
    let surface = live.surface_handle().unwrap();
    unsafe { draw(canvas.hdc, &mut hit, screen, 1.0, (0, 0), COLORONCOLOR) }.unwrap();
    assert_eq!(
        hit.surface_handle(),
        Some(surface),
        "a cache hit draws through the surface the still already uploaded"
    );

    hit.replace_decoded(square(2, 2, [0, 0, 255, 255]));
    assert_eq!(
        live.surface_handle(),
        Some(surface),
        "a shared surface is never rewritten"
    );
    assert_eq!(hit.surface_handle(), None);

    std::fs::remove_file(path).unwrap();
}
