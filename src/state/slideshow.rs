use super::*;

impl State {
    /// Turns a looped slideshow into a repeating one: shuffle the order, and shuffle it again when
    /// the last row is reached.
    /// Runs from the image on screen, in the order the list has. `Shuffle` and `Repeat` are the
    /// user's own settings now, so starting never changes them.
    pub fn start_slideshow(&mut self) {
        self.slide_revision = self.slide_revision.wrapping_add(1);
        self.slideshow_elapsed = false;
        self.slideshow_paused = self.playlist_visible;
        self.slideshow_running = !self.playlist_visible;
        let (from, to) = self.knobs.slideshow_bounds();
        let interval = if to > from {
            format!("{from}–{to}")
        } else {
            from.to_string()
        };
        self.notice = Some(format!("Slideshow started, {interval} s per image"));
        if self.knobs.play_gif_once && !self.navigation_pending && self.gif.is_some() {
            self.gif_played_once = false;
            self.rewind_gif();
        }
    }

    pub fn stop_slideshow(&mut self) {
        self.slideshow_elapsed = false;
        self.slideshow_paused = false;
        self.slideshow_running = false;
        self.notice = Some(String::from("Slideshow stopped"));
    }

    /// Holds a running slideshow while the playlist is open, so the rows do not move under the
    /// pointer, and starts it again when the list goes away.
    pub fn pause_slideshow(&mut self) {
        if self.slideshow_running {
            self.slideshow_elapsed = false;
            self.slideshow_paused = true;
            self.slideshow_running = false;
        }
    }

    pub fn resume_slideshow(&mut self) {
        if self.slideshow_paused {
            self.slideshow_paused = false;
            self.slideshow_running = true;
        }
    }

    pub fn advance_slideshow_if_ready(&mut self) -> bool {
        if !self.slideshow_running
            || self.navigation_pending
            || !self.slideshow_elapsed
            || (self.knobs.play_gif_once && self.gif.is_some() && !self.gif_played_once)
        {
            return false;
        }
        self.slideshow_elapsed = false;
        if self.step(1) {
            self.load_current();
        } else if self.gif_played_once {
            self.rewind_gif();
        }
        true
    }

    /// Moves to the next or previous file. A slideshow that reaches the last image starts again if
    /// `Repeat` is on — in a fresh shuffled order when `Shuffle` is on too — and otherwise ends.
    ///
    /// Every image of a shuffled pass is shown once: the order is only made again at the end of a
    /// pass, never in the middle of one.
    ///
    /// The picture is not loaded here: the caller says when the last move of a fast run of wheel
    /// clicks is worth the decode.
    pub fn step(&mut self, delta: i32) -> bool {
        if self.photos.is_empty() {
            self.message = Some(String::from("No images in this folder"));
            return false;
        }

        if self.slideshow_running && delta > 0 && self.photos.is_at_last() {
            if !self.knobs.loop_slideshow {
                self.slideshow_running = false;
                return false;
            }
            if self.knobs.random_order {
                self.photos.shuffle(random_seed());
            }
            self.photos.index = 0;
        } else if !self.photos.step(
            delta,
            self.knobs.loop_back,
            self.knobs.auto_sibling_dir,
            self.knobs.random_order.then(random_seed),
            (self.knobs.include_hidden, self.knobs.recurse),
        ) {
            return false;
        }
        true
    }

    /// Turns the random display order on or off without touching the slideshow.
    pub fn toggle_random_order(&mut self) {
        self.set_random_order(!self.knobs.random_order);
    }

    /// Pick the next slide's duration in whole seconds, including both bounds.
    pub fn slideshow_delay_ms(&self) -> u32 {
        let (from, to) = self.knobs.slideshow_bounds();
        let seconds = if to > from {
            from + (random_seed() % (u64::from(to - from) + 1)) as u32
        } else {
            from
        };
        seconds.saturating_mul(1000)
    }
}

impl Knobs {
    pub fn slideshow_bounds(&self) -> (u32, u32) {
        let from = self.slideshow_seconds.max(1);
        (from, self.slideshow_seconds_to.max(from))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_slideshow_can_wait_for_every_frame_of_a_long_gif() {
        let path = crate::test_path("slideshow-long-gif-test.gif");
        let mut encoder =
            ::image::codecs::gif::GifEncoder::new(std::fs::File::create(&path).unwrap());
        for n in 0..105 {
            let frame = ::image::RgbaImage::from_pixel(2, 2, ::image::Rgba([n, 0, 0, 255]));
            encoder
                .encode_frame(::image::Frame::from_parts(
                    frame,
                    0,
                    0,
                    ::image::Delay::from_numer_denom_ms(20, 1),
                ))
                .unwrap();
        }
        drop(encoder);

        let mut state = State::new(crate::test_path("unused-config.json"), &[]);
        let next = path.with_extension("ppm");
        std::fs::write(&next, b"P6\n1 1\n255\n\xff\x00\x00").unwrap();
        state.photos.files = vec![path.clone(), next.clone()];
        state.load_current();
        state.start_slideshow();
        state.slideshow_elapsed = true;
        for _ in 0..104 {
            assert!(!state.advance_slideshow_if_ready());
            assert_eq!(state.frame_delay_ms(), 20);
            state.advance_frame();
        }
        assert!(
            !state.advance_slideshow_if_ready(),
            "hold the final frame for its delay"
        );
        state.advance_frame();
        assert!(state.advance_slideshow_if_ready());
        assert_eq!(state.photos.index, 1);

        state.photos.index = 0;
        state.load_current();
        state.knobs.play_gif_once = false;
        state.slideshow_elapsed = true;
        assert!(
            state.advance_slideshow_if_ready(),
            "setting off uses only the slideshow timer"
        );
        assert_eq!(state.photos.index, 1);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(next).unwrap();
    }
}
