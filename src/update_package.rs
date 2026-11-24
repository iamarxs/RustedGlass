//! Verifies a signed update manifest and the archive it names, per the shared frozen contract:
//! Ed25519 over the exact manifest bytes first, then a strict schema and hash check, then a
//! single-file ZIP extraction whose bytes are never trusted before the SHA-256 matches.

use ed25519_dalek::{pkcs8::DecodePublicKey, Signature, VerifyingKey};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{Cursor, Read};
use zip::ZipArchive;

/// The committed release-signing key; the pinned trust anchor of the whole update path.
const PUBLIC_KEY_PEM: &str = include_str!("../assets/update-public.pem");
const TARGET_TRIPLE: &str = "x86_64-pc-windows-msvc";
const EXE_NAME: &str = "RustedGlass.exe";
const SIGNATURE_LEN: usize = 64;
const MAX_MANIFEST: usize = 64 * 1024;
const MAX_ZIP: usize = 64 * 1024 * 1024;
pub(crate) const MAX_EXE: usize = 64 * 1024 * 1024;
const MACHINE_AMD64: u16 = 0x8664;

/// A manifest whose signature and schema checked out; `extract` then verifies the archive
/// against the pinned checksum before returning any bytes.
#[derive(Debug)]
pub(crate) struct VerifiedArchive {
    filename: String,
    sha256: [u8; 32],
}

impl VerifiedArchive {
    /// The GitHub asset the caller must download: `RustedGlass-<version>-<target>.zip`.
    pub(crate) fn filename(&self) -> &str {
        &self.filename
    }

    /// Checks the ZIP's SHA-256 first, then unpacks the single root `RustedGlass.exe`.
    pub(crate) fn extract(&self, zip: &[u8]) -> Result<Vec<u8>, String> {
        if zip.len() > MAX_ZIP {
            return Err(format!("the downloaded archive exceeds {MAX_ZIP} bytes"));
        }
        // Hash before parse: unverified bytes never reach the ZIP parser.
        if Sha256::digest(zip).as_slice() != self.sha256 {
            return Err("the downloaded archive does not match the signed SHA-256".to_owned());
        }
        let mut archive = ZipArchive::new(Cursor::new(zip))
            .map_err(|e| format!("the downloaded archive is not a valid ZIP file: {e}"))?;
        if archive.len() != 1 {
            return Err(format!(
                "the archive holds {} entries, expected exactly one {EXE_NAME}",
                archive.len()
            ));
        }
        let mut entry = archive
            .by_index(0)
            .map_err(|e| format!("cannot open the archive entry: {e}"))?;
        let name = entry
            .name()
            .map_err(|e| format!("cannot read the archive entry name: {e}"))?;
        if entry.is_dir() || &*name != EXE_NAME {
            return Err(format!(
                "the archive entry is `{name}`, expected exactly `{EXE_NAME}` at the root"
            ));
        }
        if entry.size() > MAX_EXE as u64 {
            return Err(format!("the unpacked {EXE_NAME} exceeds {MAX_EXE} bytes"));
        }
        let mut exe = Vec::new();
        (&mut entry)
            .take(MAX_EXE as u64 + 1)
            .read_to_end(&mut exe)
            .map_err(|e| format!("cannot read the archive entry: {e}"))?;
        if exe.len() > MAX_EXE {
            return Err(format!("the unpacked {EXE_NAME} exceeds {MAX_EXE} bytes"));
        }
        validate_pe_x64(&exe)?;
        Ok(exe)
    }
}

/// Verifies the release manifest: strict Ed25519 signature over the exact bytes, then JSON.
pub(crate) fn verify_manifest(
    expected_version: &str,
    body: &[u8],
    signature: &[u8],
) -> Result<VerifiedArchive, String> {
    if body.len() > MAX_MANIFEST {
        return Err(format!("the manifest exceeds {MAX_MANIFEST} bytes"));
    }
    verify_manifest_with_key(&pinned_key()?, expected_version, body, signature)
}

