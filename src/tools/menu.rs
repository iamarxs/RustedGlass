use super::*;

/// `row_commands` is whether the playlist menu was opened on explicit rows, which the commands
/// that act on files need.
pub fn show_menu(hwnd: HWND, row_commands: bool) -> Option<Action> {
    let menu = crate::state::with_state(|state| {
        if state.playlist_visible {
            playlist_menu(state, row_commands)
        } else {
            viewer_menu(state)
        }
    })??;
    menu.track(hwnd)
}

fn viewer_menu(state: &State) -> Option<ContextMenu> {
    use Action::*;
    let mut menu = ContextMenu::new()?;
    let root = menu.root();
    let shown = state.photos.current().is_some();
    let zoomable = state.frame().is_some();
    let slideshow = state.slideshow_running || state.slideshow_paused;

    let nav = menu.group("Navigation")?;
    menu.add(nav, Next, "Next image\t→ / ↓", shown, false);
    menu.add(nav, Previous, "Previous image\t← / ↑", shown, false);
    menu.add(nav, First, "Go to first image\tHome", shown, false);
    menu.add(nav, Last, "Go to last image\tEnd", shown, false);
    menu.line(nav);
    menu.add(
        nav,
        RandomOrder,
        "Random image order\tCtrl+A",
        shown,
        state.knobs.random_order,
    );

    let zoom_menu = menu.group("Zoom")?;
    menu.add(zoom_menu, ZoomIn, "Zoom in\t+", zoomable, false);
    menu.add(zoom_menu, ZoomOut, "Zoom out\t−", zoomable, false);
    menu.add(
        zoom_menu,
        ExactZoom,
        "Enter exact zoom…\tZ",
        zoomable,
        false,
    );
    menu.add(
        zoom_menu,
        FitOriginal,
        "Fit image to window\t0",
        zoomable,
        false,
    );
    menu.line(zoom_menu);
    for (index, mode) in zoom::MODES.iter().enumerate() {
        menu.add(
            zoom_menu,
            ZoomMode(index),
            &format!("{}\t{}", mode.label(), char::from(mode.hotkey() as u8)),
            zoomable,
            *mode == state.active_zoom_mode(),
        );
    }

    let image = menu.group("Image")?;
    menu.add(image, Rotate, "Rotate image 90°\tR", zoomable, false);
    menu.add(image, ImageInfo, "Image information\tI", shown, false);
    menu.add(image, CopyImage, "Copy image to clipboard", zoomable, false);

    let files = menu.group("Files")?;
    menu.add(files, OpenFile, "Open images…\tO", true, false);
    menu.add(files, OpenFolder, "Open folder…\tF", true, false);
    if state.can_retry_folder_scan() {
        menu.add(files, RetryFolderScan, "Retry folder scan", true, false);
    }
    menu.add(files, SaveAs, "Save as…\tC", shown, false);
    menu.add(files, Rename, "Rename image…\tF2", shown, false);
    menu.add(
        files,
        CopyFilePath,
        "Copy file path\tCtrl+Shift+C",
        shown,
        false,
    );
    menu.add(
        files,
        MoveToTarget,
        "Quick-move image\tM",
        shown && !state.knobs.move_to.as_os_str().is_empty(),
        false,
    );
    menu.add(
        files,
        ShowInExplorer,
        "Show image in Explorer\tE",
        shown,
        false,
    );
    menu.add(
        files,
        DeleteFile,
        "Move image to Recycle Bin\tCtrl+Del",
        shown,
        false,
    );

    let playlist = menu.group("Playlist")?;
    menu.add(playlist, ShowPlaylist, "Show playlist\tL", true, false);
    menu.add(
        playlist,
        OpenPlaylist,
        "Open playlist…\tCtrl+O",
        true,
        false,
    );
    menu.add(
        playlist,
        SavePlaylist,
        "Save playlist…\tCtrl+S",
        true,
        false,
    );
    menu.add(playlist, AddFiles, "Add images…\tA", true, false);
    menu.add(playlist, AddFolder, "Add folder…\tShift+A", true, false);
    menu.add(
        playlist,
        RemoveItem,
        "Remove image from playlist\tDel",
        shown,
        false,
    );

    let slides = menu.group("Slideshow")?;
    let (action, label) = if slideshow {
        (StopSlideshow, "Stop slideshow\tF11")
    } else {
        (StartSlideshow, "Start slideshow\tF11")
    };
    menu.add(slides, action, label, shown || slideshow, false);
    menu.add(
        slides,
        ToggleSlideshowPause,
        if state.slideshow_paused {
            "Resume slideshow\tSpace"
        } else {
            "Pause slideshow\tSpace"
        },
        slideshow,
        false,
    );
    menu.add(
        slides,
        ToggleRepeat,
        "Loop slideshow",
        shown,
        state.knobs.loop_slideshow,
    );

    let window = menu.group("Window")?;
    menu.add(
        window,
        ToggleToolBar,
        "Show toolbar\tT",
        true,
        state.toolbar_visible,
    );
    menu.add(
        window,
        ToggleFullscreen,
        "Full screen\tF12",
        true,
        state.is_fullscreen,
    );
    menu.add(
        window,
        ToggleBorderless,
        "Hide window frame\tF10",
        true,
        state.is_borderless && !state.is_fullscreen,
    );

    menu.line(root);
    menu.add(root, Settings, "Settings…\tS", true, false);
    menu.add(root, Shortcuts, "Keyboard shortcuts…\tF1", true, false);
    menu.add(root, About, "About RustedGlass…", true, false);
    menu.add(root, Quit, "Quit\tCtrl+Q", true, false);
    Some(menu)
}

