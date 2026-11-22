// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! App-data directory and device identity (ADR 0008, ADR 0010).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Environment variable that overrides the data directory.
pub const DATA_DIR_ENV: &str = "LUCUBRA_DATA_DIR";

/// Resolves the data directory: `LUCUBRA_DATA_DIR` if set, otherwise the
/// platform's local data directory for `lucubra`.
pub fn default_data_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os(DATA_DIR_ENV).filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    directories::ProjectDirs::from("", "", "lucubra")
        .map(|dirs| dirs.data_local_dir().to_path_buf())
        .ok_or_else(|| Error::Invalid("no home directory to place Lucubra data in".into()))
}

/// Whether `id` has the device identifier form `d-` plus 8 lowercase hex digits.
pub fn is_device_id(id: &str) -> bool {
    id.strip_prefix("d-").is_some_and(|hex| {
        hex.len() == 8
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// Returns this install's device identifier, creating it on first use.
pub fn device_id(data_dir: &Path) -> Result<String> {
    fs::create_dir_all(data_dir)
        .map_err(|e| Error::io(format!("create {}", data_dir.display()), e))?;
    let path = data_dir.join("device_id");
    let fresh = format!("d-{:08x}", ulid::Ulid::new().random() as u32);
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            file.write_all(fresh.as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(|e| Error::io(format!("write {}", path.display()), e))?;
            Ok(fresh)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            // Another process may have created the file and not written it
            // yet: retry briefly before treating it as invalid.
            for _ in 0..50 {
                let text = fs::read_to_string(&path)
                    .map_err(|e| Error::io(format!("read {}", path.display()), e))?;
                let id = text.trim().to_owned();
                if is_device_id(&id) {
                    return Ok(id);
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(Error::Invalid(format!(
                "{} does not hold a device id",
                path.display()
            )))
        }
        Err(e) => Err(Error::io(format!("create {}", path.display()), e)),
    }
}

/// Cache directory for one workspace.
pub fn workspace_cache_dir(data_dir: &Path, workspace_id: &str) -> PathBuf {
    data_dir.join("workspaces").join(workspace_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_id_is_created_once() {
        let dir = tempfile::tempdir().unwrap();
        let first = device_id(dir.path()).unwrap();
        assert!(is_device_id(&first), "{first}");
        assert_eq!(device_id(dir.path()).unwrap(), first);
    }

    #[test]
    fn validates_device_ids() {
        assert!(is_device_id("d-0a1b2c3d"));
        for bad in ["d-0A1B2C3D", "d-123", "x-0a1b2c3d", "d-0a1b2c3d4"] {
            assert!(!is_device_id(bad), "{bad}");
        }
    }
}
