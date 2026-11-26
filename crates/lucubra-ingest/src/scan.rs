// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! One scan of a workspace: discover, hash what changed, log `file.seen`.
//!
//! Modification times differ between devices for a synced file, so they are
//! never compared against the event log. Each device keeps its own stat cache
//! in app-data (`scan-stat.json`), keyed by path, holding the size, the
//! modification time and the hash it last computed.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use lucubra_core::hlc::Hlc;
use lucubra_core::projection::write_atomic;
use lucubra_core::state::col;
use lucubra_core::{Error, Result, Workspace};
use serde_json::{Value, json};

use crate::discover::discover;
use crate::hash::hash_file;
use crate::route::{Format, route, route_file};

/// Stat cache file name inside the workspace's app-data directory.
pub const STAT_CACHE: &str = "scan-stat.json";
/// Flush stat cache every N newly-hashed files so large scans resume cleanly.
const STAT_FLUSH_INTERVAL: usize = 200;

/// Synthetic `files` collection key for an online-only placeholder path.
pub fn online_only_key(rel: &str) -> String {
    format!("online-only:{}", blake3::hash(rel.as_bytes()).to_hex())
}

/// A file hashed during this scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scanned {
    /// Workspace-relative path.
    pub rel: String,
    /// Absolute path.
    pub abs: PathBuf,
    /// BLAKE3 hex digest.
    pub hash: String,
    /// Size in bytes.
    pub bytes: u64,
    /// Routed format.
    pub format: Format,
    /// True when a `file.seen` event was written for it.
    pub logged: bool,
}

/// What one scan did.
#[derive(Debug, Default)]
pub struct ScanReport {
    /// Files hashed in this scan.
    pub hashed: Vec<Scanned>,
    /// Files skipped because their size and modification time had not changed.
    pub unchanged: usize,
    /// Online-only files: paths and total bytes, never read (S20).
    pub online_only: Vec<(String, u64)>,
    /// Files that could not be listed or hashed, with the reason.
    pub problems: Vec<String>,
}

impl ScanReport {
    /// Number of `file.seen` events written.
    pub fn logged(&self) -> usize {
        self.hashed.iter().filter(|s| s.logged).count()
    }

