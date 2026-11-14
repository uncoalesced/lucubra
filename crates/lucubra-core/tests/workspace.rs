// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Workspace gates: S18, S19, S31, S42, S45, torn lines and conflict copies.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use lucubra_core::Workspace;
use lucubra_core::projection::USAGE_HEADING;
use serde_json::json;

struct Fixture {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    data_a: PathBuf,
    data_b: PathBuf,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("ws");
    let data_a = tmp.path().join("data-a");
    let data_b = tmp.path().join("data-b");
    Fixture {
        root,
        data_a,
        data_b,
        _tmp: tmp,
    }
}

fn seed(ws: &Workspace) {
    ws.append(
        "profile.set",
        json!({"level": "Undergraduate", "programme": "B.Com"}),
    )
    .unwrap();
    ws.append(
        "unit.created",
        json!({"subject": "accounts", "subject_title": "Financial Accounting", "unit": "ledgers",
               "title": "Ledgers", "summary": "Posting journal entries to ledger accounts.",
               "concepts": ["double entry", "balancing"],
               "sources": [{"file": "sources/ch2.pdf", "pages": "12-30"}]}),
    )
    .unwrap();
    ws.append(
        "exam.set",
        json!({"id": "acc-final", "subject": "accounts", "title": "Final", "date": "2026-12-04", "type": "written"}),
    )
    .unwrap();
}

/// BLAKE3 hash of every file under `dir`, keyed by relative path.
fn tree(dir: &Path) -> BTreeMap<String, String> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let rel = path
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                let hash = blake3::hash(&fs::read(&path).unwrap()).to_hex().to_string();
                out.insert(rel, hash);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

fn unit_card(root: &Path) -> PathBuf {
    root.join("subjects/accounts/ledgers/UNIT.md")
}

#[test]
fn s19_rebuild_gives_identical_hash() {
    let f = fixture();
    let ws = Workspace::init(&f.root, &f.data_a).unwrap();
    seed(&ws);
    let (_, first) = ws.refresh().unwrap();
    let cache = ws.cache_dir().unwrap();
    assert!(cache.join("cache.sqlite").is_file());
    fs::remove_dir_all(&cache).unwrap();
    assert!(!cache.exists());
    let (_, second) = ws.rebuild().unwrap();
    assert_eq!(first, second);
    assert_eq!(first, ws.status().unwrap().state_hash);
}

