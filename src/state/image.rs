use super::*;

impl State {
    pub fn frame(&self) -> Option<&Bitmap> {
        self.bitmap.as_ref()
    }

    /// The zoom factor to draw with: the zoom the user chose, else the one the zoom mode gives.
    pub fn factor(&self, area: (u32, u32)) -> f64 {
        if self.scale > 0.0 {
            return self.scale;
        }
        match self.frame() {
            Some(bitmap) => zoom::factor(self.zoom_mode, (bitmap.width, bitmap.height), area, 1.0),
            None => 1.0,
        }
    }

    /// The zoom mode the drawing actually uses: a manual zoom overrides it, as `IsManualZoom`
    /// does in the C# view.
    pub fn active_zoom_mode(&self) -> ZoomMode {
        if self.scale > 0.0 {
            ZoomMode::LockZoom
        } else {
            self.zoom_mode
        }
    }

    /// Picks a zoom mode, clearing a manual zoom. `LockZoom` keeps what is on screen now, which is
    /// how "Lock zoom ratio" behaves in ImageGlass.
    pub fn select_zoom_mode(&mut self, mode: ZoomMode, area: (u32, u32)) {
        if mode == ZoomMode::LockZoom {
            self.scale = self.factor(area);
        } else {
            self.scale = 0.0;
        }
        self.zoom_mode = mode;
        self.config
            .set("ZoomMode", Value::String(mode.as_str().to_string()));
        self.offset = (0, 0);
    }

    /// Sets a zoom the user chose, which overrides the zoom mode.
    pub fn set_zoom_factor(&mut self, factor: f64) {
        self.scale = factor.max(0.01);
    }

    /// Gives the zoom back to the zoom mode.
    pub fn clear_manual_zoom(&mut self) {
        self.scale = 0.0;
        self.offset = (0, 0);
    }

    /// Multiplies the zoom by `by`. A zoom the mode gives becomes a manual zoom first, so the
    /// wheel and the `+`/`-` keys zoom the image that is on screen.
    pub fn zoom_by(&mut self, area: (u32, u32), by: f64) {
        let base = if self.scale > 0.0 {
            self.scale
        } else {
            self.factor(area)
        };
        self.set_zoom_factor(base * by);
    }

    pub fn finish_navigation(&mut self) {
        if self.navigation_pending {
            self.load_current();
        }
    }

    /// Decode only the image entering view, skipping unreadable entries.
    pub fn load_current(&mut self) {
        self.load_skipping(None);
    }

    fn load_skipping(&mut self, failed_frame: Option<PathBuf>) {
        self.slide_revision = self.slide_revision.wrapping_add(1);
        self.navigation_pending = false;
        self.gif = None;
        self.gif_played_once = false;
        self.slideshow_elapsed = false;
        self.rotation_turns = 0;
        self.drag = None;
        self.message = None;

        self.bitmap = None;
        self.clear_manual_zoom();
        let mut first_failure = failed_frame;
        let mut failures = 0;
        if first_failure.is_some() {
            self.photos.remove_current();
            failures += 1;
        }

        while let Some(path) = self.photos.current().cloned() {
            if let Some((gif, first)) = decode::is_animated(&path)
                .then(|| decode::Gif::open(&path).ok())
                .flatten()
            {
                self.gif = Some(gif);
                self.show_gif_frame(first);
                break;
            }
            match crate::cache::load(&path) {
                Ok(bitmap) => {
                    self.bitmap = Some(bitmap);
                    break;
                }
                Err(_) => {
                    self.photos.remove_current();
                    first_failure.get_or_insert(path);
                    failures += 1;
                }
            }
        }

        if let Some(path) = first_failure {
            let name = path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy();
            let mut notice = format!("Removed from the playlist — {name} is unreadable.");
            if failures > 1 {
                notice.push_str(&format!(
                    " {} more unreadable entries removed from the playlist.",
                    failures - 1
                ));
            }
            self.notice = Some(notice);
            self.unreadable_notice_pending = true;
        }
        if self.bitmap.is_none() {
            self.message = Some(
                "Drop files or a folder here. Press O to open files or F to open a folder.".into(),
            );
        }
    }

    /// Called after the current frame has had its full display delay.
    pub fn advance_frame(&mut self) {
        let Some(gif) = &mut self.gif else { return };
        match gif.next_frame() {
            Ok(Some(frame)) => self.show_gif_frame(frame),
            Ok(None) => {
                self.gif_played_once = true;
                if !self.slideshow_elapsed {
                    self.rewind_gif();
                }
            }
            Err(_) => self.load_skipping(self.photos.current().cloned()),
        }
    }

    pub(super) fn rewind_gif(&mut self) {
        let Some(gif) = &mut self.gif else { return };
        match gif.rewind().and_then(|_| gif.next_frame()) {
            Ok(Some(frame)) => self.show_gif_frame(frame),
            _ => self.load_skipping(self.photos.current().cloned()),
        }
    }

    fn show_gif_frame(&mut self, frame: decode::AnimFrame) {
        let mut bitmap = Bitmap::from_decoded(frame.image);
        for _ in 0..self.rotation_turns {
            bitmap.rotate_90_clockwise();
        }
        self.bitmap = Some(bitmap);
        self.gif_frame_ms = frame.delay_ms;
        self.frame_revision = self.frame_revision.wrapping_add(1);
    }

    pub fn frame_delay_ms(&self) -> u32 {
        if self.navigation_pending
            || self.gif.is_none()
            || (self.slideshow_paused && self.knobs.play_gif_once)
        {
            return 0;
        }
        self.gif_frame_ms
    }

    /// Applies a rotation to the still image and to every decoded GIF frame.
    pub fn rotate_frames(&mut self, degrees: i32) {
        let turns = ((degrees.rem_euclid(360)) / 90) as usize;
        self.rotation_turns = (self.rotation_turns + turns) % 4;
        for _ in 0..turns {
            if let Some(bitmap) = self.bitmap.as_mut() {
                bitmap.rotate_90_clockwise();
            }
        }
        self.offset = (0, 0);
    }
}
