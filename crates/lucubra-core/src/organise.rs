// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! First-run layout proposal, approved file moves and undo (SPEC section 10.4,
//! addendum A3). Moves never overwrite, never leave the workspace and never
//! modify file contents; each one is a `file.moved` event.

use std::collections::BTreeMap;
use std::fs;

use serde_json::{Value, json};

use crate::paths::resolve_workspace_path;
use crate::projection::{free_path, is_slug, rel_string, rel_to_path};
use crate::state::{State, col};
use crate::{Error, Result, Workspace};

/// Top-level folders Lucubra manages; they are never reorganised.
const FIXED_DIRS: &[&str] = &[
    ".lucubra",
    "subjects",
    "schedule",
    "sessions",
    "packs",
    "dashboard",
];

/// Folder-name fragments that suggest a folder belongs to another tool or
/// account, so it should be indexed in place rather than moved (addendum A3).
const SHARED_HINTS: &[&str] = &[
    "classroom",
    "shared",
    "onedrive",
    "google drive",
    "dropbox",
    "icloud",
    "teams",
];

/// Folder handling chosen at first-run approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderMode {
    /// Move the user's files into `subjects/...`.
    Move,
    /// Leave files in place and link them by path.
    Index,
}

impl FolderMode {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            FolderMode::Move => "move",
            FolderMode::Index => "index",
        }
    }

    /// Parses a wire name.
    pub fn parse(s: &str) -> Option<FolderMode> {
        match s {
            "move" => Some(FolderMode::Move),
            "index" => Some(FolderMode::Index),
            _ => None,
        }
    }
}

/// One planned or requested move, workspace-relative with forward slashes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Move {
    /// Current path.
    pub from: String,
    /// Destination path.
    pub to: String,
}

/// The proposed first organisation, shown before anything moves.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Proposal {
    /// Top-level folder (`""` for files at the root) to suggested mode.
    pub folders: BTreeMap<String, FolderMode>,
    /// Moves for classified files in folders marked `move`.
    pub moves: Vec<Move>,
    /// Paths with no classification yet; they stay where they are.
    pub unclassified: Vec<String>,
}

/// Outcome of applying or undoing moves.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MoveReport {
    /// Batch id of the moves done (empty if nothing moved).
    pub batch: String,
    /// Moves done, with the final destination after any collision suffix.
    pub moved: Vec<Move>,
    /// Requests refused, with the reason.
    pub refused: Vec<(String, String)>,
}

fn top_folder(path: &str) -> &str {
    path.split_once('/').map_or("", |(top, _)| top)
}

fn suggest_mode(folder: &str) -> FolderMode {
    let lower = folder.to_lowercase();
    if SHARED_HINTS.iter().any(|h| lower.contains(h)) {
        FolderMode::Index
    } else {
        FolderMode::Move
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// Builds the first-run proposal from replayed state, with optional folder
/// mode overrides. Pure: touches no files.
pub fn propose(state: &State, overrides: &BTreeMap<String, FolderMode>) -> Proposal {
    let mut proposal = Proposal::default();
    for (path, location) in state.collection(col::PATHS) {
        let top = top_folder(path);
        if FIXED_DIRS.contains(&top) {
            continue;
        }
        let mode = *proposal.folders.entry(top.to_owned()).or_insert_with(|| {
            overrides
                .get(top)
                .copied()
                .unwrap_or_else(|| suggest_mode(top))
        });
        let file = location
            .get("hash")
            .and_then(Value::as_str)
            .and_then(|h| state.get(col::FILES, h));
        let Some(subject) = file
            .and_then(|f| f.get("subject"))
            .and_then(Value::as_str)
            .filter(|s| is_slug(s))
        else {
            proposal.unclassified.push(path.clone());
            continue;
        };
        if mode == FolderMode::Index {
            continue;
        }
        let unit = file
            .and_then(|f| f.get("unit"))
            .and_then(Value::as_str)
            .filter(|u| is_slug(u))
            .unwrap_or("general");
        proposal.moves.push(Move {
            from: path.clone(),
            to: format!("subjects/{subject}/{unit}/sources/{}", file_name(path)),
        });
    }
    proposal
}

fn check_target(root: &std::path::Path, to: &str) -> std::result::Result<String, String> {
    let (rel, _) = resolve_workspace_path(root, to)?;
    let top = top_folder(&rel);
    if top != "subjects" && FIXED_DIRS.contains(&top) {
        return Err(format!(
            "destination '{rel}' is inside a folder Lucubra manages"
        ));
    }
    Ok(rel)
}

/// Applies approved moves. Each move is validated on its own; refused moves
/// are reported and the rest still run. Nothing is ever overwritten: a taken
/// destination gets a `-N` suffix.
pub fn apply(ws: &Workspace, moves: &[Move], session_id: Option<&str>) -> Result<MoveReport> {
    let root = ws.root();
    let state = ws.replay()?.state;
    let mut report = MoveReport::default();
    let batch = ulid::Ulid::new().to_string();
    for request in moves {
        let from = match resolve_workspace_path(root, &request.from) {
            Ok((rel, _)) => rel,
            Err(why) => {
                report.refused.push((request.from.clone(), why));
                continue;
            }
        };
        let Some(hash) = state
            .get(col::PATHS, &from)
            .and_then(|l| l.get("hash"))
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            report.refused.push((
                from,
                "not a file Lucubra has scanned; run a scan first".into(),
            ));
            continue;
        };
        let to = match check_target(root, &request.to) {
            Ok(rel) => rel,
            Err(why) => {
                report.refused.push((from, why));
                continue;
            }
        };
        let src = rel_to_path(root, &from);
        if !src.is_file() {
            report
                .refused
                .push((from, "file no longer exists at this path".into()));
            continue;
        }
        let dest = free_path(rel_to_path(root, &to));
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| Error::io(format!("create {}", parent.display()), e))?;
        }
        fs::rename(&src, &dest).map_err(|e| Error::io(format!("move {from}"), e))?;
        let final_to = rel_string(root, &dest);
        let mut data = json!({
            "move_id": ulid::Ulid::new().to_string(),
            "hash": hash,
            "from": from,
            "to": final_to,
            "batch": batch,
        });
        if let Some(sid) = session_id {
            data["session_id"] = Value::String(sid.to_owned());
        }
        ws.append("file.moved", data)?;
        report.moved.push(Move { from, to: final_to });
    }
    if !report.moved.is_empty() {
        report.batch = batch;
    }
    Ok(report)
}