/// Same as [`verify_manifest`], but against a caller-supplied key (the test key path).
fn verify_manifest_with_key(
    key: &VerifyingKey,
    expected_version: &str,
    body: &[u8],
    signature: &[u8],
) -> Result<VerifiedArchive, String> {
    // Signature before JSON: a forged or tampered manifest is never parsed, only rejected.
    if signature.len() != SIGNATURE_LEN {
        return Err(format!(
            "the manifest signature is {} bytes, expected {SIGNATURE_LEN}",
            signature.len()
        ));
    }
    key.verify_strict(
        body,
        &Signature::from_slice(signature).expect("length checked above"),
    )
    .map_err(|_| "the manifest signature does not match the release key".to_owned())?;

    let json: Value =
        serde_json::from_slice(body).map_err(|e| format!("the manifest is not valid JSON: {e}"))?;
    let version = text_field(&json, "version")?;
    let version = normalized_version(version)
        .ok_or_else(|| format!("the manifest version `{version}` is not a release version"))?;
    let expected = normalized_version(expected_version)
        .ok_or_else(|| format!("cannot parse the expected version {expected_version}"))?;
    if version != expected {
        return Err(format!(
            "the manifest offers version {version}, update was checked for {expected}"
        ));
    }
    let target = text_field(&json, "target")?;
    if target != TARGET_TRIPLE {
        return Err(format!(
            "the manifest targets `{target}`, not `{TARGET_TRIPLE}`"
        ));
    }
    let filename = text_field(&json, "filename")?;
    let expected_name = format!("RustedGlass-{version}-{TARGET_TRIPLE}.zip");
    if filename != expected_name {
        return Err(format!(
            "the manifest names `{filename}`, expected `{expected_name}`"
        ));
    }
    let hex = text_field(&json, "sha256")?;
    let lowercase_hex = hex.len() == 64
        && hex
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
    if !lowercase_hex {
        return Err("the manifest checksum is not 64 lowercase hex characters".to_owned());
    }
    let mut sha256 = [0u8; 32];
    for (out, pair) in sha256.iter_mut().zip(hex.as_bytes().chunks(2)) {
        let hi = (pair[0] as char).to_digit(16).unwrap_or(0);
        let lo = (pair[1] as char).to_digit(16).unwrap_or(0);
        *out = (hi * 16 + lo) as u8;
    }
    Ok(VerifiedArchive {
        filename: filename.to_owned(),
        sha256,
    })
}

/// `1.2.3` (with or without a leading `v`) as three numeric components, nothing else.
fn normalized_version(text: &str) -> Option<String> {
    let text = text.trim().strip_prefix(['v', 'V']).unwrap_or(text);
    let mut parts = text.split('.');
    let (Some(major), Some(minor), Some(patch)) = (
        parts.next()?.parse::<u32>().ok(),
        parts.next()?.parse::<u32>().ok(),
        parts.next()?.parse::<u32>().ok(),
    ) else {
        return None;
    };
    if parts.next().is_some() {
        return None;
    }
    Some(format!("{major}.{minor}.{patch}"))
}

/// A required manifest string field.
fn text_field<'a>(json: &'a Value, key: &str) -> Result<&'a str, String> {
    json.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("the manifest has no string field `{key}`"))
}

/// The Ed25519 key pinned in `assets/update-public.pem`, as raw 32 bytes.
fn pinned_key() -> Result<VerifyingKey, String> {
    VerifyingKey::from_public_key_pem(PUBLIC_KEY_PEM)
        .map_err(|error| format!("Could not read the bundled update public key: {error}"))
}

/// Minimal PE sanity: DOS header, PE signature, and x86-64 machine type.
fn validate_pe_x64(exe: &[u8]) -> Result<(), String> {
    if !exe.starts_with(b"MZ") {
        return Err("the unpacked file has no DOS header".to_owned());
    }
    let offset = exe
        .get(0x3c..0x40)
        .and_then(|bytes| usize::try_from(u32::from_le_bytes(bytes.try_into().unwrap())).ok())
        .ok_or("the unpacked file's DOS header is truncated")?;
    let pe = exe
        .get(offset..offset + 6)
        .ok_or("the unpacked file's PE header is missing")?;
    if pe[..4] != *b"PE\0\0" {
        return Err("the unpacked file has no PE signature".to_owned());
    }
    let machine = u16::from_le_bytes(pe[4..6].try_into().unwrap());
    if machine != MACHINE_AMD64 {
        return Err(format!(
            "the unpacked file is not an {TARGET_TRIPLE} binary"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
