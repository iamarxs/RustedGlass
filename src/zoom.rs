//! Zoom modes, mirroring `ZoomMode` (`UI/Viewer/ViewerControl_Enums.cs`) and
//! `Viewer.CalculateZoomFactor` (`ViewerControl_ZoomAndPan.cs`).
//!
//! A mode describes how the drawing factor is derived from the window size, so a resize keeps
//! behaving the way the user picked. A manual zoom (wheel, `+`/`-`) overrides the mode until the
//! mode changes or a new image is loaded, which is `IsManualZoom` in the C# view.

/// How the image is scaled into the viewer area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomMode {
    /// Original size, unless the image is larger than the area, then shrink to fit.
    AutoZoom,
    /// Keep the factor that is on screen now.
    LockZoom,
    ScaleToWidth,
    ScaleToHeight,
    ScaleToFit,
    ScaleToFill,
}

/// The modes in the order the `1`..`6` hotkeys and the menu use, as in
/// `AppAPIProvider_Hotkeys.cs`.
pub const MODES: [ZoomMode; 6] = [
    ZoomMode::AutoZoom,
    ZoomMode::LockZoom,
    ZoomMode::ScaleToWidth,
    ZoomMode::ScaleToHeight,
    ZoomMode::ScaleToFit,
    ZoomMode::ScaleToFill,
];

impl ZoomMode {
    /// Label of the tool bar button and the menu entry.
    pub fn label(self) -> &'static str {
        match self {
            ZoomMode::AutoZoom => "Auto",
            ZoomMode::LockZoom => "Lock",
            ZoomMode::ScaleToWidth => "Width",
            ZoomMode::ScaleToHeight => "Height",
            ZoomMode::ScaleToFit => "Fit",
            ZoomMode::ScaleToFill => "Fill",
        }
    }

    /// Name as written in `rgconfig.json` under the `ZoomMode` key.
    pub fn as_str(self) -> &'static str {
        match self {
            ZoomMode::AutoZoom => "AutoZoom",
            ZoomMode::LockZoom => "LockZoom",
            ZoomMode::ScaleToWidth => "ScaleToWidth",
            ZoomMode::ScaleToHeight => "ScaleToHeight",
            ZoomMode::ScaleToFit => "ScaleToFit",
            ZoomMode::ScaleToFill => "ScaleToFill",
        }
    }

    /// Reads a config value; an unknown name falls back to the C# default `AutoZoom`.
    pub fn parse(text: &str) -> Option<ZoomMode> {
        MODES
            .into_iter()
            .find(|mode| mode.as_str().eq_ignore_ascii_case(text.trim()))
    }

    /// The `1`..`6` key that selects this mode.
    pub fn hotkey(self) -> u16 {
        MODES
            .iter()
            .position(|m| *m == self)
            .map_or(1, |index| b'1' as u16 + index as u16)
    }
}

/// The factor to draw `bitmap` with in `area`, matching `CalculateZoomFactor`.
///
/// `locked` is the factor kept by `LockZoom`, and what stays in use when a size is unknown.
/// The C# version multiplies by the DPI scale so that 100% means device-independent pixels;
/// this app is system-DPI-aware and draws in physical pixels, so 1.0 is the file's own size.
pub fn factor(mode: ZoomMode, bitmap: (u32, u32), area: (u32, u32), locked: f64) -> f64 {
    let (bw, bh) = (bitmap.0 as f64, bitmap.1 as f64);
    if bw == 0.0 || bh == 0.0 || area.0 == 0 || area.1 == 0 {
        return locked.max(1.0);
    }

    let width = area.0 as f64 / bw;
    let height = area.1 as f64 / bh;

    match mode {
        ZoomMode::AutoZoom => crate::render::fit_factor(bitmap, area).min(1.0),
        ZoomMode::LockZoom => locked,
        ZoomMode::ScaleToWidth => width,
        ZoomMode::ScaleToHeight => height,
        ZoomMode::ScaleToFit => crate::render::fit_factor(bitmap, area),
        ZoomMode::ScaleToFill => width.max(height),
    }
}

/// How much one wheel notch or one zoom key press multiplies the zoom by, from the `ZoomSpeed`
/// setting (`0` = auto), mirroring the increment of `ImageViewZoomAndPan.cs`: the smaller that
/// setting, the smaller each step.
pub fn zoom_step(zoom_speed: f64, delta: i32) -> f64 {
    let speed = zoom_speed.clamp(0.0, 499.0);
    let step = (delta.abs() as f64 / (500.0 - speed + 1.0)).clamp(0.01, 1.0);
    1.0 + step
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_zoom_shows_the_original_size_and_only_shrinks_when_needed() {
        assert_eq!(factor(ZoomMode::AutoZoom, (100, 100), (400, 400), 3.0), 1.0);
        // Bigger than the area: the largest factor that still fits.
        assert_eq!(factor(ZoomMode::AutoZoom, (400, 200), (200, 200), 3.0), 0.5);
    }

    #[test]
    fn each_mode_uses_its_own_axis() {
        let (bitmap, area) = ((400, 200), (200, 200));
        assert_eq!(factor(ZoomMode::ScaleToWidth, bitmap, area, 3.0), 0.5);
        assert_eq!(factor(ZoomMode::ScaleToHeight, bitmap, area, 3.0), 1.0);
        assert_eq!(factor(ZoomMode::ScaleToFit, bitmap, area, 3.0), 0.5);
        // Filling crops: the larger factor is used and the rest is clipped by the window.
        assert_eq!(factor(ZoomMode::ScaleToFill, bitmap, area, 3.0), 1.0);
        assert_eq!(factor(ZoomMode::LockZoom, bitmap, area, 2.5), 2.5);
    }

    #[test]
    fn an_unknown_size_or_area_keeps_the_current_factor() {
        assert_eq!(factor(ZoomMode::ScaleToFit, (0, 0), (100, 100), 4.0), 4.0);
        assert_eq!(factor(ZoomMode::ScaleToFit, (100, 100), (0, 0), 4.0), 4.0);
        assert_eq!(factor(ZoomMode::LockZoom, (0, 0), (0, 0), 0.0), 1.0);
    }

    #[test]
    fn a_bigger_zoom_speed_zooms_more_at_once() {
        assert!(zoom_step(0.0, 120) < zoom_step(400.0, 120));
        assert_eq!(
            zoom_step(0.0, -120),
            zoom_step(0.0, 120),
            "only the size of the step matters"
        );
        assert!(zoom_step(0.0, 0) > 1.0, "a step is never zero");
        assert!(zoom_step(-500.0, 120) <= zoom_step(500.0, 120));
    }

    #[test]
    fn config_names_and_hotkeys_match_the_csharp_names() {
        assert_eq!(ZoomMode::parse("scaleToFill"), Some(ZoomMode::ScaleToFill));
        assert_eq!(ZoomMode::parse("  LockZoom "), Some(ZoomMode::LockZoom));
        assert_eq!(ZoomMode::parse("nonsense"), None);
        assert_eq!(
            ZoomMode::parse("").unwrap_or(ZoomMode::AutoZoom),
            ZoomMode::AutoZoom
        );
        let keys: Vec<u16> = [b'1', b'2', b'3', b'4', b'5', b'6']
            .iter()
            .map(|key| *key as u16)
            .collect();
        assert_eq!(MODES.iter().map(|m| m.hotkey()).collect::<Vec<_>>(), keys);
        assert_eq!(ZoomMode::ScaleToHeight.as_str(), "ScaleToHeight");
        assert_eq!(ZoomMode::LockZoom.label(), "Lock");
    }
}
