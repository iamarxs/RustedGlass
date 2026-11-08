# RustedGlass

RustedGlass is a fast, portable, Rust-based Windows image viewer that combines on-the-fly decoding with a lean memory footprint.

## Features

- Open individual images, browse folders, or drag and drop files, folders, and playlists. Browse with the keyboard, mouse wheel, or toolbar.
- View JPEG, PNG, WebP, TIFF, BMP, GIF, ICO, QOI, DDS, HDR, TGA, and Netpbm images. Animated GIFs play automatically.
- Zoom, pan, and rotate images. Embedded EXIF orientation is applied automatically.
- Inspect an image's full path, file size, dimensions, detected format, and available EXIF details. Copy the displayed frame to the clipboard at its native resolution.
- Run slideshows with fixed or random intervals, shuffle, repeat, and an option to let animated GIFs play through at least once.
- Build and reorder playlists with drag and drop, sort by name, and add the same image more than once. Save and reopen `.rgpl` playlists with relative paths.
- Rename, copy, move, and recycle files or reveal them in Explorer. Copy and move use numbered filenames when the destination already contains a file with the same name.
- Switch between framed, borderless, and fullscreen views, hide the toolbar, or keep the viewer on top. Window position and size are restored on restart.
- Browse rapidly with a cache for recently decoded still images.
- Keep settings in a portable JSON file beside the executable, select another file with `--config`, or override settings for a single run.
- **F1** for the grouped keyboard shortcut list.

## Open and browse images

- Press **O** to choose one or more images, or **F** to choose a folder. You can also drop files or a folder onto the window.
- Use the **arrow keys**, **Space**, or the **mouse wheel** to browse. **Home** and **End** jump to the first and last image. Rapid navigation loads the final selection after a short pause. Recently decoded images are cached in memory.
- Press **0** to fit the whole image. Use **+**, **−**, or **Ctrl + wheel** to zoom. **1–6** select Auto, Lock, Width, Height, Fit, and Fill. To pan a zoomed image, enable **Drag the image to move it** in Settings, then drag the image.
- Press **R** to rotate the displayed image 90° or **I** to inspect its path, size, dimensions, detected format, and available EXIF details.

## Window and slideshow controls

| Control               | Action                                                                 |
| --------------------- | ---------------------------------------------------------------------- |
| **F10**               | Switch between a framed and borderless window.                         |
| **Drag near the top** | Move the borderless window. Drag the outer edges or corners to resize. |
| **F12**               | Enter or exit fullscreen.                                              |
| **T**                 | Show or hide the toolbar.                                              |
| **F11**               | Start or stop the slideshow.                                           |
| **S** or **Ctrl + P** | Open Settings.                                                         |
| **Esc**               | Close the active panel or playlist, then the image, then the window.   |

Use **Shuffle** or **Ctrl + A** for random browsing order. Toggle **Repeat** to loop the slideshow. In Settings, enter the minimum and maximum seconds per image. Equal values give a fixed duration. Different values give a random whole-second duration within that range per image. **During slideshow, play a GIF file at least once** is on by default. It waits for one complete playback if the GIF runs longer than the selected interval. Turn it off to advance on the slideshow timer alone. Opening the playlist pauses an active slideshow. Closing it resumes playback.

## Manage playlists and files

1. Press **L** or click **List** on the toolbar to toggle the playlist. Click a row to select its image and drag a row to reorder it.
2. Right-click in the playlist to add files or folders, sort by name, shuffle, or open and save a playlist. **A** adds files and **Shift + A** adds a folder.
3. Use **Ctrl + S** to save a `.rgpl` playlist and **Ctrl + O** to open one. You can also drop a `.rgpl` file onto the app or pass its path on the command line.

A `.rgpl` file is UTF-8 text with one image path per line. Lines beginning with `#` are comments. Relative paths resolve from the playlist's folder, so keep the same relative layout when moving a playlist and its images.

**Delete** removes selected entries from the playlist and keeps the files on disk. **Ctrl + Delete** sends the selected files to the Windows Recycle Bin. Use **Show the containing folder in Explorer** in the playlist menu to locate a selected file. In the viewer, **F2** renames the current file, **C** copies it to a folder, **M** moves it to the destination set in Settings, and **E** selects it in Explorer.

## Settings and command-line use

**RustedGlass is portable.** When you click **Save & Close**, preferences are saved in `rgconfig.json` beside `RustedGlass.exe`. Use `--config <path>` to read and save settings in a different file. Relative config paths are resolved from the working directory. File commands such as Copy, Move, Rename, and Recycle Bin act on the files you select.

From PowerShell in the folder containing `RustedGlass.exe`:

```powershell
.\RustedGlass.exe "./Pictures"
.\RustedGlass.exe "./Pictures/Weekend.rgpl" --fullscreen
.\RustedGlass.exe "./Pictures" -p:SlideshowInterval=8
.\RustedGlass.exe --config "./Settings/viewer.json" "./Pictures"
.\RustedGlass.exe --help | Out-Host
```

Use `--maximized` (`-m`), `--fullscreen` (`-f`), or `--topmost` (`-t`) to control the startup window. Use `-p:Key=value` for a setting override for that run.

## Build from source

Install [Rust and Cargo](https://rust-lang.org/tools/install/) with the `x86_64-pc-windows-msvc` toolchain. Install the [Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/) with the **Desktop development with C++** workload and a Windows SDK. Then run the build script in PowerShell from the source folder.

```powershell
.\build.ps1
```

Cargo downloads the pinned dependencies on the first build. The script writes the Windows x64 release executable to `build/RustedGlass.exe`. Run `cargo test --locked` to run the tests.
