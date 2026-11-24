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
        // Zooming out can leave the old pan outside the smaller image's limits.
        let image = self.frame().map(|bitmap| (bitmap.width, bitmap.height));
        if let Some(image) = image {
            self.offset =
                crate::render::clamped_offset(image, area, self.factor(area), self.offset);
        }
    }

    /// Multiplies the zoom by `by` around `anchor`, a client point: the image point under it
    /// stays where it is, as far as the pan limits of the new factor allow.
    pub fn zoom_by_anchored(&mut self, area: (u32, u32), by: f64, anchor: (i32, i32)) {
        let image = self.frame().map(|bitmap| (bitmap.width, bitmap.height));
        let from = self.factor(area);
        let to = (from * by).max(0.01);
        self.set_zoom_factor(to);
        if let Some(image) = image {
            self.offset = crate::render::zoom_offset(image, area, from, to, self.offset, anchor);
        }
    }

    pub fn finish_navigation(&mut self) {
        if self.navigation_pending {
            self.load_current();
        }
    }

    /// Decode only the image entering view.
    pub fn load_current(&mut self) {
        self.load_file();
    }

    /// The "X is unreadable." notice, shared by the sync and drained-failure paths.
    pub(super) fn report_unreadable(&mut self, path: &Path, error: &str) {
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

    /// A terminal frame or rewind failure: the stream is gone and the file is not reopened
    /// by itself. The last good frame stays on screen with one notice, the cursor and the
    /// pending flag clear, and a slideshow may still advance at its deadline.
    pub(super) fn stop_animation(&mut self) {
        self.animation = None;
        self.animated_selection = false;
        self.frame_pending = false;
        self.animation_played_once = true;
        let name = self
            .photos
            .current()
            .map(|path| {
                path.file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy()
                    .into_owned()
            })
            .unwrap_or_else(|| String::from("The animation"));
        self.notice = Some(format!("{name} stopped playing. Showing the last frame."));
    }

    /// An unreadable file stays in the playlist (removing it would silently drop it from a
    /// saved playlist too); the load stops at it and the notice carries the decode error.
    pub(super) fn load_file(&mut self) {
        self.slide_revision = self.slide_revision.wrapping_add(1);
        self.navigation_pending = false;
        self.selection = 0;
        self.load_pending = false;
        self.file_size = None;
        self.animation = None;
        self.animated_selection = false;
        self.animation_played_once = false;
        self.frame_pending = false;
        self.slideshow_elapsed = false;
        self.rotation_turns = 0;
        self.drag = None;
        self.message = None;

        self.bitmap = None;
        if self.zoom_mode != ZoomMode::LockZoom {
            self.clear_manual_zoom();
        }

        // No stat here: the cache candidate the lane receives is looked up purely
        // in memory, and the lane stats the live file itself.
        if let Some(worker) = self.worker.as_ref() {
            let Some(path) = self.photos.current().cloned() else {
                self.message = Some(
                    "Drop files or a folder here. Press O to open files or F to open a folder."
                        .into(),
                );
                return;
            };
            match worker.open(path.clone(), crate::cache::candidate(&path)) {
                Ok(generation) => {
                    self.selection = generation;
                    self.load_pending = true;
                }
                Err(_) => self.report_unreadable(&path, "The decoder is unavailable."),
            }
            return;
        }
        let mut failure: Option<(PathBuf, String)> = None;

        if let Some(path) = self.photos.current().cloned() {
            if let Some((stat, bitmap)) = crate::cache::cached_with_stat(&path) {
                self.file_size = Some(stat.0);
                self.bitmap = Some(bitmap);
            } else {
                // One stat before the decode: the file size to report, and the identity
                // the just-decoded pixels are stored under only if the file still matches.
                let before = crate::cache::stat(&path);
                self.file_size = before.map(|(len, _)| len);
                match decode::open(&path) {
                    Ok(decode::Opened::Still(decoded)) => {
                        self.bitmap = Some(crate::cache::store_checked(&path, before, decoded));
                    }
                    Ok(decode::Opened::Animated(animation, first)) => {
                        self.animation = Some(animation);
                        self.show_frame(first);
                    }
                    Err(e) => failure = Some((path, e)),
                }
            }
        }

        if let Some((path, error)) = failure {
            self.report_unreadable(&path, &error);
        }
        if self.bitmap.is_none() {
            self.message = Some(
                "Drop files or a folder here. Press O to open files or F to open a folder.".into(),
            );
        }
    }

    /// Called after the current frame has had its full display delay.
    pub fn advance_frame(&mut self) {
        if let Some(worker) = self.worker.as_ref() {
            // A slow frame holds the picture; the frame timer re-arms until the reply lands.
            if self.frame_pending || !self.animated_selection {
                return;
            }
            let result = worker.next_frame(self.selection);
            self.track_frame_request(result);
            return;
        }
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
            Err(_) => self.stop_animation(),
        }
    }

    pub(super) fn rewind_animation(&mut self) {
        if let Some(worker) = self.worker.as_ref() {
            if self.frame_pending || !self.animated_selection {
                return;
            }
            let result = worker.rewind(self.selection);
            self.track_frame_request(result);
            return;
        }
        let Some(animation) = &mut self.animation else {
            return;
        };
        match animation.rewind().and_then(|_| animation.next_frame()) {
            Ok(Some(frame)) => self.show_frame(frame),
            _ => self.stop_animation(),
        }
    }

    fn track_frame_request(&mut self, result: Result<(), worker::Rejected>) {
        match result {
            Ok(()) => self.frame_pending = true,
            Err(worker::Rejected::Busy) => {}
            Err(worker::Rejected::Disconnected) => {
                self.animated_selection = false;
                if let Some(path) = self.photos.current().cloned() {
                    self.report_unreadable(&path, "The decoder is unavailable.");
                }
            }
        }
    }

    pub(super) fn show_frame(&mut self, frame: decode::AnimFrame) {
        if let Some(bitmap) = self.bitmap.as_mut().filter(|_| self.rotation_turns == 0) {
            bitmap.replace_decoded(frame.image);
        } else {
            let mut bitmap = Bitmap::from_decoded(frame.image);
            for _ in 0..self.rotation_turns {
                bitmap.rotate_90_clockwise();
            }
            self.bitmap = Some(bitmap);
        }
        self.frame_ms = frame.delay_ms;
        self.frame_revision = self.frame_revision.wrapping_add(1);
    }

    pub fn frame_delay_ms(&self) -> u32 {
        if self.navigation_pending
            || self.load_pending
            || self.frame_pending
            || !self.is_animated()
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
