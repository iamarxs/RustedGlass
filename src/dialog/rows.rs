use crate::config::{ALWAYS_MAXIMIZED, FILES_MAXIMIZED};

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
    pub(super) tooltip: &'static str,
    pub(super) kind: Kind,
    /// What to show when the key is not in the settings file; matches `config.rs`.
    pub(super) default: &'static str,
}

impl Row {
    pub(super) fn indent(&self) -> i32 {
        if self.key == FILES_MAXIMIZED {
            super::MARGIN
        } else {
            0
        }
    }
}

/// The settings the viewer reads while it runs, in the order the panel shows them.
pub(super) const ROWS: &[Row] = &[
    Row {
        key: "EnableToolBar",
        label: "Show toolbar",
        tooltip: "Show the toolbar at the top of the viewer window",
        kind: Kind::CheckBox,
        default: "true",
    },
    Row {
        key: ALWAYS_MAXIMIZED,
        label: "Always open maximized",
        tooltip: "Start maximized on every launch. Closing the window while maximized will not overwrite its saved unmaximized size and screen position",
        kind: Kind::CheckBox,
        default: "false",
    },
    Row {
        key: FILES_MAXIMIZED,
        label: "Only when opening files on launch",
        tooltip: "Start maximized when an image or playlist is passed at launch. Closing the window while maximized will not overwrite its saved unmaximized size and screen position",
        kind: Kind::CheckBox,
        default: "false",
    },
    Row {
        key: "EnableFreePan",
        label: "Pan image by dragging",
        tooltip: "Click and drag with the pointer to pan around the image",
        kind: Kind::CheckBox,
        default: "false",
    },
    Row {
        key: "EnableHiddenImagesLoading",
        label: "Show hidden images",
        tooltip: "Include files marked hidden in the operating system",
        kind: Kind::CheckBox,
        default: "false",
    },
    Row {
        key: "EnableSubfoldersLoading",
        label: "Include subfolders",
        tooltip: "Also load images from subfolders of the opened folder",
        kind: Kind::CheckBox,
        default: "false",
    },
    Row {
        key: "EnableAutoSwitchSiblingDir",
        label: "Browse sibling folders",
        tooltip: "When sequential browsing reaches either end of the list, load images from a neighboring folder. Turn off Loop image navigation to use this setting",
        kind: Kind::CheckBox,
        default: "false",
    },
    Row {
        key: "EnableLoopBackNavigation",
        label: "Loop image navigation",
        tooltip: "Wrap back to the other end of the image list in both directions during sequential navigation",
        kind: Kind::CheckBox,
        default: "true",
    },
    Row {
        key: "EnableRandomDisplayOrder",
        label: "Random image order",
        tooltip: "Randomize the order in which images are shown during browsing or a slideshow. Playlist rows stay in place",
        kind: Kind::CheckBox,
        default: "false",
    },
    Row {
        key: "EnableLoopSlideshow",
        label: "Loop slideshow",
        tooltip: "Restart the slideshow after the final image",
        kind: Kind::CheckBox,
        default: "true",
    },
    Row {
        key: "SlideshowPlayGifOnce",
        label: "Finish one animation cycle per slide",
        tooltip: "Wait for animated image formats to complete at least one full cycle before advancing to the next slide. Applies to GIF, APNG, and animated WebP",
        kind: Kind::CheckBox,
        default: "true",
    },
    Row {
        key: "MoveToFolder",
        label: "Quick-move destination folder",
        tooltip: "Press M to move the current image to this folder",
        kind: Kind::PathField,
        default: "",
    },
    Row {
        key: "SlideshowInterval",
        label: "Minimum slide duration (seconds)",
        tooltip: "Shortest time between slides. The slideshow chooses a delay between the minimum and maximum for each slide",
        kind: Kind::TextField,
        default: "5",
    },
    Row {
        key: "SlideshowIntervalTo",
        label: "Maximum slide duration (seconds)",
        tooltip: "Longest delay between slides. Set both durations to the same value for a fixed delay. A maximum below the minimum is raised to match",
        kind: Kind::TextField,
        default: "5",
    },
    Row {
        key: "ZoomSpeed",
        label: "Zoom step (0 = normal)",
        tooltip: "0 is the standard zoom step. Higher values zoom more, and negative values zoom less. Values are limited to −499 through 499",
        kind: Kind::TextField,
        default: "0",
    },
    Row {
        key: "PanSpeed",
        label: "Pan speed",
        tooltip: "How fast the image moves when panning. Enter a value from 1 to 1000. The default is 20",
        kind: Kind::TextField,
        default: "20",
    },
];
