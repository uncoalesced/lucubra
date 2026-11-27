// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Extraction limits (ADR 0011 section 4).

/// Input file size; larger files are `limit_exceeded` before any read.
pub const MAX_FILE_BYTES: u64 = 1 << 30;
/// Zip containers: declared uncompressed total.
pub const MAX_ZIP_DECLARED_BYTES: u64 = 512 << 20;
/// Zip containers: entry count.
pub const MAX_ZIP_ENTRIES: usize = 20_000;
/// Zip containers: bytes actually inflated.
pub const MAX_ZIP_READ_BYTES: u64 = 512 << 20;
/// Zip containers: entries above this size are ratio-checked.
pub const ZIP_RATIO_FLOOR_BYTES: u64 = 1 << 20;
/// Zip containers: maximum compression ratio for large entries.
pub const MAX_ZIP_RATIO: u64 = 200;
/// Extracted text kept per file.
pub const MAX_TEXT_BYTES: usize = 64 << 20;
/// Long edge of a rendered page image, in pixels.
pub const RENDER_LONG_EDGE: u32 = 1600;
/// A PDF page with fewer non-whitespace characters than this needs vision.
pub const VISION_MIN_CHARS: usize = 40;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn limits_are_consistent() {
        assert!(MAX_ZIP_READ_BYTES <= MAX_ZIP_DECLARED_BYTES);
        assert!(MAX_ZIP_DECLARED_BYTES < MAX_FILE_BYTES);
        assert!(ZIP_RATIO_FLOOR_BYTES < MAX_ZIP_READ_BYTES);
        assert!(MAX_ZIP_RATIO > 1 && MAX_ZIP_ENTRIES > 0);
        assert!((MAX_TEXT_BYTES as u64) < MAX_FILE_BYTES);
        assert!(RENDER_LONG_EDGE >= 1000 && VISION_MIN_CHARS > 0);
    }
}
