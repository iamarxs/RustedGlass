# RustedGlass

![RustedGlass banner](assets/RustedGlass-banner.png)

RustedGlass is a fast, portable, Rust-based Windows image viewer with a minimal interface that combines on-the-fly decoding with a lean memory footprint.

## Download

Get a prebuilt binary from the [releases page](https://github.com/iamarxs/RustedGlass/releases), or build it yourself, instructions below.

## Features

- Open individual images, browse folders, or drag and drop files, folders, and playlists. Browse with the keyboard, mouse wheel, or toolbar.
- Minimal interface. Optionally hidable toolbar and a borderless mode, a simple settings menu and all functionality available through keyboard shortcuts and the right-click menu.
- Switch between framed, borderless, and fullscreen views, or keep the viewer on top. Window position and size are restored on restart.
- View JPEG, PNG, WebP, TIFF, BMP, GIF, ICO, QOI, DDS, HDR, TGA, EXR, Farbfeld, APNG, animated-WebP and Netpbm images. Animated GIFs, PNGs and WebPs play automatically.
- Zoom, pan, and rotate images. Embedded EXIF orientation is applied automatically.
- Inspect an image's full path, file size, dimensions, detected format, and available EXIF details. Copy the displayed image to the clipboard at its native resolution.
- Run slideshows with fixed or random intervals, random image order, loop, and an option to let GIF, APNG, and animated WebP finish one animation cycle before advancing.
- Build and reorder playlists with drag and drop, sort by name, and add the same image more than once. Save and reopen `.rgpl` playlists with relative paths.
- Rename, copy, move, and recycle files or reveal them in Explorer. Copy and move use numbered filenames when the destination already contains a file with the same name.
- Browse rapidly with a cache for recently decoded still images.
- Keep settings in a portable JSON file beside the executable, select another file with `--config`, or override settings for a single run.
- **F1** for the grouped keyboard shortcut list.

## Controls

| Key                                                   | Action                                                                                                                                                |
| ----------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| **F1**                                                | Show or hide keyboard shortcuts and mouse gestures. |
| **O** / **F**                                         | Open images or a folder — or drag and drop files, folders, and playlists onto the window.                                                             |
| **Arrows**, **Wheel up/down**                         | Browse.                                                                                                                                               |
| **Home** / **End**                                    | Jump to the first and last image. Rapid navigation loads the final selection after a short pause; recently decoded images are cached.                 |
| **0**, **+**/**−**, **Ctrl + Wheel up/down**, **1–6** | Use **+**, **−**, or **Ctrl + Wheel** to zoom. Choose a zoom mode with **1–6**, or use **0** or **5** to fit the image to the window. Enable **Pan image by dragging** in Settings to pan with the mouse. |
| **Space**                                             | Pause or resume the slideshow, or view the next image when the slideshow is off. |
| **R**                                                 | Rotate 90°                                                                                                                                            |
| **I**                                                 | Inspect path, size, dimensions, format, and EXIF details                                                                                              |
| **L**                                                 | Show or hide the playlist.                                                                                                                             |
| **T**                                                 | Show or hide the toolbar.                                                                                                                             |
| **F10**                                               | Framed ↔ borderless. Drag near the top to move a borderless window; drag its edges to resize.                                                         |
| **F11**                                               | Start or stop slideshow.                                                                                                                               |
| **F12**                                               | Fullscreen.                                                                                                                                           |
| **S** or **Ctrl + P**                                 | Open Settings.                                                                                                                                        |
| **Esc**                                               | Close the panel or playlist. Stop an active slideshow. Quit if neither applies. |

Use **Random image order** or **Ctrl + A** to pick images at random during navigation and slideshows. Playlist rows stay in place. In the playlist menu, **Sort by → Shuffle rows** rearranges the rows. **Loop slideshow** starts a new pass after the last slide. Set a minimum and maximum duration in Settings. Equal values give a fixed delay, and a maximum below the minimum is raised to match. **Finish one animation cycle per slide** is on by default and covers GIF, APNG, and animated WebP. Opening the playlist pauses an active slideshow, and closing the playlist resumes playback.

## Manage playlists and files

1. Press **L** or click **Show playlist** or **Hide playlist** on the toolbar. Click a row to select its image and drag a row to reorder the list.
2. Right-click in the playlist to add images or folders, sort by name, shuffle rows, or open and save a playlist. **A** adds images and **Shift + A** adds a folder.
3. Use **Ctrl + S** to save a `.rgpl` playlist and **Ctrl + O** to open one. You can also drop a `.rgpl` file onto the app or pass its path on the command line.

A `.rgpl` file is UTF-8 text with one image path per line. Lines beginning with `#` are comments. Relative paths resolve from the playlist's folder, so keep the same relative layout when moving a playlist and its images.

**Delete** removes selected entries from the playlist and keeps the files on disk. **Ctrl + Delete** sends the selected files to the Windows Recycle Bin. Use **Show selected image in Explorer** in the playlist menu to locate a selected file. In the viewer, **F2** renames the image, **C** opens **Save as** to save a copy in the original format, **M** moves the image to the quick-move destination, and **E** shows the image in Explorer.

## Settings and command-line use

**RustedGlass is portable.** When you click **Save & Close**, preferences are saved in `rgconfig.json` beside `RustedGlass.exe`. Use `--config <path>` (or `--config=<path>`) to read and save settings in a different file. Relative config paths are resolved from the working directory. File commands such as Copy, Move, Rename, and Recycle Bin act on the files you select.

From PowerShell in the folder containing `RustedGlass.exe`:

```powershell
.\RustedGlass.exe "./Pictures"
.\RustedGlass.exe "./Pictures/Weekend.rgpl" --fullscreen
.\RustedGlass.exe "./Pictures" -p:SlideshowInterval=8
.\RustedGlass.exe --config "./Settings/viewer.json" "./Pictures"
.\RustedGlass.exe --help
```

Use `--maximized` (`-m`), `--fullscreen` (`-f`), or `--topmost` (`-t`) to control the startup window. Use `-p:Key=value` for a setting override for that run.

## Build from source

Install [Rust and Cargo](https://rust-lang.org/tools/install/) with the `x86_64-pc-windows-msvc` toolchain. Install the [Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/) with the **Desktop development with C++** workload and a Windows SDK. Then run the build script in PowerShell from the source folder.

```powershell
.\build.ps1
```

Cargo downloads the pinned dependencies on the first build. The script writes the Windows x64 release executable to `build/RustedGlass.exe`. Run `cargo test --locked` to run the tests.

## AI Disclosure

The app was developed with the aid of several AI models, including OpenAI's GPT-6 Astra, GPT 6.1 Sol, Mistral's Mistral Large 4, Z.ai's GLM 5.3 and GLM 5.3-Flash, and Deepseek's DeepSeek-V4.1-Flash.

The development followed an orchestrator/subagent model, where I planned the work with the main orchestrator AI, which then maintained a list of tasks and passed them on to subagents. All work done by subagents was verified by the lead AI and myself, and periodically all models were tasked to run a security audit and a code review for the full project.

## Screenshot

![RustedGlass screenshot](assets/RustedGlass-Screenshot.png)

## Icons used

The app uses icons provided by [Tabler](https://tabler.io/icons) ([GitHub](https://github.com/tabler/tabler-icons)).

## License

MIT License, see LICENSE.