/// Which moves `undo` reverses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoScope {
    /// The most recent batch.
    Last,
    /// Every move not yet undone.
    All,
}

/// Reverses moves, newest first. A taken original path gets a `-N` suffix.
pub fn undo(ws: &Workspace, scope: UndoScope, session_id: Option<&str>) -> Result<MoveReport> {
    let root = ws.root();
    let state = ws.replay()?.state;
    // Move ids are ULIDs, so reversed key order is newest first.
    let mut open: Vec<(&String, &Value)> = state
        .collection(col::MOVES)
        .filter(|(_, m)| m.get("undone") != Some(&Value::Bool(true)))
        .filter(|(_, m)| {
            session_id.is_none_or(|sid| m.get("session_id").and_then(Value::as_str) == Some(sid))
        })
        .collect();
    open.reverse();
    if scope == UndoScope::Last
        && let Some(last) = open.first().and_then(|(_, m)| m.get("batch")).cloned()
    {
        open.retain(|(_, m)| m.get("batch") == Some(&last));
    }
    let mut report = MoveReport::default();
    for (id, entry) in open {
        let (Some(from), Some(to)) = (
            entry.get("from").and_then(Value::as_str),
            entry.get("to").and_then(Value::as_str),
        ) else {
            continue;
        };
        let current = rel_to_path(root, to);
        if !current.is_file() {
            report.refused.push((
                to.to_owned(),
                "moved file is no longer at its new path".into(),
            ));
            continue;
        }
        let dest = free_path(rel_to_path(root, from));
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| Error::io(format!("create {}", parent.display()), e))?;
        }
        fs::rename(&current, &dest).map_err(|e| Error::io(format!("undo move {to}"), e))?;
        let restored = rel_string(root, &dest);
        ws.append(
            "file.move_undone",
            json!({"move_id": id, "from": to, "to": restored}),
        )?;
        if let Some(batch) = entry.get("batch").and_then(Value::as_str) {
            report.batch = batch.to_owned();
        }
        report.moved.push(Move {
            from: to.to_owned(),
            to: restored,
        });
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    type Fixture = (tempfile::TempDir, tempfile::TempDir, Workspace);

    fn ws_with(files: &[(&str, &[u8])]) -> Fixture {
        let root = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let ws = Workspace::init(root.path(), data.path()).unwrap();
        for (rel, bytes) in files {
            let p = rel_to_path(root.path(), rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, bytes).unwrap();
            let hash = blake3::hash(bytes).to_hex().to_string();
            ws.append(
                "file.seen",
                json!({"path": rel, "hash": hash, "bytes": bytes.len()}),
            )
            .unwrap();
        }
        (root, data, ws)
    }

    fn classify(ws: &Workspace, bytes: &[u8], subject: &str) {
        let hash = blake3::hash(bytes).to_hex().to_string();
        ws.append(
            "file.classified",
            json!({"hash": hash, "subject": subject, "by": "engine"}),
        )
        .unwrap();
    }

    fn mv(from: &str, to: &str) -> Move {
        Move {
            from: from.into(),
            to: to.into(),
        }
    }

    #[test]
    fn organise_proposal_moves_nothing_and_respects_shared_folders() {
        let (root, _d, ws) = ws_with(&[
            ("Stats/notes.txt", b"stat201 notes"),
            ("Google Classroom/hand.txt", b"handout"),
            ("loose.txt", b"loose"),
        ]);
        classify(&ws, b"stat201 notes", "stats");
        classify(&ws, b"handout", "stats");
        let state = ws.replay().unwrap().state;
        let p = propose(&state, &BTreeMap::new());
        assert_eq!(p.folders["Stats"], FolderMode::Move);
        assert_eq!(p.folders["Google Classroom"], FolderMode::Index);
        assert_eq!(
            p.moves,
            vec![mv(
                "Stats/notes.txt",
                "subjects/stats/general/sources/notes.txt"
            )]
        );
        assert_eq!(p.unclassified, vec!["loose.txt".to_string()]);
        assert!(root.path().join("Stats/notes.txt").is_file());

        let overrides = BTreeMap::from([("Stats".to_string(), FolderMode::Index)]);
        assert!(propose(&state, &overrides).moves.is_empty());
    }

    #[test]
    fn organise_apply_suffixes_collisions_and_undo_restores_bytes() {
        let (root, _d, ws) = ws_with(&[("A/n.txt", b"one"), ("B/n.txt", b"two")]);
        let to = "subjects/s/general/sources/n.txt";
        let r = apply(&ws, &[mv("A/n.txt", to), mv("B/n.txt", to)], None).unwrap();
        assert_eq!(r.moved.len(), 2);
        assert_eq!(r.moved[1].to, "subjects/s/general/sources/n-1.txt");
        assert_eq!(fs::read(root.path().join(to)).unwrap(), b"one");
        let live = ws.replay().unwrap().state;
        assert!(live.get(col::PATHS, "A/n.txt").is_none());
        assert!(live.get(col::PATHS, to).is_some());

        let u = undo(&ws, UndoScope::Last, None).unwrap();
        assert_eq!(u.moved.len(), 2);
        assert_eq!(fs::read(root.path().join("A/n.txt")).unwrap(), b"one");
        assert_eq!(fs::read(root.path().join("B/n.txt")).unwrap(), b"two");
        assert!(
            ws.replay()
                .unwrap()
                .state
                .get(col::PATHS, "B/n.txt")
                .is_some()
        );
        assert!(undo(&ws, UndoScope::All, None).unwrap().moved.is_empty());
    }

    #[test]
    fn organise_undo_last_only_reverses_newest_batch() {
        let (root, _d, ws) = ws_with(&[("A/a.txt", b"a"), ("B/b.txt", b"b")]);
        apply(&ws, &[mv("A/a.txt", "subjects/s/u/sources/a.txt")], None).unwrap();
        apply(&ws, &[mv("B/b.txt", "subjects/s/u/sources/b.txt")], None).unwrap();
        undo(&ws, UndoScope::Last, None).unwrap();
        assert!(root.path().join("B/b.txt").is_file());
        assert!(root.path().join("subjects/s/u/sources/a.txt").is_file());
        undo(&ws, UndoScope::All, None).unwrap();
        assert!(root.path().join("A/a.txt").is_file());
    }

    #[test]
    fn organise_refuses_escapes_and_unknown_files() {
        let (root, _d, ws) = ws_with(&[("A/n.txt", b"one")]);
        fs::write(root.path().join("stray.txt"), b"x").unwrap();
        let bad = [
            mv("A/n.txt", "../out.txt"),
            mv("A/n.txt", "C:/Windows/out.txt"),
            mv("A/n.txt", "/etc/out.txt"),
            mv("A/n.txt", ".lucubra/events/x.txt"),
            mv("A/n.txt", "sessions/x.txt"),
            mv("stray.txt", "subjects/s/u/sources/stray.txt"),
            mv("../A/n.txt", "subjects/s/u/sources/n.txt"),
        ];
        let r = apply(&ws, &bad, None).unwrap();
        assert!(r.moved.is_empty(), "{:?}", r.moved);
        assert_eq!(r.refused.len(), bad.len());
        assert!(root.path().join("A/n.txt").is_file());
    }

    #[test]
    fn organise_replay_of_moves_matches_rebuilt_state() {
        let (_root, _d, ws) = ws_with(&[("A/n.txt", b"one")]);
        apply(&ws, &[mv("A/n.txt", "subjects/s/u/sources/n.txt")], None).unwrap();
        let live = ws.replay().unwrap().state.hash();
        let (_, rebuilt) = ws.rebuild().unwrap();
        assert_eq!(live, rebuilt);
    }
}
