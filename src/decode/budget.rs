//! The app's image-memory budget: an image size is checked with checked arithmetic before any
//! full-size buffer is allocated.
//!
//! Account for the decoder's native output during orientation and conversion and for the
//! viewer-owned RGBA/BGRA buffers and DIB. Decoder-internal scratch follows the codec's own limits.
//!
//! The viewer's buffers are checked here because every `Decoded` the viewer draws comes from
//! this module.

/// App-controlled cap on the full-image buffers that can exist at one time.
pub const MAX_BUDGET: u64 = 512 * 1024 * 1024;

/// RGBA bytes per pixel in one buffer.
const BYTES_PER_PIXEL: u64 = 4;

/// Bytes for one RGBA buffer of this size, or `None` when the count overflows.
pub fn buffer_bytes(width: u32, height: u32) -> Option<u64> {
    u64::from(width)
        .checked_mul(u64::from(height))?
        .checked_mul(BYTES_PER_PIXEL)
}

/// Peak bytes this size can need while it is decoded and drawn, or `None` on overflow.
pub fn peak_bytes(width: u32, height: u32, native: u64) -> Option<u64> {
    let rgba = buffer_bytes(width, height)?;
    let orientation = native.checked_mul(2)?;
    let conversion = native.checked_add(rgba)?;
    let display = rgba.checked_mul(4)?;
    Some(orientation.max(conversion).max(display))
}

/// Rejects a size whose buffers would go over the budget, before anything is allocated.
pub fn check(width: u32, height: u32, native: u64) -> Result<(), String> {
    match peak_bytes(width, height, native) {
        Some(peak) if peak <= MAX_BUDGET => Ok(()),
        _ => Err(format!(
            "{width}x{height} needs more than {} MiB of image memory",
            MAX_BUDGET / (1024 * 1024)
        )),
    }
}
