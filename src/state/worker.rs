//! Stage RG06 background lanes: decoding and playlist/folder preparation run off the UI
//! thread, each on its own bounded queue (see [`queue`]). Nothing UI-owned crosses a
//! thread boundary (no `State`, no GDI object), and the `image::Frames` cursor is not
//! `Send`, so it lives on the decode thread. Only paths, raw pixels, delays, file sizes
//! and statuses travel, and every result carries its selection generation or caller tag,
//! so a result of an outdated request can be told apart and discarded. The UI thread owns
//! the `Worker`, enqueues without blocking and drains results through
//! [`Worker::next_event`]; shutting down closes the lanes, and the threads finish on
//! their own — their `JoinHandle`s are simply dropped.
//!
//! Result lanes are split by payload: pixel results (full-size frames) get a lane of a
//! few entries — never a queue of 32 full-size images — while lightweight import events
//! get room to carry a whole dropped batch.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use windows::Win32::Foundation::HWND;

use crate::cache::FileStat;
use crate::decode::{AnimFrame, Decoded};

mod decode_lane;
pub(super) mod prep;
mod queue;
#[cfg(test)]
mod tests;
mod wake;

use prep::PrepRequest;
pub use prep::{PreparedImport, Source};
use queue::Lane;
pub use queue::Rejected;
use wake::Wake;
pub(crate) use wake::BACKGROUND_READY;

/// Room for one queued selection plus one queued frame/rewind operation.
const DECODE_QUEUE: usize = 2;
/// Folder and playlist preparations waiting for their thread.
const PREP_QUEUE: usize = 16;
/// Finished pixel results waiting for the UI: full-size images, so a small constant.
const PIXEL_RESULTS: usize = 4;
/// Finished lightweight import events waiting for the UI.
const NOTE_RESULTS: usize = 16;

/// A request to the decode lane, carrying the selection generation it belongs to, so one
/// a newer selection made obsolete is dropped inside the lane.
enum DecodeRequest {
    Open {
        generation: u64,
        path: PathBuf,
    },
    /// Stream the next frame of that selection's animation.
    NextFrame {
        generation: u64,
    },
    /// Rewind that selection's animation to its first frame.
    Rewind {
        generation: u64,
    },
}

/// What opening a selection produced.
pub enum OpenedOutcome {
    Still {
        image: Decoded,
    },
    /// The first frame of an animation; the rest arrive as `Event::Frame`.
    AnimatedFirst {
        frame: AnimFrame,
    },
    Failed {
        error: String,
    },
}

/// One finished piece of background work, tagged so the caller can match it up and drop
/// results older than what it holds already.
pub enum Event {
    /// The selection of `generation` opened, as pixels to show or a failure to report.
    Opened {
        generation: u64,
        /// The file's length and modified time, sampled before the decode: the file size
        /// to report, and the identity a cache insertion is checked against.
        identity: Option<FileStat>,
        outcome: OpenedOutcome,
    },
    Frame {
        generation: u64,
        frame: AnimFrame,
    },
    /// The animation played once; the UI rewinds or navigates, as it does today.
    FrameEnded {
        generation: u64,
    },
    /// A frame could not be read; the UI keeps the last good frame and stops the animation.
    FrameFailed {
        generation: u64,
    },
    Appended {
        tag: u64,
        rows: PreparedImport,
    },
    Replaced {
        tag: u64,
        rows: PreparedImport,
    },
    PrepFailed {
        tag: u64,
        error: String,
    },
    Inspected {
        revision: u64,
        path: PathBuf,
        text: String,
    },
}

/// The two background lanes. The UI thread owns the `Worker` and is the only caller of
/// its methods.
pub struct Worker {
    /// The generation of the selection [`Worker::open`] hands out next.
    generation: AtomicU64,
    decode: Arc<Lane<DecodeRequest>>,
    prep: Arc<Lane<PrepRequest>>,
    pixels: Arc<Lane<Event>>,
    notes: Arc<Lane<Event>>,
    wake: Arc<Wake>,
}

