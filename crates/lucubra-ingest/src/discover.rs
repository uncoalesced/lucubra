// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Workspace discovery (ADR 0011 section 1).
//!
//! Discovery never opens a file: it reads directory entries and metadata only,
//! so online-only placeholders are not downloaded.

use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use lucubra_core::projection::is_projection_path;
use lucubra_core::{Error, Result};

/// Top-level folders that are never ingested.
pub const SKIP_TOP: &[&str] = &[".lucubra", "packs", "sessions", "dashboard"];

/// Generated files outside the projection index set.
const GENERATED: &[&str] = &["schedule/lucubra-plan.ics"];

/// One file found in the workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovered {
    /// Workspace-relative path with `/` separators.
    pub rel: String,
    /// Absolute path.
    pub abs: PathBuf,
    /// Size in bytes, from metadata.
    pub bytes: u64,
    /// Modification time in milliseconds since the Unix epoch.
    pub mtime_ms: u64,
    /// True for a cloud placeholder whose contents are not on disk.
    pub online_only: bool,
}

/// What discovery found.
#[derive(Debug, Default)]
pub struct Discovery {
    /// Files, sorted by `rel`.
    pub files: Vec<Discovered>,
    /// Entries that could not be listed, with the reason.
    pub problems: Vec<String>,
}

impl Discovery {
    /// Online-only files: count and total size (S20).
    pub fn online_only(&self) -> (usize, u64) {
        self.files
            .iter()
            .filter(|f| f.online_only)
            .fold((0, 0), |(n, b), f| (n + 1, b + f.bytes))
    }
}

/// True when a workspace-relative path is never ingested.
pub fn is_skipped(rel: &str) -> bool {
    let first = rel.split('/').next().unwrap_or_default();
    if SKIP_TOP.contains(&first) {
        return true;
    }
    if rel
        .split('/')
        .any(|p| p.starts_with('.') || p.starts_with("~$"))
    {
        return true;
    }
    if is_projection_path(rel) || GENERATED.contains(&rel) {
        return true;
    }
    rel.ends_with(".transcript.md") && rel.split('/').any(|p| p == "notes")
}

#[cfg(windows)]
fn is_online_only(meta: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const OFFLINE: u32 = 0x1000;
    const RECALL_ON_OPEN: u32 = 0x0004_0000;
    const RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;
    meta.file_attributes() & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0
}

#[cfg(target_os = "macos")]
fn is_online_only(meta: &Metadata) -> bool {
    use std::os::macos::fs::MetadataExt;
    const SF_DATALESS: u32 = 0x4000_0000;
    meta.st_flags() & SF_DATALESS != 0
}

#[cfg(not(any(windows, target_os = "macos")))]
fn is_online_only(_meta: &Metadata) -> bool {
    false
}

fn mtime_ms(meta: &Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as u64)
}

/// Lists every ingestible file under `root`. Symbolic links are not followed,
/// so discovery stays inside the workspace.
pub fn discover(root: &Path) -> Result<Discovery> {
    let mut out = Discovery::default();
    let mut stack: Vec<(PathBuf, String)> = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if prefix.is_empty() => {
                return Err(Error::io(format!("listing {}", dir.display()), e));
            }
            Err(e) => {
                out.problems.push(format!("{prefix}: {e}"));
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    out.problems.push(format!("{prefix}: {e}"));
                    continue;
                }
            };
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                out.problems.push(format!(
                    "{prefix}/{}: name is not valid Unicode",
                    entry.file_name().to_string_lossy()
                ));
                continue;
            };
            let rel = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            if is_skipped(&rel) {
                continue;
            }
            let meta = match fs::symlink_metadata(entry.path()) {
                Ok(meta) => meta,
                Err(e) => {
                    out.problems.push(format!("{rel}: {e}"));
                    continue;
                }
            };
            let kind = meta.file_type();
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                stack.push((entry.path(), rel));
            } else if kind.is_file() {
                out.files.push(Discovered {
                    abs: entry.path(),
                    bytes: meta.len(),
                    mtime_ms: mtime_ms(&meta),
                    online_only: is_online_only(&meta),
                    rel,
                });
            }
        }
    }
    out.files.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(root: &Path, rel: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, rel.as_bytes()).unwrap();
    }

    #[test]
    fn discover_applies_skip_rules() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for rel in [
            "inbox/a.pdf",
            "subjects/maths/algebra/sources/b.pdf",
            "subjects/maths/algebra/notes/mine.md",
            "subjects/maths/algebra/notes/b.pdf.p1.transcript.md",
            "subjects/maths/algebra/UNIT.md",
            "subjects/maths/agents_maths.md",
            "AGENTS.md",
            ".lucubra/events/d-1.jsonl",
            "packs/p.md",
            "sessions/SESSION_x.md",
            "dashboard/index.html",
            "inbox/~$lock.docx",
            "inbox/.hidden",
            ".obsidian/app.json",
            "schedule/lucubra-plan.ics",
            "schedule/timetable.pdf",
        ] {
            touch(root, rel);
        }
        let found = discover(root).unwrap();
        let rels: Vec<&str> = found.files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(
            rels,
            [
                "inbox/a.pdf",
                "schedule/timetable.pdf",
                "subjects/maths/algebra/notes/mine.md",
                "subjects/maths/algebra/sources/b.pdf",
            ]
        );
        assert!(found.problems.is_empty());
        let a = &found.files[0];
        assert_eq!(a.bytes, "inbox/a.pdf".len() as u64);
        assert!(a.mtime_ms > 0);
        assert!(!a.online_only);
    }

    #[test]
    fn discover_missing_root_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(discover(&dir.path().join("absent")).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn discover_reports_offline_files_without_reading_them() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "inbox/cloud.pdf");
        touch(dir.path(), "inbox/local.pdf");
        let status = std::process::Command::new("attrib")
            .arg("+O")
            .arg(dir.path().join("inbox").join("cloud.pdf"))
            .status()
            .unwrap();
        assert!(status.success());
        let found = discover(dir.path()).unwrap();
        assert_eq!(found.online_only(), (1, "inbox/cloud.pdf".len() as u64));
        assert!(
            found
                .files
                .iter()
                .any(|f| f.rel == "inbox/cloud.pdf" && f.online_only)
        );
    }
}
