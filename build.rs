//! Embeds an application manifest, so the common controls (check boxes, buttons, list views)
//! are drawn by the current Windows theme instead of the classic Win32 look. Without it the
//! controls of the settings panel and the tool bar look like Windows 3.1.

use std::env;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=assets/RustedGlass.ico");

    let rustc = Command::new(env::var_os("RUSTC").expect("Cargo Rust compiler"))
        .arg("--version")
        .output()
        .expect("could not query the Rust compiler version");
    assert!(rustc.status.success(), "rustc --version failed");
    println!(
        "cargo:rustc-env=RG_RUSTC={}",
        String::from_utf8_lossy(&rustc.stdout).trim()
    );
    println!(
        "cargo:rustc-env=RG_TARGET={}",
        env::var("TARGET").expect("Cargo build target")
    );
    println!(
        "cargo:rustc-env=RG_PROFILE={}",
        env::var("PROFILE").expect("Cargo build profile")
    );

    let resource =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory")).join("app.res");
    let compile = |compiler| {
        Command::new(compiler)
            .arg("/nologo")
            .arg("/fo")
            .arg(&resource)
            .arg("app.rc")
            .status()
    };
    let status = match compile(PathBuf::from("rc.exe")) {
        Err(error) if error.kind() == ErrorKind::NotFound => compile(sdk_resource_compiler()),
        result => result,
    }
    .expect("could not run the Windows SDK resource compiler");
    assert!(status.success(), "could not compile app.rc");
    println!("cargo:rustc-link-arg={}", resource.display());
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' \
name='Microsoft.Windows.Common-Controls' version='6.0.0.0' \
processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
    );
}

fn sdk_resource_compiler() -> PathBuf {
    let arch = match env::consts::ARCH {
        "aarch64" => "arm64",
        "x86" => "x86",
        _ => "x64",
    };
    if let Some(version_bin) = env::var_os("WindowsSdkVerBinPath") {
        let compiler = PathBuf::from(version_bin).join(arch).join("rc.exe");
        if compiler.is_file() {
            return compiler;
        }
    }

    let sdk = env::var_os("WindowsSdkDir")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env::var_os("ProgramFiles(x86)").expect("Program Files (x86)"))
                .join("Windows Kits/10")
        });
    std::fs::read_dir(sdk.join("bin"))
        .expect("could not find the Windows SDK bin directory")
        .flatten()
        .map(|entry| entry.path().join(arch).join("rc.exe"))
        .filter(|path| path.is_file())
        .max()
        .expect("could not find rc.exe in the Windows SDK")
}
