//! Tests build their own signed manifests and ZIPs with a deterministic Ed25519 test key,
//! never the production private key.

use super::*;
use ed25519_dalek::{Signer, SigningKey};
use sha2::Digest;
use std::io::Write;
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

/// Fixed-seed key: the same key on every run, so signatures are stable and re-runnable.
fn test_key() -> SigningKey {
    SigningKey::from_bytes(&[0x42u8; 32])
}

/// A minimal but structurally valid x86-64 PE.
fn fake_exe() -> Vec<u8> {
    let mut exe = vec![0u8; 0x80];
    exe[0..2].copy_from_slice(b"MZ");
    exe[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());
    exe[0x40..0x44].copy_from_slice(b"PE\0\0");
    exe[0x44..0x46].copy_from_slice(&MACHINE_AMD64.to_le_bytes());
    exe
}

/// A signed manifest for `version` describing exactly these `zip` bytes.
fn signed_manifest(version: &str, zip: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let hex: String = Sha256::digest(zip)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let filename = format!("RustedGlass-{version}-{TARGET_TRIPLE}.zip");
    let body = format!(
        r#"{{"version":"{version}","target":"{TARGET_TRIPLE}","filename":"{filename}","sha256":"{hex}"}}"#
    )
    .into_bytes();
    let signature = test_key().sign(&body).to_bytes().to_vec();
    (body, signature)
}

