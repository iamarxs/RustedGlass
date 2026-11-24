//! Owns the memory DC and uploaded DIB pixels for a bitmap.

use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
};

pub(super) struct Dib {
    pub(super) dc: HDC,
    pub(super) bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut u8,
}

impl Dib {
    /// A top-down 32-bit DIB section with its own memory DC, uploaded with `pixels`.
    pub(super) unsafe fn create(
        reference: HDC,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<Dib, String> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32), // negative height = top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0,
                ..Default::default()
            },
            bmiColors: [Default::default()],
        };

        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let bitmap =
            unsafe { CreateDIBSection(Some(reference), &info, DIB_RGB_COLORS, &mut bits, None, 0) }
                .map_err(|e| format!("CreateDIBSection failed: {e}"))?;

        let dc = unsafe { CreateCompatibleDC(Some(reference)) };
        if bits.is_null() || dc.is_invalid() {
            let _ = unsafe { DeleteObject(bitmap.into()) };
            if !dc.is_invalid() {
                let _ = unsafe { DeleteDC(dc) };
            }
            return Err("Could not create a bitmap drawing surface".into());
        }
        let previous = unsafe { SelectObject(dc, bitmap.into()) };
        if previous.is_invalid() {
            let _ = unsafe { DeleteDC(dc) };
            let _ = unsafe { DeleteObject(bitmap.into()) };
            return Err("Could not select the bitmap for drawing".into());
        }
        let surface = Dib {
            dc,
            bitmap,
            previous,
            bits: bits.cast::<u8>(),
        };
        unsafe { surface.upload(pixels) };
        Ok(surface)
    }

    /// Rewrites a same-size surface without touching its handles.
    pub(super) unsafe fn upload(&self, pixels: &[u8]) {
        unsafe { std::ptr::copy_nonoverlapping(pixels.as_ptr(), self.bits, pixels.len()) };
    }
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
