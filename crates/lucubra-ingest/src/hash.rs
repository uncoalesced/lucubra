// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Content hashing (ADR 0011 section 1).

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use lucubra_core::{Error, Result};

/// The BLAKE3 hash of a file's contents, as lowercase hex, and its length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hashed {
    /// Lowercase hex digest.
    pub hash: String,
    /// Bytes read.
    pub bytes: u64,
}

impl Hashed {
    /// The first eight hex digits, used in chunk references (`F:<hash8>`).
    pub fn hash8(&self) -> &str {
        &self.hash[..8]
    }
}

/// Hashes a file by streaming its contents.
pub fn hash_file(path: &Path) -> Result<Hashed> {
    let ctx = || format!("hashing {}", path.display());
    let file = File::open(path).map_err(|e| Error::io(ctx(), e))?;
    let mut reader = BufReader::with_capacity(1 << 16, file);
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut bytes = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| Error::io(ctx(), e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        bytes += n as u64;
    }
    Ok(Hashed {
        hash: hasher.finalize().to_hex().to_string(),
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_matches_one_shot_blake3() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &data).unwrap();
        let h = hash_file(&path).unwrap();
        assert_eq!(h.hash, blake3::hash(&data).to_hex().to_string());
        assert_eq!(h.bytes, data.len() as u64);
        assert_eq!(h.hash8().len(), 8);
    }
}