/// A ZIP whose entries are given as (name, bytes) pairs, built with the given compression.
fn build_zip(compression: CompressionMethod, entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(compression);
    for (name, bytes) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn exe_zip(compression: CompressionMethod) -> Vec<u8> {
    build_zip(compression, &[(EXE_NAME, fake_exe())])
}

/// Verifies + extracts in one step, with the test key standing in for the pinned one.
fn roundtrip(zip: &[u8], body: &[u8], signature: &[u8]) -> Result<Vec<u8>, String> {
    let verified = verify_manifest_with_key(&test_key().verifying_key(), "1.2.3", body, signature)?;
    assert_eq!(
        verified.filename(),
        "RustedGlass-1.2.3-x86_64-pc-windows-msvc.zip"
    );
    verified.extract(zip)
}

#[test]
fn stored_and_deflated_archives_extract() {
    for compression in [CompressionMethod::Stored, CompressionMethod::Deflated] {
        let zip = exe_zip(compression);
        let (body, signature) = signed_manifest("1.2.3", &zip);
        assert_eq!(roundtrip(&zip, &body, &signature).unwrap(), fake_exe());
    }
}

#[test]
fn tampered_manifest_is_rejected_before_parsing() {
    let zip = exe_zip(CompressionMethod::Stored);
    let (mut body, signature) = signed_manifest("1.2.3", &zip);
    assert!(roundtrip(&zip, &body, &signature).is_ok());
    *body.last_mut().unwrap() ^= 1;
    assert!(roundtrip(&zip, &body, &signature)
        .unwrap_err()
        .contains("signature"));
}

#[test]
fn malformed_signatures_are_rejected() {
    let zip = exe_zip(CompressionMethod::Stored);
    let (body, signature) = signed_manifest("1.2.3", &zip);
    assert!(roundtrip(&zip, &body, &signature[..63])
        .unwrap_err()
        .contains("expected 64"));
    let forged = SigningKey::from_bytes(&[0x00; 32])
        .sign(&body)
        .to_bytes()
        .to_vec();
    assert!(roundtrip(&zip, &body, &forged)
        .unwrap_err()
        .contains("signature"));
}

#[test]
fn manifest_fields_are_pinned_to_the_contract() {
    let zip = exe_zip(CompressionMethod::Stored);
    let (body, _) = signed_manifest("1.2.3", &zip);
    for (find, replace, expected) in [
        ("\"1.2.3\"", "\"1.2.4\"", "version"),
        ("1.2.3-x86_64", "1.2.4-x86_64", "RustedGlass-"),
        (
            "\"x86_64-pc-windows-msvc\"",
            "\"aarch64-pc-windows-msvc\"",
            "target",
        ),
        ("RustedGlass-1.2.3", "RustedGlass", "manifest names"),
        ("\"sha256\"", "\"sha128\"", "sha256"),
    ] {
        let tampered = String::from_utf8(body.clone())
            .unwrap()
            .replace(find, replace);
        assert!(
            !tampered.as_bytes().eq(&body),
            "test replacement `{find}` did not hit the manifest"
        );
        // Re-sign the tampered manifest: the field checks must fail on their own, not on
        // the signature check.
        let signature = test_key().sign(tampered.as_bytes()).to_bytes().to_vec();
        let error = roundtrip(&zip, tampered.as_bytes(), &signature).unwrap_err();
        assert!(
            error.contains(expected),
            "wrong error for `{find}`: {error}"
        );
    }
}

#[test]
fn version_prefix_normalization_still_pins_the_release() {
    let zip = exe_zip(CompressionMethod::Stored);
    let (body, signature) = signed_manifest("1.2.3", &zip);
    // A `v` tag prefix matches the numeric triple, but a different number does not.
    assert!(
        verify_manifest_with_key(&test_key().verifying_key(), "v1.2.3", &body, &signature).is_ok()
    );
    assert!(
        verify_manifest_with_key(&test_key().verifying_key(), "1.2.4", &body, &signature).is_err()
    );
    assert!(
        verify_manifest_with_key(&test_key().verifying_key(), "1.2.3-x", &body, &signature)
            .is_err()
    );
}

#[test]
fn tampered_zip_fails_hash_before_parsing() {
    let mut zip = exe_zip(CompressionMethod::Stored);
    let (body, signature) = signed_manifest("1.2.3", &zip);
    let flip = zip.len() / 2;
    zip[flip] ^= 1;
    let error = roundtrip(&zip, &body, &signature).unwrap_err();
    assert!(error.contains("SHA-256"));
}

#[test]
fn crc_mismatch_inside_a_hash_valid_zip_is_rejected() {
    // Flip one entry byte *after* recording, so the outer hash is signed over corrupted
    // bytes and must pass, leaving only the stored CRC-32 to catch the corruption.
    let mut zip = exe_zip(CompressionMethod::Stored);
    let exe_offset = zip.windows(2).position(|w| w == b"MZ").unwrap();
    zip[exe_offset + 4] ^= 1;
    let (body, signature) = signed_manifest("1.2.3", &zip);
    let error = roundtrip(&zip, &body, &signature).unwrap_err();
    assert!(
        error.contains("CRC-32") || error.contains("checksum"),
        "expected CRC error, got: {error}"
    );
}

#[test]
fn zip_structure_is_pinned_to_one_root_exe() {
    let exe = fake_exe();
    for entries in [
        vec![("nested/RustedGlass.exe", exe.clone())],
        vec![("RustedGlass.bin", exe.clone())],
        vec![(EXE_NAME, exe.clone()), ("extra.txt", b"second".to_vec())],
        // A duplicate `RustedGlass.exe` cannot be written by a mature ZIP implementation.
    ] {
        let zip = build_zip(CompressionMethod::Stored, &entries);
        let (body, signature) = signed_manifest("1.2.3", &zip);
        let error = roundtrip(&zip, &body, &signature).unwrap_err();
        assert!(error.contains("exactly"), "unexpected error: {error}");
    }
}

#[test]
fn non_pe_or_wrong_architecture_is_rejected() {
    let mut i386 = fake_exe();
    i386[0x44..0x46].copy_from_slice(&0x014cu16.to_le_bytes());
    let junk = vec![b'M'; 0x80];
    for exe in [i386, junk] {
        let zip = build_zip(CompressionMethod::Stored, &[(EXE_NAME, exe.clone())]);
        let (body, signature) = signed_manifest("1.2.3", &zip);
        assert!(roundtrip(&zip, &body, &signature).is_err());
    }
}

#[test]
fn oversized_manifest_is_rejected() {
    let zip = exe_zip(CompressionMethod::Stored);
    let (body, _) = signed_manifest("1.2.3", &zip);
    let body = body.repeat(MAX_MANIFEST / body.len() + 1);
    let signature = test_key().sign(body.as_slice()).to_bytes().to_vec();
    assert!(verify_manifest("1.2.3", &body, &signature)
        .unwrap_err()
        .contains("bytes"));
}

#[test]
fn pinned_key_file_is_a_valid_ed25519_public_key() {
    assert!(pinned_key().is_ok());
}

#[test]
fn signatures_are_the_raw_64_byte_form() {
    let zip = exe_zip(CompressionMethod::Stored);
    let (_, signature) = signed_manifest("1.2.3", &zip);
    assert_eq!(signature.len(), SIGNATURE_LEN);
}
