//! GitHub release check and asset download; the About window runs it on a background thread.

use std::cmp::Ordering;
use std::ffi::c_void;
use std::io;
use std::mem::size_of;
use std::ptr;
use std::time::{Duration, Instant};

use serde_json::Value;
use windows::core::PCWSTR;
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption,
    WinHttpSetTimeouts, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_OPTION_REDIRECT_POLICY, WINHTTP_OPTION_REDIRECT_POLICY_DISALLOW_HTTPS_TO_HTTP,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};

const API_HOST: &str = "api.github.com";
const API_PATH: &str = "/repos/iamarxs/RustedGlass/releases/latest";
const DOWNLOAD_HOST: &str = "github.com";
const DOWNLOAD_PATH: &str = "/iamarxs/RustedGlass/releases/download";
const TARGET: &str = "x86_64-pc-windows-msvc";
const METADATA: [&str; 2] = ["update.json", "update.json.sig"];
const USER_AGENT: &str = concat!("RustedGlass/", env!("CARGO_PKG_VERSION"));
const TIMEOUT_MS: i32 = 5_000;
const TOTAL_TIMEOUT: Duration = Duration::from_secs(300);
const HTTPS_PORT: u16 = 443;

/// A `major.minor.patch` release version, ordered numerically.
type Version = (u32, u32, u32);

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum UpdateStatus {
    Available(String),
    Current,
    Ahead(String),
    NoStableRelease,
}

pub(crate) fn check() -> Result<UpdateStatus, String> {
    let body = get(API_HOST, API_PATH, 1024 * 1024)?.ok_or("GitHub returned HTTP 404.")?;
    compare_latest(&body, crate::VERSION)
}

fn compare_latest(body: &[u8], current: &str) -> Result<UpdateStatus, String> {
    let Some(latest) = parse_latest(body)? else {
        return Ok(UpdateStatus::NoStableRelease);
    };
    let current = parse_version(current)
        .ok_or_else(|| format!("cannot parse the running version {current}"))?;
    let version = || format!("{}.{}.{}", latest.0, latest.1, latest.2);
    Ok(match latest.cmp(&current) {
        Ordering::Greater => UpdateStatus::Available(version()),
        Ordering::Equal => UpdateStatus::Current,
        Ordering::Less => UpdateStatus::Ahead(version()),
    })
}

/// The stable release's version, or `None` for a draft or prerelease response.
fn parse_latest(body: &[u8]) -> Result<Option<Version>, String> {
    let json: Value =
        serde_json::from_slice(body).map_err(|e| format!("GitHub sent invalid JSON: {e}"))?;
    let unstable = ["draft", "prerelease"]
        .iter()
        .any(|key| json.get(*key).and_then(Value::as_bool) == Some(true));
    if unstable {
        return Ok(None);
    }
    let tag = json
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or("GitHub sent no tag_name")?;
    let version =
        parse_version(tag).ok_or_else(|| format!("cannot parse the release tag {tag}"))?;
    Ok(Some(version))
}

/// `v1.2.3` (or `1.2.3`) as numbers; anything else is not a published release version.
fn parse_version(text: &str) -> Option<Version> {
    let text = text.trim();
    let text = text.strip_prefix(['v', 'V']).unwrap_or(text);
    let mut parts = text.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    parts.next().is_none().then_some((major, minor, patch))
}

/// The fixed GitHub download path of one asset of a strict `major.minor.patch` `version`, or
/// `None`. Rebuilding the path here keeps the fetch off any URL the network supplies.
fn asset_path(version: &str, filename: &str) -> Option<String> {
    let version = version.strip_prefix('v').unwrap_or(version);
    let (major, minor, patch) = parse_version(version)?;
    if version != format!("{major}.{minor}.{patch}") {
        return None;
    }
    let published =
        METADATA.contains(&filename) || filename == format!("RustedGlass-{version}-{TARGET}.zip");
    published.then(|| format!("{DOWNLOAD_PATH}/v{version}/{filename}"))
}

/// Downloads one release asset, or `Ok(None)` when the release has none; `limit` bounds it and a
/// failed transfer is an error, never a shorter body.
pub(crate) fn fetch_asset(
    version: &str,
    filename: &str,
    limit: usize,
) -> Result<Option<Vec<u8>>, String> {
    let path = asset_path(version, filename)
        .ok_or_else(|| format!("{version} is not published with an asset named {filename}"))?;
    get(DOWNLOAD_HOST, &path, limit)
}

