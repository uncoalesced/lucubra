// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Zip container limits checked before and during inflation (ADR 0011 section 4).

use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::Path;

use zip::ZipArchive;
use zip::result::ZipError;

use crate::limits::{
    MAX_FILE_BYTES, MAX_ZIP_DECLARED_BYTES, MAX_ZIP_ENTRIES, MAX_ZIP_RATIO, MAX_ZIP_READ_BYTES,
    ZIP_RATIO_FLOOR_BYTES,
};
use crate::native::Failure;

fn fail(code: &'static str, detail: impl Into<String>) -> Failure {
    Failure {
        code,
        detail: detail.into(),
    }
}

fn map_zip_err(err: ZipError) -> Failure {
    match err {
        ZipError::UnsupportedArchive(msg) if msg.to_ascii_lowercase().contains("encrypt") => {
            fail("encrypted", "archive is password-protected")
        }
        other => fail("corrupt", format!("damaged zip container: {other}")),
    }
}

/// Verifies file size, entry count, declared sizes, actual inflated bytes, and
/// compression ratios across all entries in a zip archive.
pub fn check_zip(path: &Path) -> Result<(), Failure> {
    let meta = fs::metadata(path).map_err(|e| fail("corrupt", e.to_string()))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(fail(
            "limit_exceeded",
            format!("{} bytes is over the input limit", meta.len()),
        ));
    }

    let file = File::open(path).map_err(|e| fail("corrupt", e.to_string()))?;
    let mut archive = ZipArchive::new(BufReader::new(file)).map_err(map_zip_err)?;
    let count = archive.len();
    if count > MAX_ZIP_ENTRIES {
        return Err(fail(
            "limit_exceeded",
            format!("{count} zip entries is over the limit of {MAX_ZIP_ENTRIES}"),
        ));
    }

    let mut declared_total: u64 = 0;
    for i in 0..count {
        let entry = archive.by_index_raw(i).map_err(map_zip_err)?;
        if entry.encrypted() {
            return Err(fail("encrypted", "archive contains an encrypted entry"));
        }
        declared_total = declared_total.saturating_add(entry.size());
        if declared_total > MAX_ZIP_DECLARED_BYTES {
            return Err(fail(
                "limit_exceeded",
                format!("declares over {MAX_ZIP_DECLARED_BYTES} uncompressed bytes"),
            ));
        }
        if entry.size() > ZIP_RATIO_FLOOR_BYTES {
            let ratio = entry.size() / entry.compressed_size().max(1);
            if ratio > MAX_ZIP_RATIO {
                let name = String::from_utf8_lossy(entry.name_raw());
                return Err(fail(
                    "limit_exceeded",
                    format!(
                        "entry {name} declares expansion ratio {ratio}:1, over {MAX_ZIP_RATIO}:1"
                    ),
                ));
            }
        }
    }

    let mut total_read: u64 = 0;
    let mut buf = [0u8; 64 * 1024];
    for i in 0..count {
        let mut entry = archive.by_index(i).map_err(map_zip_err)?;
        let compressed = entry.compressed_size().max(1);
        let name = String::from_utf8_lossy(entry.name_raw()).into_owned();
        let mut entry_read: u64 = 0;
        loop {
            let n = entry
                .read(&mut buf)
                .map_err(|e| fail("corrupt", format!("reading zip entry {name}: {e}")))?;
            if n == 0 {
                break;
            }
            entry_read = entry_read.saturating_add(n as u64);
            total_read = total_read.saturating_add(n as u64);
            if total_read > MAX_ZIP_READ_BYTES {
                return Err(fail(
                    "limit_exceeded",
                    format!("inflated bytes exceed {MAX_ZIP_READ_BYTES}"),
                ));
            }
            if entry_read > ZIP_RATIO_FLOOR_BYTES && entry_read / compressed > MAX_ZIP_RATIO {
                return Err(fail(
                    "limit_exceeded",
                    format!(
                        "entry {name} expands {}:1 while streaming, over {MAX_ZIP_RATIO}:1",
                        entry_read / compressed
                    ),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use zip::CompressionMethod;
    use zip::write::SimpleFileOptions;

    use super::*;

    fn write_zip(path: &Path, entries: &[(&str, &[u8], CompressionMethod)]) {
        let file = File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        for (name, body, method) in entries {
            let opts = SimpleFileOptions::default().compression_method(*method);
            writer.start_file(*name, opts).unwrap();
            writer.write_all(body).unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn valid_zip_passes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ok.zip");
        write_zip(
            &path,
            &[("a.txt", b"hello world", CompressionMethod::Deflated)],
        );
        assert!(check_zip(&path).is_ok());
    }

    #[test]
    fn ratio_bomb_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bomb.zip");
        let zeros = vec![0u8; 4 * 1024 * 1024];
        write_zip(&path, &[("pad.bin", &zeros, CompressionMethod::Deflated)]);
        let err = check_zip(&path).unwrap_err();
        assert_eq!(err.code, "limit_exceeded");
    }

    #[test]
    fn non_zip_is_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.zip");
        fs::write(&path, b"not a zip archive").unwrap();
        let err = check_zip(&path).unwrap_err();
        assert_eq!(err.code, "corrupt");
    }

    #[test]
    fn lying_header_is_caught() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lying.zip");
        let zeros = vec![0u8; 3 * 1024 * 1024];
        write_zip(&path, &[("pad.bin", &zeros, CompressionMethod::Deflated)]);
        let mut raw = fs::read(&path).unwrap();
        // Patch every declared uncompressed size matching 3 MiB down to 1 KiB
        // in both local header and central directory.
        let target = (3u32 * 1024 * 1024).to_le_bytes();
        let lied = 1024u32.to_le_bytes();
        for i in 0..=raw.len().saturating_sub(4) {
            if raw[i..i + 4] == target {
                raw[i..i + 4].copy_from_slice(&lied);
            }
        }
        fs::write(&path, raw).unwrap();
        let err = check_zip(&path).unwrap_err();
        assert!(
            matches!(err.code, "limit_exceeded" | "corrupt"),
            "unexpected code: {} ({})",
            err.code,
            err.detail
        );
    }
}
