// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Debounced filesystem watcher over the workspace (SPEC section 10.1, ADR 0011).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use lucubra_core::{Error, Result};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};

use crate::discover::is_skipped;

/// Watches a workspace root and emits filtered workspace-relative paths.
pub struct WorkspaceWatcher {
    root: PathBuf,
    _watcher: RecommendedWatcher,
    rx: Receiver<notify::Result<Event>>,
    debounce: Duration,
}

fn to_rel(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let joined = rel
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect::<Vec<_>>()
        .join("/");
    (!joined.is_empty() && !is_skipped(&joined)).then_some(joined)
}

impl WorkspaceWatcher {
    /// Starts watching `root` recursively with the given debounce window.
    pub fn start(root: &Path, debounce: Duration) -> Result<Self> {
        let canonical = root
            .canonicalize()
            .map_err(|e| Error::io(format!("canonicalising {}", root.display()), e))?;
        let (tx, rx) = mpsc::channel();
        let mut watcher = RecommendedWatcher::new(
            move |res| {
                let _ = tx.send(res);
            },
            Config::default(),
        )
        .map_err(|e| Error::Invalid(format!("starting watcher: {e}")))?;
        watcher
            .watch(&canonical, RecursiveMode::Recursive)
            .map_err(|e| Error::Invalid(format!("watching {}: {e}", canonical.display())))?;
        Ok(Self {
            root: canonical,
            _watcher: watcher,
            rx,
            debounce,
        })
    }

    /// Waits up to `wait` for an initial change, then debounces for `debounce`
    /// and returns sorted, non-skipped workspace-relative paths.
    pub fn next_batch(&self, wait: Duration) -> Vec<String> {
        let mut changed = BTreeSet::new();
        let first = match self.rx.recv_timeout(wait) {
            Ok(ev) => ev,
            Err(_) => return Vec::new(),
        };
        if let Ok(event) = first {
            self.collect(&event, &mut changed);
        }
        let deadline = Instant::now() + self.debounce;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            match self.rx.recv_timeout(left) {
                Ok(Ok(event)) => self.collect(&event, &mut changed),
                Ok(Err(_)) => {}
                Err(_) => break,
            }
        }
        changed.into_iter().collect()
    }

    fn collect(&self, event: &Event, out: &mut BTreeSet<String>) {
        for path in &event.paths {
            if let Some(rel) = to_rel(&self.root, path) {
                out.insert(rel);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn watcher_emits_inbox_changes_and_ignores_skipped_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("inbox")).unwrap();
        fs::create_dir_all(root.join(".lucubra/events")).unwrap();
        let watcher = WorkspaceWatcher::start(root, Duration::from_millis(100)).unwrap();

        fs::write(root.join(".lucubra/events/d-1.jsonl"), b"{}\n").unwrap();
        fs::write(root.join("inbox/note.md"), b"# Note\n").unwrap();

        let mut seen = BTreeSet::new();
        for _ in 0..8 {
            for rel in watcher.next_batch(Duration::from_millis(400)) {
                seen.insert(rel);
            }
            if seen.contains("inbox/note.md") {
                break;
            }
        }
        assert!(seen.contains("inbox/note.md"), "seen: {seen:?}");
        assert!(
            !seen.iter().any(|p| p.starts_with(".lucubra")),
            "skipped dir leaked: {seen:?}"
        );
    }
}
