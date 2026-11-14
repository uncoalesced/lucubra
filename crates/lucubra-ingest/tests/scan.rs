// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Scan integration tests (GATES M2-1, M2-9).

use std::fs;
use std::path::Path;

use lucubra_core::Workspace;
use lucubra_core::state::col;
use lucubra_ingest::{Format, scan};

fn write(root: &Path, rel: &str, body: &[u8]) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

fn seen_events(ws: &Workspace) -> usize {
    ws.replay()
        .unwrap()
        .ordered
        .iter()
        .filter(|e| e.kind == "file.seen")
        .count()
}

#[test]
fn discover_scan_logs_new_files_once_and_skips_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let ws = Workspace::init(root.path(), data.path()).unwrap();
    write(root.path(), "inbox/a.pdf", b"%PDF-1.7\nhello");
    write(root.path(), "inbox/b.md", b"# Unit 1\n");

    let first = scan(&ws).unwrap();
    assert_eq!(first.logged(), 2);
    assert_eq!(first.unchanged, 0);
    let formats: Vec<Format> = first.hashed.iter().map(|s| s.format).collect();
    assert_eq!(formats, [Format::Pdf, Format::Md]);

    let second = scan(&ws).unwrap();
    assert_eq!(second.logged(), 0);
    assert_eq!(second.unchanged, 2);
    assert!(second.hashed.is_empty());
    assert_eq!(seen_events(&ws), 2);
}

#[test]
fn discover_rename_keeps_hash_and_edit_logs_new_hash() {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let ws = Workspace::init(root.path(), data.path()).unwrap();
    write(root.path(), "inbox/a.txt", b"photosynthesis");
    scan(&ws).unwrap();
    let hash = ws
        .replay()
        .unwrap()
        .state
        .get(col::PATHS, "inbox/a.txt")
        .unwrap()["hash"]
        .as_str()
        .unwrap()
        .to_owned();

    fs::create_dir_all(root.path().join("subjects/bio/cells/sources")).unwrap();
    fs::rename(
        root.path().join("inbox/a.txt"),
        root.path().join("subjects/bio/cells/sources/a.txt"),
    )
    .unwrap();
    let moved = scan(&ws).unwrap();
    assert_eq!(moved.logged(), 1);
    let state = ws.replay().unwrap().state;
    assert_eq!(
        state
            .get(col::PATHS, "subjects/bio/cells/sources/a.txt")
            .unwrap()["hash"],
        hash.as_str()
    );
    // One file, two locations: identity is the content hash.
    assert_eq!(state.collection(col::FILES).count(), 1);

    write(
        root.path(),
        "subjects/bio/cells/sources/a.txt",
        b"photosynthesis, edited",
    );
    let edited = scan(&ws).unwrap();
    assert_eq!(edited.logged(), 1);
    assert_ne!(edited.hashed[0].hash, hash);
}

#[test]
fn discover_second_device_does_not_relog_synced_files() {
    let root = tempfile::tempdir().unwrap();
    let data_a = tempfile::tempdir().unwrap();
    let data_b = tempfile::tempdir().unwrap();
    let a = Workspace::init(root.path(), data_a.path()).unwrap();
    write(root.path(), "inbox/a.csv", b"x,y\n1,2\n");
    assert_eq!(scan(&a).unwrap().logged(), 1);

    let b = Workspace::open(root.path(), data_b.path()).unwrap();
    let first_b = scan(&b).unwrap();
    assert_eq!(
        first_b.hashed.len(),
        1,
        "device B hashes once to fill its stat cache"
    );
    assert_eq!(first_b.logged(), 0);
    let second_b = scan(&b).unwrap();
    assert_eq!(second_b.unchanged, 1);
    assert_eq!(seen_events(&b), 1);
}

#[cfg(windows)]
#[test]
fn online_only_files_are_reported_and_not_logged() {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let ws = Workspace::init(root.path(), data.path()).unwrap();
    write(root.path(), "inbox/cloud.pdf", b"%PDF-1.7 cloud copy");
    write(root.path(), "inbox/local.pdf", b"%PDF-1.7 local");
    let status = std::process::Command::new("attrib")
        .arg("+O")
        .arg(root.path().join("inbox").join("cloud.pdf"))
        .status()
        .unwrap();
    assert!(status.success());
    let report = scan(&ws).unwrap();
    assert_eq!(report.online_only, [("inbox/cloud.pdf".to_owned(), 19)]);
    assert_eq!(report.online_only_bytes(), 19);
    assert_eq!(report.logged(), 1);
    assert!(
        ws.replay()
            .unwrap()
            .state
            .get(col::PATHS, "inbox/cloud.pdf")
            .is_none()
    );
}
