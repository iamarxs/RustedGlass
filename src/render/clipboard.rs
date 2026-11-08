use super::{Bitmap, BITMAPINFOHEADER};
use std::mem::MaybeUninit;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

const CF_DIB: u32 = 8;

impl Bitmap {
    /// Publish the displayed frame at its native size, including any rotation.
    pub fn copy_to_clipboard(&self, hwnd: HWND) -> windows::core::Result<()> {
        let header = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: self.width as i32,
            biHeight: self.height as i32,
            biPlanes: 1,
            biBitCount: 32,
            ..Default::default()
        };
        let header_size = header.biSize as usize;
        let size = header_size + self.pixel_bytes();
        unsafe {
            let memory = GlobalAlloc(GMEM_MOVEABLE, size)?;
            let result = (|| {
                let bytes = GlobalLock(memory).cast::<u8>();
                if bytes.is_null() {
                    return Err(windows::core::Error::from_win32());
                }
                bytes.cast::<BITMAPINFOHEADER>().write(header);
                let pixels = std::slice::from_raw_parts_mut(
                    bytes.add(header_size).cast::<MaybeUninit<u32>>(),
                    self.pixel_bytes() / 4,
                );
                self.write_dib_pixels(pixels);
                let _ = GlobalUnlock(memory);

                OpenClipboard(Some(hwnd))?;
                let result = EmptyClipboard()
                    .and_then(|()| SetClipboardData(CF_DIB, Some(HANDLE(memory.0))));
                let _ = CloseClipboard();
                result.map(|_| ())
            })();
            if result.is_err() {
                let _ = GlobalFree(Some(memory));
            }
            result
        }
    }

    fn write_dib_pixels(&self, pixels: &mut [MaybeUninit<u32>]) {
        let width = self.width as usize;
        // Positive-height DIBs store the bottom row first.
        for (dest, source) in pixels
            .chunks_exact_mut(width)
            .zip(self.bgra.rchunks_exact(width * 4))
        {
            for (dest, source) in dest.iter_mut().zip(source.chunks_exact(4)) {
                dest.write(u32::from_le_bytes([source[0], source[1], source[2], 255]));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_pixels_are_opaque_bgra_in_bottom_up_order() {
        let bitmap = Bitmap::from_decoded(crate::decode::Decoded {
            width: 2,
            height: 2,
            rgba: vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 0, 0, 0, 0],
        });
        let mut pixels = [MaybeUninit::new(0); 4];
        bitmap.write_dib_pixels(&mut pixels);
        assert_eq!(
            pixels.map(|pixel| unsafe { pixel.assume_init() }),
            [0xff00_00ff, 0xff11_1111, 0xffff_0000, 0xff00_ff00]
        );
    }
}
