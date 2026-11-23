//! Checks GitHub for a newer published release. The About window runs `check` on a background
//! thread, so it blocks only that thread.

use std::cmp::Ordering;
use std::ffi::c_void;
use std::io;
use std::mem::size_of;
use std::ptr;

use serde_json::Value;
use windows::core::{w, PCWSTR};
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE,
};

const HOST: &str = "api.github.com";
const PATH: &str = "/repos/iamarxs/RustedGlass/releases/latest";
const USER_AGENT: &str = concat!("RustedGlass/", env!("CARGO_PKG_VERSION"));
const TIMEOUT_MS: i32 = 5_000;
const MAX_BODY: usize = 1024 * 1024;
const HTTPS_PORT: u16 = 443;

/// A release version, ordered numerically as `major.minor.patch`.
type Version = (u32, u32, u32);

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum UpdateStatus {
    Available(String),
    Current,
    Ahead(String),
    NoStableRelease,
}

pub(crate) fn check() -> Result<UpdateStatus, String> {
    let body = fetch()?;
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

/// HTTPS GET of the latest release: WinHTTP session, connection and request closed on drop.
fn fetch() -> Result<Vec<u8>, String> {
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

    let host = crate::w32::wide(HOST);
    let connect =
        Handle(unsafe { WinHttpConnect(session.0, PCWSTR(host.as_ptr()), HTTPS_PORT, 0) });
    if connect.0.is_null() {
        return Err(format!(
            "cannot connect to {HOST}: {}",
            io::Error::last_os_error()
        ));
    }

    let verb = crate::w32::wide("GET");
    let path = crate::w32::wide(PATH);
    let request = Handle(unsafe {
        WinHttpOpenRequest(
            connect.0,
            PCWSTR(verb.as_ptr()),
            PCWSTR(path.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            ptr::null(),
            WINHTTP_FLAG_SECURE,
        )
    });
    if request.0.is_null() {
        return Err(format!(
            "cannot create a request for {HOST}{PATH}: {}",
            io::Error::last_os_error()
        ));
    }

    let accept = w!("Accept: application/vnd.github+json\r\n");
    unsafe { WinHttpSendRequest(request.0, Some(accept.as_wide()), None, 0, 0, 0) }
        .map_err(|e| format!("Could not reach GitHub: {e}"))?;
    unsafe { WinHttpReceiveResponse(request.0, ptr::null_mut()) }
        .map_err(|e| format!("cannot read the update response: {e}"))?;

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
    if status != 200 {
        return Err(format!("GitHub returned HTTP {status}."));
    }

    let mut body = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let mut read = 0u32;
        unsafe {
            WinHttpReadData(
                request.0,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                &mut read,
            )
        }
        .map_err(|e| format!("cannot read the update response: {e}"))?;
        let read = read as usize;
        if read == 0 {
            return Ok(body);
        }
        body.extend_from_slice(&buffer[..read]);
        if body.len() > MAX_BODY {
            return Err(format!("the update response exceeds {MAX_BODY} bytes"));
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
    fn versions_compare_numerically() {
        let latest = parse_version("v1.0.10").unwrap();
        assert!(latest > parse_version("1.0.9").unwrap());
        assert!(parse_version("v2.0.0").unwrap() > parse_version("v1.99.99").unwrap());
        assert_eq!(
            parse_version("v1.0.5").unwrap(),
            parse_version("1.0.5").unwrap()
        );
        assert!(parse_version(crate::VERSION).is_some());
    }

    #[test]
    fn malformed_versions_are_rejected() {
        for tag in [
            "",
            "v",
            "1.2",
            "1.2.3.4",
            "1.2.3-beta.1",
            "nightly",
            "v1.x.3",
        ] {
            assert!(parse_version(tag).is_none(), "{tag} should not parse");
        }
    }

    #[test]
    fn stable_release_is_read() {
        assert_eq!(parse_latest(STABLE), Ok(Some((1, 2, 0))));
    }

    #[test]
    fn compares_the_running_build_with_the_published_release() {
        assert_eq!(
            compare_latest(STABLE, "1.1.9"),
            Ok(UpdateStatus::Available("1.2.0".to_owned()))
        );
        assert_eq!(compare_latest(STABLE, "1.2.0"), Ok(UpdateStatus::Current));
        assert_eq!(
            compare_latest(STABLE, "1.2.1"),
            Ok(UpdateStatus::Ahead("1.2.0".to_owned()))
        );
    }

    #[test]
    fn unstable_release_is_not_an_update() {
        let prerelease = br#"{"tag_name":"v2.0.0-beta.1","prerelease":true}"#;
        let draft = br#"{"tag_name":"v2.0.0","draft":true}"#;
        assert_eq!(parse_latest(prerelease), Ok(None));
        assert_eq!(parse_latest(draft), Ok(None));
    }

    #[test]
    fn malformed_responses_are_errors() {
        assert!(parse_latest(b"").is_err());
        assert!(parse_latest(br#"{"draft":false}"#).is_err());
        assert!(parse_latest(br#"{"tag_name":"main"}"#).is_err());
        assert!(parse_latest(br#"{"tag_name":7}"#).is_err());
    }
}
