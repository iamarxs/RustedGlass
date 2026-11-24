use super::{photo, PathBuf};

/// A folder or `.rgpl` file to turn into a list of image paths.
#[derive(Clone, Debug)]
pub enum Source {
    Folder {
        path: PathBuf,
        include_hidden: bool,
        recurse: bool,
        seed: Option<PathBuf>,
    },
    /// A retry of a batch already identified as folders, even when a root is inaccessible.
    Folders {
        paths: Vec<PathBuf>,
        include_hidden: bool,
        recurse: bool,
    },
    /// Entries come back as listed, missing ones included, the way `rgpl::read_file`
    /// always has; the caller reports them.
    PlaylistFile { path: PathBuf },
    /// A dropped batch: folders expand, playlist files load their rows, other paths stay.
    Batch {
        paths: Vec<PathBuf>,
        include_hidden: bool,
        recurse: bool,
    },
}

/// A request to the preparation lane.
pub(in crate::state::worker) enum PrepRequest {
    /// The prepared files append to the playlist.
    Append {
        tag: u64,
        source: Source,
    },
    /// The prepared files replace the whole playlist.
    Replace {
        tag: u64,
        source: Source,
    },
    Inspect {
        revision: u64,
        path: PathBuf,
    },
    Deduplicate {
        replace_tag: u64,
        revision: u64,
        paths: Vec<PathBuf>,
    },
    /// Image details for a batch of playlist rows, tagged with the rows token.
    Metadata {
        revision: u64,
        paths: Vec<(PathBuf, Option<photo::FileFacts>)>,
        read_header: bool,
        background_header: bool,
    },
}

/// Prepared rows and how many candidates were dropped, counted the way the synchronous
/// methods have always counted them.
#[derive(Default)]
pub struct PreparedImport {
    pub files: Vec<PathBuf>,
    pub skipped: usize,
    pub missing: usize,
    pub remote_skipped: usize,
    /// The first playlist read failure, kept when other rows of a mixed drop still loaded.
    pub read_error: Option<String>,
}
