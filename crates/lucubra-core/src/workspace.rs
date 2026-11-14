// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! A study workspace: layout, event log access, cache and projections
//! (SPEC section 5, ADR 0008 to ADR 0010).

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::appdata::{device_id, workspace_cache_dir};
use crate::cache::{Cache, Refresh};
use crate::error::{Error, Result};
use crate::event::Event;
use crate::hlc::{Clock, Hlc, SystemClock};
use crate::log::{BadLine, append_event, read_all_logs};
use crate::projection::{self, ProjectReport, content_hash, known_hashes, rel_to_path};
use crate::replay::{Replay, replay};
use crate::state::col;

/// Workspace format version written to `.lucubra/VERSION`.
pub const FORMAT_VERSION: &str = "1";

/// Folders created by `init` (SPEC section 5).
const LAYOUT: &[&str] = &[
    "inbox",
    "schedule",
    "subjects",
    "sessions",
    "packs",
    "dashboard",
    ".lucubra/events",
    ".lucubra/conflicts",
    ".lucubra/preserved",
    ".lucubra/written",
];

/// Whether `id` looks like a ULID (26 Crockford base-32 characters).
fn is_ulid(id: &str) -> bool {
    id.len() == 26
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b.is_ascii_uppercase() && !b"ILOU".contains(&b)))
}

/// Drops the Windows verbatim prefix (`\\?\C:\...`) that `canonicalize`
/// adds, when the remaining path is an ordinary drive path.
fn simplify(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest)
            if rest.len() >= 3
                && rest.as_bytes()[0].is_ascii_alphabetic()
                && &rest[1..3] == r":\"
                && rest.len() < 260 =>
        {
            PathBuf::from(rest)
        }
        _ => path,
    }
}

/// Counts reported by `status`.
#[derive(Debug, Clone)]
pub struct Status {
    /// Workspace id.
    pub workspace_id: String,
    /// This device.
    pub device: String,
    /// Unique events across all logs.
    pub events: usize,
    /// Log lines that did not parse.
    pub bad_lines: Vec<BadLine>,
    /// Events the fold skipped.
    pub skipped: usize,
    /// State hash from the in-memory replay.
    pub state_hash: String,
}

/// An open workspace bound to this device.
pub struct Workspace {
    root: PathBuf,
    data_dir: PathBuf,
    dev: String,
    clock: Box<dyn Clock + Send + Sync>,
}

impl Workspace {
    fn bind(root: &Path, data_dir: &Path) -> Result<Workspace> {
        let root = simplify(
            root.canonicalize()
                .map_err(|e| Error::io(format!("open {}", root.display()), e))?,
        );
        Ok(Workspace {
            dev: device_id(data_dir)?,
            root,
            data_dir: data_dir.to_path_buf(),
            clock: Box::new(SystemClock),
        })
    }

    /// Creates the workspace layout and records `workspace.initialized` if no
    /// device has done so yet. Safe to run on an existing workspace.
    pub fn init(root: &Path, data_dir: &Path) -> Result<Workspace> {
        fs::create_dir_all(root).map_err(|e| Error::io(format!("create {}", root.display()), e))?;
        let ws = Workspace::bind(root, data_dir)?;
        for dir in LAYOUT {
            let path = rel_to_path(&ws.root, dir);
            fs::create_dir_all(&path)
                .map_err(|e| Error::io(format!("create {}", path.display()), e))?;
        }
        let version = ws.lucubra_dir().join("VERSION");
        if !version.exists() {
            projection::write_atomic(&version, format!("{FORMAT_VERSION}\n").as_bytes())?;
        }
        let current = ws.replay()?;
        if current.state.get(col::WORKSPACE, "id").is_none() {
            let id = ulid::Ulid::new().to_string();
            ws.append_after(
                "workspace.initialized",
                json!({"workspace_id": id, "format": FORMAT_VERSION}),
                current.max_hlc.as_ref(),
            )?;
        }
        Ok(ws)
    }

    /// Opens an existing workspace.
    pub fn open(root: &Path, data_dir: &Path) -> Result<Workspace> {
        if !root.join(".lucubra").join("VERSION").is_file() {
            return Err(Error::Invalid(format!(
                "{} is not a Lucubra workspace; run `lucubra init` there first",
                root.display()
            )));
        }
        Workspace::bind(root, data_dir)
    }

    /// Replaces the clock, for tests.
    pub fn with_clock(mut self, clock: impl Clock + Send + Sync + 'static) -> Workspace {
        self.clock = Box::new(clock);
        self
    }

    /// Workspace root (canonical path).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// This device's identifier.
    pub fn device(&self) -> &str {
        &self.dev
    }

