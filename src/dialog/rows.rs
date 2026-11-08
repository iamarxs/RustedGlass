#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    CheckBox,
    /// A number, read as `f64`.
    TextField,
    /// A folder path, with a button for the folder picker.
    PathField,
}

pub(super) struct Row {
    pub(super) key: &'static str,
    pub(super) label: &'static str,
    pub(super) kind: Kind,
    /// What to show when the key is not in the settings file; matches `config.rs`.
    pub(super) default: &'static str,
}

/// The settings the viewer reads while it runs, in the order the panel shows them.
pub(super) const ROWS: &[Row] = &[
    Row {
        key: "EnableToolBar",
        label: "Show the tool bar",
        kind: Kind::CheckBox,
        default: "true",
    },
    Row {
        key: "EnableFreePan",
        label: "Drag the image to move it",
        kind: Kind::CheckBox,
        default: "false",
    },
    Row {
        key: "EnableHiddenImagesLoading",
        label: "Show images hidden by Windows",
        kind: Kind::CheckBox,
        default: "false",
    },
    Row {
        key: "EnableSubfoldersLoading",
        label: "Include images in subfolders",
        kind: Kind::CheckBox,
        default: "false",
    },
    Row {
        key: "EnableAutoSwitchSiblingDir",
        label: "Move to the next folder at its last image",
        kind: Kind::CheckBox,
        default: "false",
    },
    Row {
        key: "EnableLoopBackNavigation",
        label: "Loop back to the first image at the end",
        kind: Kind::CheckBox,
        default: "true",
    },
    Row {
        key: "EnableRandomDisplayOrder",
        label: "Show images in random order",
        kind: Kind::CheckBox,
        default: "false",
    },
    Row {
        key: "EnableLoopSlideshow",
        label: "Repeat the slideshow",
        kind: Kind::CheckBox,
        default: "true",
    },
    Row {
        key: "SlideshowPlayGifOnce",
        label: "During slideshow, play a GIF file at least once",
        kind: Kind::CheckBox,
        default: "true",
    },
    Row {
        key: "MoveToFolder",
        label: "Move here with M",
        kind: Kind::PathField,
        default: "",
    },
    Row {
        key: "SlideshowInterval",
        label: "Show each image for (seconds)",
        kind: Kind::TextField,
        default: "5",
    },
    Row {
        key: "SlideshowIntervalTo",
        label: "… or up to this many seconds",
        kind: Kind::TextField,
        default: "5",
    },
    Row {
        key: "ZoomSpeed",
        label: "Zoom speed (0 = normal, higher = faster)",
        kind: Kind::TextField,
        default: "0",
    },
    Row {
        key: "PanSpeed",
        label: "Pan speed",
        kind: Kind::TextField,
        default: "20",
    },
];
