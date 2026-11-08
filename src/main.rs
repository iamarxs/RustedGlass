//! The command line entry point: parse the arguments, then hand everything to `view`, which owns
//! the window and the state it shows.

// A windowed app: starting it must not leave a console window open behind the viewer. `-h`, `-V`
// and a failed start write to the terminal that launched the app by attaching to its console.
#![windows_subsystem = "windows"]

mod about;
mod atomic_file;
mod bar;
mod cache;
mod config;
mod decode;
mod dialog;
mod fileops;
mod frame_hit;
mod input;
mod listdraw;
mod metadata;
mod paint;
mod photo;
mod playlist;
mod render;
mod rgpl;
mod state;
mod style;
mod tools;
mod view;
mod w32;
mod zoom;

use std::path::PathBuf;

const VERSION: &str = env!("CARGO_PKG_VERSION");

const HELP: &str = "\
RustedGlass — the ImageGlass viewer, ported to Rust

Usage:
  RustedGlass [PATH] [options]

Arguments:
  PATH                  Image, folder or `.rgpl` playlist to open
                        Use -- before a path that starts with a dash

Options:
  --config <path>       Read and save settings in this file
  -f, --fullscreen      Start full screen
  -h, --help            Show this help
  -m, --maximized       Start maximized
  -p:Key=value          Set a config value for this run
  -t, --topmost         Keep the window above the others
  -V, --version         Print the version and exit

Keys:
  Esc                   Close the panel, then the image, then the window
  F1                    Toggle the shortcut list
  F10                   Toggle the window frame
  F11                   Start or stop the slideshow
  F12                   Toggle full screen
  L                     Toggle the playlist
  S                     Open settings
  T                     Toggle the toolbar

By default, settings are read from and written to `rgconfig.json` next to the executable.
";

fn main() {
    let (config_path, original_args) = match config_option(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(error) => fail(&error),
    };
    let options = || original_args.iter().take_while(|arg| arg.as_str() != "--");
    if options().any(|arg| arg == "--help" || arg == "-h") {
        attach_parent_console();
        print!("{HELP}");
        return;
    }
    if options().any(|arg| arg == "--version" || arg == "-V") {
        attach_parent_console();
        println!("RustedGlass {VERSION}");
        return;
    }

    // The shorthand flags are the `-p:` form of the settings they stand for, so `config` reads
    // them like any other command line override.
    let mut args = setting_overrides(&original_args);
    args.extend(original_args);

    let config_path = match config_path {
        Some(path) => {
            let path = std::path::absolute(&path).unwrap_or_else(|error| fail(&error.to_string()));
            if !path.parent().is_some_and(|parent| parent.is_dir()) {
                fail(&format!(
                    "config folder does not exist for {}",
                    path.display()
                ));
            }
            if let Err(error) = config::read_user_file(&path) {
                fail(&error);
            }
            path
        }
        None => app_dir().join(config::CONFIG_USER),
    };
    match launch_detached_from_msys() {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => fail(&format!("could not launch the viewer: {error}")),
    }
    if let Err(error) = view::run(config_path, &args) {
        fail(&error);
    }
}

const CHILD_MARKER: &str = "RUSTEDGLASS_DETACHED_VIEWER";

fn launch_detached_from_msys() -> std::io::Result<bool> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    const DETACHED_PROCESS: u32 = 0x0000_0008;
    if std::env::var_os("MSYSTEM").is_none() || std::env::var_os(CHILD_MARKER).is_some() {
        return Ok(false);
    }
    // Git Bash waits for the executable it started. The detached child owns the window.
    Command::new(std::env::current_exe()?)
        .args(std::env::args_os().skip(1))
        .env(CHILD_MARKER, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(DETACHED_PROCESS)
        .spawn()?;
    Ok(true)
}

fn config_option(
    mut args: impl Iterator<Item = String>,
) -> Result<(Option<PathBuf>, Vec<String>), String> {
    let mut path = None;
    let mut remaining = Vec::new();
    while let Some(arg) = args.next() {
        if arg == "--" {
            remaining.push(arg);
            remaining.extend(args);
            break;
        }
        let value = if arg == "--config" {
            args.next().filter(|value| !value.starts_with('-'))
        } else if let Some(value) = arg.strip_prefix("--config=") {
            Some(value.to_owned())
        } else {
            remaining.push(arg);
            continue;
        };
        let value = value
            .filter(|value| !value.is_empty())
            .ok_or("--config requires a file path")?;
        if path.replace(PathBuf::from(value)).is_some() {
            return Err("--config may only be specified once".into());
        }
    }
    Ok((path, remaining))
}

fn fail(message: &str) -> ! {
    if std::env::var_os(CHILD_MARKER).is_some() {
        use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        let text = w32::wide(message);
        let _ = unsafe {
            MessageBoxW(
                None,
                windows::core::PCWSTR(text.as_ptr()),
                windows::core::w!("RustedGlass"),
                MB_OK | MB_ICONERROR,
            )
        };
    } else {
        attach_parent_console();
        eprintln!("rustedglass: {message}");
    }
    std::process::exit(1);
}

/// Reaches the console of the process that started us, so the messages above are not thrown away
/// by a windowed app. Without one there is nothing to attach to and the call simply fails.
fn attach_parent_console() {
    unsafe {
        let _ = windows::Win32::System::Console::AttachConsole(
            windows::Win32::System::Console::ATTACH_PARENT_PROCESS,
        );
    }
}

/// Settings live next to the executable, so they follow this build instead of the opened folder.
fn app_dir() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|dir| dir.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

/// `-m`, `-f` and `-t` as `-p:` overrides of the settings `Knobs` reads.
fn setting_overrides(args: &[String]) -> Vec<String> {
    const FLAGS: [(&str, &str, &str); 3] = [
        ("-m", "--maximized", "EnableMainWindowMaximized"),
        ("-f", "--fullscreen", "EnableFullScreen"),
        ("-t", "--topmost", "EnableWindowTopMost"),
    ];

    FLAGS
        .iter()
        .filter(|(short, long, _)| {
            args.iter()
                .take_while(|arg| arg.as_str() != "--")
                .any(|arg| arg == short || arg == long)
        })
        .map(|(_, _, key)| format!("-p:{key}=true"))
        .collect()
}

#[cfg(test)]
fn test_path(name: &str) -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target"));
    let dir = std::path::absolute(target)
        .unwrap()
        .join("test-files")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_option_consumes_only_the_config_argument() {
        let parse = |args: &[&str]| config_option(args.iter().map(|arg| (*arg).to_string()));
        assert_eq!(
            parse(&["--config", "profiles/my config.json", "image.png", "-f"]).unwrap(),
            (
                Some(PathBuf::from("profiles/my config.json")),
                vec!["image.png".into(), "-f".into()],
            ),
        );
        assert_eq!(
            parse(&["image.png", "--config=-profile.json"]).unwrap(),
            (
                Some(PathBuf::from("-profile.json")),
                vec!["image.png".into()]
            ),
        );
        assert!(parse(&["--config", "--fullscreen"]).is_err());
        assert!(parse(&["--config="]).is_err());
        assert!(parse(&["--config=a.json", "--config=b.json"]).is_err());
    }

    #[test]
    fn flags_become_the_settings_the_help_promises() {
        assert_eq!(
            setting_overrides(&["-f".into(), "--topmost".into(), "images/img.png".into()]),
            vec![
                "-p:EnableFullScreen=true".to_string(),
                "-p:EnableWindowTopMost=true".to_string()
            ],
        );
        assert!(setting_overrides(&["images/img.png".into()]).is_empty());
    }
}