    /// Total size of online-only files.
    pub fn online_only_bytes(&self) -> u64 {
        self.online_only.iter().map(|(_, b)| b).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Stat {
    bytes: u64,
    mtime_ms: u64,
    hash: String,
}

fn load_stats(path: &Path) -> BTreeMap<String, Stat> {
    let Ok(text) = fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&text) else {
        return BTreeMap::new();
    };
    map.into_iter()
        .filter_map(|(rel, v)| {
            Some((
                rel,
                Stat {
                    bytes: v.get("bytes")?.as_u64()?,
                    mtime_ms: v.get("mtime_ms")?.as_u64()?,
                    hash: v.get("hash")?.as_str()?.to_owned(),
                },
            ))
        })
        .collect()
}

fn save_stats(path: &Path, stats: &BTreeMap<String, Stat>) -> Result<()> {
    let map: serde_json::Map<String, Value> = stats
        .iter()
        .map(|(rel, s)| {
            (
                rel.clone(),
                json!({"bytes": s.bytes, "mtime_ms": s.mtime_ms, "hash": s.hash}),
            )
        })
        .collect();
    let text = serde_json::to_string(&Value::Object(map)).map_err(Error::from)?;
    write_atomic(path, text.as_bytes())
}

/// Scans the workspace once.
pub fn scan(ws: &Workspace) -> Result<ScanReport> {
    let replay = ws.replay()?;
    let state = replay.state;
    let mut observed = replay.max_hlc;
    let cache_dir = ws.cache_dir()?;
    fs::create_dir_all(&cache_dir)
        .map_err(|e| Error::io(format!("creating {}", cache_dir.display()), e))?;
    let stat_path = cache_dir.join(STAT_CACHE);
    let old_stats = load_stats(&stat_path);
    let mut stats = BTreeMap::new();

    let found = discover(ws.root())?;
    let mut report = ScanReport {
        problems: found.problems,
        ..ScanReport::default()
    };
    let mut hashed_since_flush = 0usize;

    for file in found.files {
        if file.online_only {
            let key = online_only_key(&file.rel);
            let already_pending = state.get(col::FILES, &key).is_some_and(|entry| {
                entry.get("status").and_then(Value::as_str) == Some("pending")
                    && entry.get("code").and_then(Value::as_str) == Some("online_only")
            });
            if !already_pending {
                // Route by extension only so zero bytes of the placeholder are read.
                let format = route(&file.abs, &[]);
                let ev = ws.append_observed(
                    "file.failed",
                    json!({
                        "hash": key,
                        "path": file.rel,
                        "format": format.as_str(),
                        "outcome": "pending",
                        "code": "online_only",
                        "detail": "online-only cloud placeholder; not downloaded",
                    }),
                    observed.as_ref(),
                )?;
                observed = Hlc::parse(&ev.hlc).or(observed);
            }
            report.online_only.push((file.rel, file.bytes));
            continue;
        }
        let logged_hash = state
            .get(col::PATHS, &file.rel)
            .and_then(|v| v.get("hash"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        if let Some(old) = old_stats.get(&file.rel)
            && old.bytes == file.bytes
            && old.mtime_ms == file.mtime_ms
            && logged_hash.as_deref() == Some(old.hash.as_str())
        {
            stats.insert(file.rel, old.clone());
            report.unchanged += 1;
            continue;
        }
        let hashed = match hash_file(&file.abs) {
            Ok(h) => h,
            Err(e) => {
                report.problems.push(format!("{}: {e}", file.rel));
                continue;
            }
        };
        let format = route_file(&file.abs).unwrap_or(Format::Unknown);
        let logged = logged_hash.as_deref() != Some(hashed.hash.as_str());
        if logged {
            let ev = ws.append_observed(
                "file.seen",
                json!({
                    "path": file.rel,
                    "hash": hashed.hash,
                    "bytes": hashed.bytes,
                    "mtime_ms": file.mtime_ms,
                    "online_only": false,
                }),
                observed.as_ref(),
            )?;
            observed = Hlc::parse(&ev.hlc).or(observed);
        }
        stats.insert(
            file.rel.clone(),
            Stat {
                bytes: file.bytes,
                mtime_ms: file.mtime_ms,
                hash: hashed.hash.clone(),
            },
        );
        report.hashed.push(Scanned {
            rel: file.rel,
            abs: file.abs,
            hash: hashed.hash,
            bytes: hashed.bytes,
            format,
            logged,
        });
        hashed_since_flush += 1;
        if hashed_since_flush >= STAT_FLUSH_INTERVAL {
            save_stats(&stat_path, &stats)?;
            hashed_since_flush = 0;
        }
    }
    save_stats(&stat_path, &stats)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn online_only_keys_are_stable_and_distinct() {
        let a = online_only_key("inbox/big.pdf");
        assert_eq!(a, online_only_key("inbox/big.pdf"));
        assert_ne!(a, online_only_key("inbox/big2.pdf"));
        assert!(a.starts_with("online-only:") && !a.contains("big"));
    }

    #[test]
    fn stat_cache_round_trips_and_tolerates_damage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STAT_CACHE);
        assert!(load_stats(&path).is_empty(), "missing file");
        let mut stats = BTreeMap::new();
        stats.insert(
            "inbox/a.md".to_owned(),
            Stat {
                bytes: 12,
                mtime_ms: 1_700_000_000_000,
                hash: "h1".into(),
            },
        );
        save_stats(&path, &stats).unwrap();
        assert_eq!(load_stats(&path), stats);

        fs::write(
            &path,
            "{\"ok\":{\"bytes\":1,\"mtime_ms\":2,\"hash\":\"h\"},\"bad\":{\"bytes\":\"x\"}}",
        )
        .unwrap();
        let loaded = load_stats(&path);
        assert_eq!(loaded.len(), 1, "the damaged entry is dropped");
        fs::write(&path, "not json").unwrap();
        assert!(load_stats(&path).is_empty());
        fs::write(&path, "[1,2]").unwrap();
        assert!(load_stats(&path).is_empty());
    }
}
