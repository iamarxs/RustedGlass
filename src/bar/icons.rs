use super::Action;
use std::sync::OnceLock;
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{
    SetDIBitsToDevice, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, HDC,
};

const SIZE: usize = 20;
const COUNT: usize = 18;
// Tabler outline icons at 58f60538fe2fab2e1459a09198f9bd3b033bf90e.
// The slots below follow the order of the icons in the embedded atlas.
const ATLAS: &[u8] = include_bytes!("../../assets/toolbar-tabler.png");

fn slot(action: Action) -> Option<usize> {
    Some(match action {
        Action::Settings => 0,       // settings
        Action::ToggleToolBar => 1,  // layout-navbar
        Action::ShowPlaylist => 2,   // list-details
        Action::StartSlideshow => 3, // player-play
        Action::StopSlideshow => 4,  // player-stop
        Action::Previous => 5,       // chevron-left
        Action::Next => 6,           // chevron-right
        Action::RandomOrder => 7,    // arrows-shuffle
        Action::ToggleRepeat => 8,   // repeat
        Action::ZoomIn => 9,         // zoom-in
        Action::ZoomOut => 10,       // zoom-out
        Action::FitOriginal => 11,   // zoom-scan
        Action::ZoomMode(0) => 12,   // focus-auto
        Action::ZoomMode(1) => 13,   // lock
        Action::ZoomMode(2) => 14,   // arrow-autofit-width
        Action::ZoomMode(3) => 15,   // arrow-autofit-height
        Action::ZoomMode(4) => 16,   // maximize
        Action::ZoomMode(5) => 17,   // window-maximize
        _ => return None,
    })
}

fn atlas() -> &'static image::RgbaImage {
    static ICONS: OnceLock<image::RgbaImage> = OnceLock::new();
    ICONS.get_or_init(|| {
        let atlas = image::load_from_memory_with_format(ATLAS, image::ImageFormat::Png)
            .expect("embedded toolbar PNG")
            .into_rgba8();
        assert_eq!(atlas.dimensions(), ((SIZE * COUNT) as u32, SIZE as u32));
        atlas
    })
}

pub(super) fn draw(dc: HDC, rect: RECT, action: Action, fill: COLORREF, ink: COLORREF) {
    let Some(slot) = slot(action) else { return };
    let atlas = atlas();
    let mut pixels = [[0; 4]; SIZE * SIZE];
    let [fill_r, fill_g, fill_b, _] = fill.0.to_le_bytes();
    let [ink_r, ink_g, ink_b, _] = ink.0.to_le_bytes();
    for (i, pixel) in pixels.iter_mut().enumerate() {
        let alpha =
            u16::from(atlas.get_pixel((slot * SIZE + i % SIZE) as u32, (i / SIZE) as u32)[3]);
        let mix =
            |bg: u8, fg: u8| ((u16::from(bg) * (255 - alpha) + u16::from(fg) * alpha) / 255) as u8;
        *pixel = [
            mix(fill_b, ink_b),
            mix(fill_g, ink_g),
            mix(fill_r, ink_r),
            0,
        ];
    }
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: SIZE as i32,
            biHeight: -(SIZE as i32),
            biPlanes: 1,
            biBitCount: 32,
            ..Default::default()
        },
        ..Default::default()
    };
    unsafe {
        let _ = SetDIBitsToDevice(
            dc,
            (rect.left + rect.right - SIZE as i32) / 2,
            (rect.top + rect.bottom - SIZE as i32) / 2,
            SIZE as u32,
            SIZE as u32,
            0,
            0,
            0,
            SIZE as u32,
            pixels.as_ptr().cast(),
            &info,
            DIB_RGB_COLORS,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_toolbar_action_has_a_distinct_visible_icon() {
        use Action::*;
        let actions = [
            Settings,
            ToggleToolBar,
            ShowPlaylist,
            StartSlideshow,
            StopSlideshow,
            Previous,
            Next,
            RandomOrder,
            ToggleRepeat,
            ZoomIn,
            ZoomOut,
            FitOriginal,
            ZoomMode(0),
            ZoomMode(1),
            ZoomMode(2),
            ZoomMode(3),
            ZoomMode(4),
            ZoomMode(5),
        ];
        let mut masks = std::collections::HashSet::new();
        for action in actions {
            let slot = slot(action).unwrap();
            let mask: Vec<_> = (0..SIZE * SIZE)
                .map(|i| atlas().get_pixel((slot * SIZE + i % SIZE) as u32, (i / SIZE) as u32)[3])
                .collect();
            assert!(mask.iter().any(|&alpha| alpha > 128), "{action:?}");
            assert!(masks.insert(mask), "{action:?}");
        }
    }
}
