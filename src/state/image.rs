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

    /// The zoom mode the drawing actually uses: a manual zoom overrides it.
    pub fn active_zoom_mode(&self) -> ZoomMode {
        if self.scale > 0.0 {
            ZoomMode::LockZoom
        } else {
            self.zoom_mode
        }
    }

    /// Picks a zoom mode, clearing a manual zoom. `LockZoom` keeps what is on screen now.
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
        self.animation = None;
        self.animation_played_once = false;
        self.slideshow_elapsed = false;
        self.rotation_turns = 0;
        self.drag = None;
        self.message = None;

        self.bitmap = None;
        self.file_size = self
            .photos
            .current()
            .and_then(|path| path.metadata().ok())
            .map(|metadata| metadata.len());
        if self.zoom_mode != ZoomMode::LockZoom {
            self.clear_manual_zoom();
        }
        // An unreadable file stays in the playlist (removing it would silently drop it from a
        // saved playlist too); the load stops at it and the notice carries the decode error.
        let mut failure: Option<(PathBuf, String)> = failed_frame.map(|p| (p, String::new()));

        while let Some(path) = self.photos.current().cloned() {
            // One decode per file: a cache hit needs none, a still is stored from its only
            // decode, and an animation streams its frames from the same single open.
            if let Some(bitmap) = crate::cache::cached(&path) {
                self.bitmap = Some(bitmap);
                break;
            }
            match decode::open(&path) {
                Ok(decode::Opened::Still(decoded)) => {
                    self.bitmap = Some(crate::cache::store(&path, decoded));
                }
                Ok(decode::Opened::Animated(animation, first)) => {
                    self.animation = Some(animation);
                    self.show_frame(first);
                }
                Err(e) => failure = Some((path, e)),
            }
            break;
        }

        if let Some((path, error)) = failure {
            let name = path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy();
            let mut notice = format!("{name} is unreadable.");
            if !error.is_empty() {
                notice.push_str(&format!(" {error}"));
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
        let Some(animation) = &mut self.animation else {
            return;
        };
        match animation.next_frame() {
            Ok(Some(frame)) => self.show_frame(frame),
            Ok(None) => {
                self.animation_played_once = true;
                if !self.slideshow_elapsed {
                    self.rewind_animation();
                }
            }
            Err(_) => self.load_skipping(self.photos.current().cloned()),
        }
    }

    pub(super) fn rewind_animation(&mut self) {
        let Some(animation) = &mut self.animation else {
            return;
        };
        match animation.rewind().and_then(|_| animation.next_frame()) {
            Ok(Some(frame)) => self.show_frame(frame),
            _ => self.load_skipping(self.photos.current().cloned()),
        }
    }

    fn show_frame(&mut self, frame: decode::AnimFrame) {
        let mut bitmap = Bitmap::from_decoded(frame.image);
        for _ in 0..self.rotation_turns {
            bitmap.rotate_90_clockwise();
        }
        self.bitmap = Some(bitmap);
        self.frame_ms = frame.delay_ms;
        self.frame_revision = self.frame_revision.wrapping_add(1);
    }

    pub fn frame_delay_ms(&self) -> u32 {
        if self.navigation_pending
            || self.animation.is_none()
            || (self.slideshow_paused
                && self.knobs.play_gif_once
                && !self.slideshow_paused_by_space)
        {
            return 0;
        }
        self.frame_ms
    }

    /// Applies a rotation to the still image and to every decoded animation frame.
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