    fn lucubra_dir(&self) -> PathBuf {
        self.root.join(".lucubra")
    }

    fn events_dir(&self) -> PathBuf {
        self.lucubra_dir().join("events")
    }

    /// Reads every log and replays it in memory.
    pub fn replay(&self) -> Result<Replay> {
        Ok(replay(read_all_logs(&self.events_dir())?.events))
    }

    fn append_after(&self, kind: &str, data: Value, observed: Option<&Hlc>) -> Result<Event> {
        append_event(
            &self.events_dir(),
            &self.dev,
            kind,
            data,
            observed,
            self.clock.as_ref(),
        )
    }

    /// Appends one event to this device's log, ordered after every event seen.
    pub fn append(&self, kind: &str, data: Value) -> Result<Event> {
        let seen = self.replay()?.max_hlc;
        self.append_after(kind, data, seen.as_ref())
    }

    /// The workspace id from `workspace.initialized`.
    pub fn workspace_id(&self) -> Result<String> {
        let replay = self.replay()?;
        let id = replay
            .state
            .get(col::WORKSPACE, "id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Invalid("workspace has no workspace.initialized event".into()))?;
        if !is_ulid(id) {
            return Err(Error::Invalid(format!("workspace id '{id}' is not a ULID")));
        }
        Ok(id.to_owned())
    }

    /// This workspace's cache directory in the data directory.
    pub fn cache_dir(&self) -> Result<PathBuf> {
        Ok(workspace_cache_dir(&self.data_dir, &self.workspace_id()?))
    }

    /// Brings the cache up to date and returns the state hash read from it.
    pub fn refresh(&self) -> Result<(Refresh, String)> {
        let dir = self.cache_dir()?;
        fs::create_dir_all(&dir).map_err(|e| Error::io(format!("create {}", dir.display()), e))?;
        let mut cache = Cache::open(&dir.join("cache.sqlite"))?;
        let events = read_all_logs(&self.events_dir())?.events;
        let refresh = cache.refresh(events)?;
        let hash = cache.state_hash()?;
        Ok((refresh, hash))
    }

    /// Deletes this workspace's cache and rebuilds it from the logs.
    pub fn rebuild(&self) -> Result<(Refresh, String)> {
        self.delete_cache()?;
        self.refresh()
    }

    fn delete_cache(&self) -> Result<()> {
        let dir = self.cache_dir()?;
        match fs::remove_dir_all(&dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::io(format!("delete {}", dir.display()), e)),
        }
    }

    /// Regenerates the projections from replayed state.
    pub fn project(&self) -> Result<ProjectReport> {
        let current = self.replay()?;
        let rendered = projection::render(&current.state);
        let mut seen = current.max_hlc;
        let mut log = |data: Value| -> Result<()> {
            let event = self.append_after("projection.preserved", data, seen.as_ref())?;
            seen = Hlc::parse(&event.hlc);
            Ok(())
        };
        projection::project(
            &self.root,
            &self.dev,
            &rendered,
            self.clock.now_ms(),
            &mut log,
        )
    }

    /// Cache-only reset (ADR 0010): deletes the cache and every projection
    /// whose content was written by Lucubra, then rebuilds and regenerates.
    /// Event logs, preserved edits, conflict copies, sources and notes are
    /// never touched.
    pub fn reset(&self) -> Result<(String, ProjectReport)> {
        self.delete_cache()?;
        for (rel, hashes) in known_hashes(&self.lucubra_dir()) {
            // Sidecars sync from other devices: only paths with the shape of a
            // projection qualify, so no entry can name a user file.
            if !projection::is_projection_path(&rel) {
                continue;
            }
            let path = rel_to_path(&self.root, &rel);
            if let Ok(bytes) = fs::read(&path)
                && hashes.contains(&content_hash(&bytes))
            {
                fs::remove_file(&path)
                    .map_err(|e| Error::io(format!("delete {}", path.display()), e))?;
            }
        }
        let report = self.project()?;
        let (_, hash) = self.refresh()?;
        Ok((hash, report))
    }

    /// Counts and warnings for the workspace.
    pub fn status(&self) -> Result<Status> {
        let read = read_all_logs(&self.events_dir())?;
        let bad_lines = read.bad_lines.clone();
        let replayed = replay(read.events);
        Ok(Status {
            workspace_id: self.workspace_id()?,
            device: self.dev.clone(),
            events: replayed.ordered.len(),
            bad_lines,
            skipped: replayed.skipped.len(),
            state_hash: replayed.state.hash(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ulid_check() {
        assert!(is_ulid(&ulid::Ulid::new().to_string()));
        assert!(!is_ulid("../../etc"));
        assert!(!is_ulid("01JD3Q8ILOU00000000000000"));
    }
}
