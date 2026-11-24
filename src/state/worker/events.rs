use super::{FileStat, PathBuf, PreparedImport, Source};
use crate::decode::{AnimFrame, Decoded};

pub enum FolderSeed {
    Discovered(usize),
    InsertAt(usize),
}

/// What opening a selection produced.
pub enum OpenedOutcome {
    Still {
        image: Decoded,
        /// Whether the decode lane rechecked the file after the decode and it still
        /// matches the stat sampled before it: only then is the still stored.
        eligible: bool,
    },
    /// The file still matches the stat of the candidate the UI sent, so the UI reads
    /// its cached pixels back; a missing entry reopens through the worker.
    Cached,
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
    ImportRows {
        tag: u64,
        files: Vec<PathBuf>,
    },
    ImportPreview {
        tag: u64,
        replacing: bool,
        files: Vec<PathBuf>,
    },
    FolderRows {
        tag: u64,
        replacing: bool,
        rows: Vec<(PathBuf, crate::photo::FileFacts)>,
    },
    FolderFinished {
        tag: u64,
        order: Vec<usize>,
        seed: Option<FolderSeed>,
        failure: Option<(crate::photo::FolderScanError, Source, bool)>,
    },
    Deduplicated {
        replace_tag: u64,
        revision: u64,
        rows: Vec<(usize, usize)>,
    },
    RemoteImport {
        tag: u64,
        respond: std::sync::mpsc::SyncSender<bool>,
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
    /// Image details for a batch of playlist rows, tagged with the rows token it was read
    /// for; a newer row set makes the cache the only part worth keeping.
    Metadata {
        revision: u64,
        entries: Vec<(PathBuf, crate::photo::Details)>,
        complete: bool,
    },
}