#[test]
fn s18_two_devices_concurrently_share_one_workspace() {
    let f = fixture();
    Workspace::init(&f.root, &f.data_a).unwrap();
    let a = Workspace::open(&f.root, &f.data_a).unwrap();
    let b = Workspace::open(&f.root, &f.data_b).unwrap();
    assert_ne!(a.device(), b.device());

    std::thread::scope(|scope| {
        for ws in [&a, &b] {
            scope.spawn(move || {
                for i in 0..40 {
                    ws.append(
                        "setting.changed",
                        json!({"key": format!("{}-{i}", ws.device()), "value": i}),
                    )
                    .unwrap();
                }
            });
        }
    });

    let logs: Vec<String> = fs::read_dir(f.root.join(".lucubra/events"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(logs.len(), 2, "{logs:?}");
    assert!(logs.contains(&format!("{}.jsonl", a.device())));
    assert!(logs.contains(&format!("{}.jsonl", b.device())));

    let (_, hash_a) = a.rebuild().unwrap();
    let (_, hash_b) = b.rebuild().unwrap();
    assert_eq!(hash_a, hash_b);
    assert_eq!(a.status().unwrap().events, 81);
    assert!(a.status().unwrap().bad_lines.is_empty());
}

/// Helper run in a child process by the test below.
#[test]
#[ignore = "child process helper"]
fn s18_child_appender() {
    let (Some(root), Some(data)) = (
        std::env::var_os("LUCUBRA_TEST_ROOT"),
        std::env::var_os("LUCUBRA_TEST_DATA"),
    ) else {
        return;
    };
    let ws = Workspace::open(Path::new(&root), Path::new(&data)).unwrap();
    for i in 0..60 {
        ws.append(
            "setting.changed",
            json!({"key": format!("p{i}"), "value": i}),
        )
        .unwrap();
    }
}

#[test]
fn s18_two_processes_on_one_device_serialise_appends() {
    let f = fixture();
    Workspace::init(&f.root, &f.data_a).unwrap();
    let exe = std::env::current_exe().unwrap();
    let children: Vec<_> = (0..2)
        .map(|_| {
            Command::new(&exe)
                .args(["s18_child_appender", "--exact", "--ignored", "--nocapture"])
                .env("LUCUBRA_TEST_ROOT", &f.root)
                .env("LUCUBRA_TEST_DATA", &f.data_a)
                .spawn()
                .unwrap()
        })
        .collect();
    for mut child in children {
        assert!(child.wait().unwrap().success());
    }
    let ws = Workspace::open(&f.root, &f.data_a).unwrap();
    let status = ws.status().unwrap();
    assert_eq!(status.events, 121);
    assert!(status.bad_lines.is_empty());
    let mut seqs: Vec<u64> = ws.replay().unwrap().ordered.iter().map(|e| e.seq).collect();
    seqs.sort_unstable();
    assert_eq!(seqs, (1..=121).collect::<Vec<_>>());
}

#[test]
fn torn_line_is_skipped_and_log_stays_usable() {
    let f = fixture();
    let ws = Workspace::init(&f.root, &f.data_a).unwrap();
    seed(&ws);
    let log = f
        .root
        .join(format!(".lucubra/events/{}.jsonl", ws.device()));
    let mut bytes = fs::read(&log).unwrap();
    bytes.truncate(bytes.len() - 20);
    fs::write(&log, &bytes).unwrap();

    let status = ws.status().unwrap();
    assert_eq!(status.events, 3);
    assert_eq!(status.bad_lines.len(), 1);
    assert!(status.bad_lines[0].torn);

    ws.append("setting.changed", json!({"key": "after", "value": true}))
        .unwrap();
    let status = ws.status().unwrap();
    assert_eq!(status.events, 4);
    assert_eq!(status.bad_lines.len(), 1);
    assert!(!status.bad_lines[0].torn);
    let (_, hash) = ws.rebuild().unwrap();
    assert_eq!(hash, status.state_hash);
}

#[test]
fn s31_user_edit_is_preserved_before_regeneration() {
    let f = fixture();
    let ws = Workspace::init(&f.root, &f.data_a).unwrap();
    seed(&ws);
    ws.project().unwrap();
    let card = unit_card(&f.root);
    let generated = fs::read_to_string(&card).unwrap();

    // Device B sees the synced, unedited card: no false preserve.
    let b = Workspace::open(&f.root, &f.data_b).unwrap();
    let report = b.project().unwrap();
    assert!(report.preserved.is_empty(), "{:?}", report.preserved);
    assert!(report.written.is_empty());

    // The user edits the card; a state change forces regeneration.
    let edited = format!("{generated}\nMy own note about trial balances.\n");
    fs::write(&card, &edited).unwrap();
    ws.append(
        "unit.updated",
        json!({"subject": "accounts", "unit": "ledgers", "status": "learning"}),
    )
    .unwrap();
    let report = ws.project().unwrap();
    assert_eq!(report.preserved.len(), 1);
    let (path, to) = &report.preserved[0];
    assert_eq!(path, "subjects/accounts/ledgers/UNIT.md");
    assert!(to.starts_with(".lucubra/preserved/subjects/accounts/ledgers/UNIT.md."));
    assert_eq!(fs::read_to_string(f.root.join(to)).unwrap(), edited);
    assert!(!fs::read_to_string(&card).unwrap().contains("My own note"));

    let replayed = ws.replay().unwrap();
    let events: Vec<_> = replayed
        .ordered
        .iter()
        .filter(|e| e.kind == "projection.preserved")
        .collect();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data["path"], "subjects/accounts/ledgers/UNIT.md");

    // Device B now regenerates the file A wrote: still no false preserve.
    let report = b.project().unwrap();
    assert!(report.preserved.is_empty());
}

#[test]
fn conflict_copies_are_moved_and_user_conflicts_reported() {
    let f = fixture();
    let ws = Workspace::init(&f.root, &f.data_a).unwrap();
    seed(&ws);
    ws.project().unwrap();
    // Sync copies: one byte-identical to an engine-written file, one an older
    // rendering that still opens with the projection's heading.
    let dropbox = "AGENTS (Joel's conflicted copy 2026-11-14).md";
    fs::copy(f.root.join("AGENTS.md"), f.root.join(dropbox)).unwrap();
    fs::write(
        f.root.join("subjects/accounts/ledgers/UNIT-LAPTOP.md"),
        "# Ledgers\n\nAn older rendering.\n",
    )
    .unwrap();
    // User files whose names only look like conflict copies stay put.
    fs::write(f.root.join("AGENTS-old.md"), "# My own agent notes\n").unwrap();
    let summary = f.root.join("subjects/accounts/ledgers/UNIT-summary.md");
    fs::write(&summary, "My summary of the unit.\n").unwrap();
    let notes = f.root.join("subjects/accounts/ledgers/notes");
    fs::create_dir_all(&notes).unwrap();
    fs::write(notes.join("lecture (1).md"), "mine").unwrap();

    let report = ws.project().unwrap();
    let moved: Vec<&str> = report
        .conflicts_moved
        .iter()
        .map(|(from, _)| from.as_str())
        .collect();
    assert_eq!(moved, [dropbox, "subjects/accounts/ledgers/UNIT-LAPTOP.md"]);
    assert!(f.root.join(".lucubra/conflicts").join(dropbox).is_file());
    assert!(
        f.root
            .join(".lucubra/conflicts/subjects/accounts/ledgers/UNIT-LAPTOP.md")
            .is_file()
    );
    assert_eq!(
        report.user_conflicts,
        ["subjects/accounts/ledgers/notes/lecture (1).md"]
    );
    assert!(notes.join("lecture (1).md").is_file());
    assert!(f.root.join("AGENTS-old.md").is_file());
    assert!(summary.is_file());
}

#[test]
fn s42_reset_clears_derived_state_only() {
    let f = fixture();
    let ws = Workspace::init(&f.root, &f.data_a).unwrap();
    seed(&ws);
    ws.project().unwrap();
    let unit = f.root.join("subjects/accounts/ledgers");
    fs::create_dir_all(unit.join("sources")).unwrap();
    fs::write(unit.join("sources/ch2.pdf"), b"%PDF-1.7 fixture").unwrap();
    fs::create_dir_all(unit.join("notes")).unwrap();
    fs::write(unit.join("notes/mine.md"), "my notes").unwrap();
    // An edited subject index, which reset must preserve, not delete.
    let index = f.root.join("subjects/accounts/agents_accounts.md");
    let edited = format!("{}\nEdited by me.\n", fs::read_to_string(&index).unwrap());
    fs::write(&index, &edited).unwrap();
    ws.refresh().unwrap();
    let cache = ws.cache_dir().unwrap().join("cache.sqlite");
    assert!(cache.is_file());

    let kept = [
        unit.join("sources"),
        unit.join("notes"),
        f.root.join(".lucubra/events"),
    ];
    let before: Vec<_> = kept.iter().map(|d| tree(d)).collect();

    let (after_hash, report) = ws.reset().unwrap();
    assert!(cache.is_file(), "cache rebuilt");
    assert_eq!(after_hash, ws.status().unwrap().state_hash);
    assert_eq!(report.preserved.len(), 1);
    let preserved_copy = f.root.join(&report.preserved[0].1);
    assert_eq!(fs::read_to_string(&preserved_copy).unwrap(), edited);
    assert!(unit.join("UNIT.md").is_file());
    assert!(index.is_file());
    assert!(f.root.join("AGENTS.md").is_file());

    let after: Vec<_> = kept.iter().map(|d| tree(d)).collect();
    assert_eq!(before[0], after[0], "sources untouched");
    assert_eq!(before[1], after[1], "notes untouched");
    // The log only gained the projection.preserved record; nothing was removed.
    for path in before[2].keys() {
        assert!(after[2].contains_key(path));
    }

    // A second reset keeps the preserved copy and the events.
    let preserved_before = tree(&f.root.join(".lucubra/preserved"));
    let events_before = tree(&f.root.join(".lucubra/events"));
    ws.reset().unwrap();
    assert_eq!(tree(&f.root.join(".lucubra/preserved")), preserved_before);
    assert_eq!(tree(&f.root.join(".lucubra/events")), events_before);
}

#[test]
fn s45_root_index_carries_usage_and_stays_small() {
    let f = fixture();
    let ws = Workspace::init(&f.root, &f.data_a).unwrap();
    seed(&ws);
    ws.project().unwrap();
    let root = fs::read_to_string(f.root.join("AGENTS.md")).unwrap();
    assert!(root.contains(USAGE_HEADING));
    assert!(root.contains("lucubra_orient"));
    assert!(root.contains("PREFERENCES.md"));
    assert!(root.contains("[[subjects/accounts/agents_accounts|Financial Accounting]]"));
    let chars = root.chars().count();
    let estimated_tokens = chars / 4;
    println!("AGENTS.md: {chars} chars, ~{estimated_tokens} tokens");
    assert!(estimated_tokens < 1000);
}

#[test]
fn projections_carry_no_watermark_and_are_identical_across_devices() {
    let f = fixture();
    let a = Workspace::init(&f.root, &f.data_a).unwrap();
    seed(&a);
    a.project().unwrap();
    let first = tree(&f.root.join("subjects"));
    let b = Workspace::open(&f.root, &f.data_b).unwrap();
    b.project().unwrap();
    assert_eq!(tree(&f.root.join("subjects")), first);

    let provenance = ["Engineered", "by", "uncoalesced"].join(" ");
    let banner = ["Generated", "by", "Lucubra"].join(" ");
    for rel in [
        "AGENTS.md",
        "subjects/accounts/agents_accounts.md",
        "subjects/accounts/ledgers/UNIT.md",
    ] {
        let text = fs::read_to_string(f.root.join(rel)).unwrap();
        assert!(!text.contains(&provenance), "{rel}");
        assert!(!text.contains(&banner), "{rel}");
        assert!(!text.contains('\r'), "{rel}");
    }
}

#[test]
fn open_rejects_a_plain_folder() {
    let f = fixture();
    fs::create_dir_all(&f.root).unwrap();
    assert!(Workspace::open(&f.root, &f.data_a).is_err());
}