fn playlist_menu(state: &State, row_commands: bool) -> Option<ContextMenu> {
    use Action::*;
    let mut menu = ContextMenu::new()?;
    let root = menu.root();
    let shown = !state.photos.is_empty();
    let rows = shown && row_commands;

    menu.add(root, AddFiles, "Add images…\tA", true, false);
    menu.add(root, AddFolder, "Add folder…\tShift+A", true, false);
    if state.can_retry_folder_scan() {
        menu.add(root, RetryFolderScan, "Retry folder scan", true, false);
    }
    menu.add(root, OpenPlaylist, "Open playlist…\tCtrl+O", true, false);
    menu.add(root, SavePlaylist, "Save playlist…\tCtrl+S", true, false);
    menu.line(root);
    menu.add(
        root,
        RemoveSelected,
        "Remove selected from playlist\tDel",
        rows,
        false,
    );
    menu.add(
        root,
        RemoveDuplicates,
        "Remove duplicates",
        state.photos.len() > 1,
        false,
    );
    menu.add(
        root,
        DeleteSelected,
        "Move selected to Recycle Bin\tCtrl+Del",
        rows,
        false,
    );
    menu.add(
        root,
        ShowFolder,
        "Show selected image in Explorer",
        rows,
        false,
    );
    menu.add(
        root,
        CopyFilePath,
        "Copy file path\tCtrl+Shift+C",
        rows,
        false,
    );
    menu.add(root, ShufflePlaylist, "Shuffle rows", shown, false);
    let columns = menu.group("Columns")?;
    let visible = crate::photo::Column::ALL
        .into_iter()
        .filter(|&column| crate::playlist::column_visible(column))
        .count();
    for column in crate::photo::Column::ALL {
        let checked = crate::playlist::column_visible(column);
        menu.add(
            columns,
            TogglePlaylistColumn(column),
            column.title(),
            !checked || visible > 1,
            checked,
        );
    }
    menu.line(root);
    menu.add(
        root,
        ToggleRepeat,
        "Loop slideshow",
        true,
        state.knobs.loop_slideshow,
    );
    menu.add(root, ShowPlaylist, "Hide playlist\tL", true, false);
    menu.add(root, About, "About RustedGlass…", true, false);
    Some(menu)
}

mod native;
use native::ContextMenu;