impl Worker {
    /// Starts the decode and preparation threads; `formats` is what
    /// `photo::image_formats()` returns, or the photos' own list.
    pub fn new(formats: &[String]) -> Worker {
        let decode = Arc::new(Lane::new(DECODE_QUEUE));
        let prep = Arc::new(Lane::new(PREP_QUEUE));
        let pixels = Arc::new(Lane::new(PIXEL_RESULTS));
        let notes = Arc::new(Lane::new(NOTE_RESULTS));
        let wake = Arc::new(Wake::default());
        // The threads' handles are dropped here: closing the lanes below ends them, and
        // a join would only hold the UI at shutdown.
        {
            let (requests, results) = (Arc::clone(&decode), Arc::clone(&pixels));
            let signal = Arc::clone(&wake);
            if std::thread::Builder::new()
                .name(String::from("rg-decode"))
                .spawn(move || decode_lane::run(&requests, &results, &signal))
                .is_err()
            {
                decode.close();
            }
        }
        {
            let (requests, results) = (Arc::clone(&prep), Arc::clone(&notes));
            let formats = formats.to_vec();
            let signal = Arc::clone(&wake);
            if std::thread::Builder::new()
                .name(String::from("rg-prep"))
                .spawn(move || prep::lane(&requests, &results, &formats, &signal))
                .is_err()
            {
                prep.close();
            }
        }
        Worker {
            generation: AtomicU64::new(1),
            decode,
            prep,
            pixels,
            notes,
            wake,
        }
    }

    pub fn attach_window(&self, hwnd: HWND) {
        self.wake.set_window(hwnd);
    }

    /// Opens `path` as the new selection. The returned generation tags every decode event
    /// of this selection, so the UI keeps the events of the latest one and discards the
    /// rest. A queued open that has not started is replaced, not queued behind — an open
    /// is never refused for a full lane.
    pub fn open(&self, path: PathBuf) -> Result<u64, Rejected> {
        let generation = self.generation.fetch_add(1, Ordering::Relaxed);
        self.decode
            .push_replacing(DecodeRequest::Open { generation, path }, |request| {
                matches!(request, DecodeRequest::Open { .. })
            })?;
        Ok(generation)
    }

    /// Streams the next frame of the animation of `generation`.
    pub fn next_frame(&self, generation: u64) -> Result<(), Rejected> {
        self.decode.push(DecodeRequest::NextFrame { generation })
    }

    /// Rewinds the animation of `generation` to its first frame; the file is opened
    /// again, the way `Animation::rewind` always works.
    pub fn rewind(&self, generation: u64) -> Result<(), Rejected> {
        self.decode.push(DecodeRequest::Rewind { generation })
    }

    /// Prepares `source` into image paths; the result appends to the playlist. `tag`
    /// comes back with the result, so several preparations can be told apart.
    pub fn append(&self, tag: u64, source: Source) -> Result<(), Rejected> {
        self.prep.push(PrepRequest::Append { tag, source })
    }

    /// Prepares `source` into image paths; the result replaces the playlist. A queued
    /// preparation that has not started is dropped first: its files would land in the
    /// list this newer result replaces, so the newest request wins.
    pub fn replace(&self, tag: u64, source: Source) -> Result<(), Rejected> {
        self.prep
            .push_replacing(PrepRequest::Replace { tag, source }, |_| true)
    }

    pub fn inspect(&self, revision: u64, path: PathBuf) -> Result<(), Rejected> {
        self.prep
            .push_replacing(PrepRequest::Inspect { revision, path }, |job| {
                matches!(job, PrepRequest::Inspect { .. })
            })
    }

    /// The next finished piece of work, without waiting; `None` when nothing is pending.
    /// Pixel results are drained first, keeping an animation's frames in order.
    pub fn next_event(&self) -> Option<Event> {
        self.pixels.try_pop().or_else(|| self.notes.try_pop())
    }
}

impl Drop for Worker {
    /// Stops all lanes: pushes are refused, queued jobs are discarded, blocked deliveries
    /// end, the threads finish and are never joined on the UI thread.
    fn drop(&mut self) {
        self.wake.set_window(HWND::default());
        self.decode.close();
        self.prep.close();
        self.pixels.close();
        self.notes.close();
    }
}
