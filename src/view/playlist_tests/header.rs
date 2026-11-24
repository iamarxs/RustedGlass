//! The native header's custom draw: a dark row with the F1-pane accent on the sorted
//! column. Let the real header paint
//! into a DIB section and reading the pixels back.

use super::*;
use windows::Win32::Foundation::COLORREF;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC,
};
use windows::Win32::UI::WindowsAndMessaging::{PRF_CLIENT, WM_PRINTCLIENT};

const WIDTH: i32 = 480;
const HEIGHT: i32 = 24;
/// The header row's COLORREF; gray, so the DIB pixel value is the same.
const BACKGROUND: u32 = 0x0025_2525;

/// A COLORREF as a 32-bpp DIB pixel (bytes B, G, R, X in memory).
fn pixel(color: COLORREF) -> u32 {
    let c = color.0;
    ((c & 0xFF) << 16) | (c & 0xFF00) | ((c >> 16) & 0xFF)
}

/// A top-down 32-bpp DIB section with a memory DC selected into it.
struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    bits: *mut std::ffi::c_void,
}

impl Surface {
    fn new(width: i32, height: i32) -> Self {
        unsafe {
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));
            let _ = ReleaseDC(None, screen);
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap =
                CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
            let _ = SelectObject(dc, bitmap.into());
            Self { dc, bitmap, bits }
        }
    }

    fn pixels(&self) -> &[u32] {
        unsafe { std::slice::from_raw_parts(self.bits as *const u32, (WIDTH * HEIGHT) as usize) }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(self.dc);
            let _ = DeleteObject(self.bitmap.into());
        }
    }
}

#[test]
fn header_draws_dark_row_with_accent_on_the_sorted_column() {
    let directory = crate::test_path("playlist-header-dark");
    fs::create_dir_all(&directory).unwrap();
    let first = directory.join("first.ppm");
    let second = directory.join("second.ppm");
    let config = directory.join("rgconfig.json");
    write_image(&first, 2, 3, 1_000_000_000);
    write_image(&second, 7, 3, 1_650_000_000);
    let mut viewer = state::State::interactive(config.clone(), &[]);
    viewer.photos.files = vec![first.clone(), second.clone()];
    state::install(viewer);
    let owner = TestWindow(unsafe { create_window() }.unwrap());
    let _ = unsafe { ShowWindow(owner.0, SW_HIDE) };
    background::attach(owner.0);
    input::apply_action(owner.0, tools::Action::ShowPlaylist);
    let list =
        unsafe { FindWindowExW(Some(owner.0), None, w!("SysListView32"), PCWSTR::null()) }.unwrap();
    let header = HWND(unsafe { SendMessageW(list, LVM_GETHEADER, None, None).0 as *mut _ });
    assert!(unsafe { SendMessageW(header, HDM_GETITEMCOUNT, None, None) }.0 >= 2);
    // Sort by the first column, so the accent indicator lands on item 0.
    click_header(owner.0, list, 0);

    let surface = Surface::new(WIDTH, HEIGHT);
    // The real render: the header paints itself into the DIB through its own paint path.
    let _ = unsafe {
        SendMessageW(
            header,
            WM_PRINTCLIENT,
            Some(WPARAM(surface.dc.0 as usize)),
            Some(LPARAM(PRF_CLIENT as isize)),
        )
    };
    let pixels = surface.pixels();
    assert!(
        pixels.contains(&BACKGROUND),
        "the header background was not painted dark"
    );
    assert!(
        !pixels.contains(&0x00FF_FFFF),
        "the old white header is still showing"
    );
    assert!(
        pixels
            .iter()
            .any(|&p| p == pixel(crate::dialog::TEXT_HEADING)),
        "the sorted column's arrow or text is not in the accent colour"
    );

    drop(owner);
    for path in [first, second, config] {
        let _ = fs::remove_file(path);
    }
    fs::remove_dir(directory).unwrap();
}