/// HTTPS GET of `https://host/path`, handles closed on drop; `Ok(None)` is HTTP 404 only.
fn get(host: &str, path: &str, limit: usize) -> Result<Option<Vec<u8>>, String> {
    let agent = crate::w32::wide(USER_AGENT);
    let session = Handle(unsafe {
        WinHttpOpen(
            PCWSTR(agent.as_ptr()),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        )
    });
    if session.0.is_null() {
        return Err(format!(
            "WinHttpOpen failed: {}",
            io::Error::last_os_error()
        ));
    }
    unsafe { WinHttpSetTimeouts(session.0, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS) }
        .map_err(|e| format!("cannot set network timeouts: {e}"))?;

    let wide_host = crate::w32::wide(host);
    let connect =
        Handle(unsafe { WinHttpConnect(session.0, PCWSTR(wide_host.as_ptr()), HTTPS_PORT, 0) });
    let verb = crate::w32::wide("GET");
    let wide_path = crate::w32::wide(path);
    let request = Handle(unsafe {
        WinHttpOpenRequest(
            connect.0,
            PCWSTR(verb.as_ptr()),
            PCWSTR(wide_path.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            ptr::null(),
            WINHTTP_FLAG_SECURE,
        )
    });
    // GitHub serves assets from a second HTTPS host: follow redirects, never downgrade to HTTP.
    let policy = WINHTTP_OPTION_REDIRECT_POLICY_DISALLOW_HTTPS_TO_HTTP.to_ne_bytes();
    unsafe {
        WinHttpSetOption(
            Some(request.0.cast_const()),
            WINHTTP_OPTION_REDIRECT_POLICY,
            Some(&policy),
        )
    }
    .map_err(|e| format!("cannot set the redirect policy: {e}"))?;

    unsafe { WinHttpSendRequest(request.0, None, None, 0, 0, 0) }
        .map_err(|e| format!("Could not reach GitHub: {e}"))?;
    unsafe { WinHttpReceiveResponse(request.0, ptr::null_mut()) }
        .map_err(|e| format!("cannot read the GitHub response: {e}"))?;

    let mut status = 0u32;
    let mut status_size = size_of::<u32>() as u32;
    unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&mut status as *mut u32).cast()),
            &mut status_size,
            ptr::null_mut(),
        )
    }
    .map_err(|e| format!("cannot read the HTTP status: {e}"))?;
    match status {
        200 => {}
        404 => return Ok(None),
        _ => return Err(format!("GitHub returned HTTP {status}.")),
    }

    let deadline = Instant::now() + TOTAL_TIMEOUT;
    let mut body = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        if Instant::now() > deadline {
            return Err("GitHub took too long to send the asset.".to_owned());
        }
        let mut read = 0u32;
        unsafe {
            WinHttpReadData(
                request.0,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                &mut read,
            )
        }
        .map_err(|e| format!("cannot read the GitHub response: {e}"))?;
        let read = read as usize;
        if read == 0 {
            return Ok(Some(body));
        }
        body.extend_from_slice(&buffer[..read]);
        if body.len() > limit {
            return Err(format!("the download exceeds {limit} bytes"));
        }
    }
}

/// Owns a WinHTTP handle; closing it is required on every path, including errors.
struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            let _ = unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STABLE: &[u8] = br#"{"tag_name":"v1.2.0","draft":false,"prerelease":false}"#;

    #[test]
    fn versions_parse_and_compare_numerically() {
        assert_eq!(parse_version("v1.0.10"), Some((1, 0, 10)));
        assert!(parse_version("1.0.10").unwrap() > parse_version("1.0.9").unwrap());
        assert!(parse_version("v2.0.0").unwrap() > parse_version("v1.99.99").unwrap());
        assert!(parse_version(crate::VERSION).is_some());
        for tag in ["", "v", "1.2", "1.2.3.4", "nightly", "1.x.3"] {
            assert_eq!(parse_version(tag), None, "{tag}");
        }
    }

    #[test]
    fn asset_paths_are_the_published_ones() {
        let base = "/iamarxs/RustedGlass/releases/download/v1.2.3/";
        for name in [
            "RustedGlass-1.2.3-x86_64-pc-windows-msvc.zip",
            "update.json",
            "update.json.sig",
        ] {
            let want = format!("{base}{name}");
            assert_eq!(asset_path("1.2.3", name).as_deref(), Some(want.as_str()));
        }
        assert_eq!(
            asset_path("v1.2.3", "update.json").as_deref(),
            Some("/iamarxs/RustedGlass/releases/download/v1.2.3/update.json")
        );
        for name in [
            "RustedGlass.exe",
            "RustedGlass-1.2.2-x86_64-pc-windows-msvc.zip",
            "../update.json",
            "update.json.sig/../update.json",
        ] {
            assert_eq!(asset_path("1.2.3", name), None, "{name}");
        }
        for version in ["V1.2.3", "01.2.3", "1.2", "1.2.3.4", "1.2.3-beta", " 1.2.3"] {
            assert_eq!(asset_path(version, "update.json"), None, "{version}");
        }
    }

    #[test]
    fn released_versions_are_compared_or_rejected() {
        assert_eq!(parse_latest(STABLE), Ok(Some((1, 2, 0))));
        assert_eq!(
            compare_latest(STABLE, "1.1.9"),
            Ok(UpdateStatus::Available("1.2.0".to_owned()))
        );
        assert_eq!(compare_latest(STABLE, "1.2.0"), Ok(UpdateStatus::Current));
        assert_eq!(
            compare_latest(STABLE, "1.2.1"),
            Ok(UpdateStatus::Ahead("1.2.0".to_owned()))
        );
        for body in [
            br#"{"tag_name":"v2.0.0-beta.1","prerelease":true}"#.as_slice(),
            br#"{"tag_name":"v2.0.0","draft":true}"#,
        ] {
            assert_eq!(parse_latest(body), Ok(None));
        }
        for body in [
            b"".as_slice(),
            br#"{"draft":false}"#,
            br#"{"tag_name":"main"}"#,
            br#"{"tag_name":7}"#,
        ] {
            assert!(parse_latest(body).is_err());
        }
    }
}
