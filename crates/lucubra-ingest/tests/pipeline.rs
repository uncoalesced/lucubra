// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Extraction queue tests (GATES M2-8, M2-11 native part).

use std::fs;
use std::path::Path;

use lucubra_core::Workspace;
use lucubra_core::state::col;
use lucubra_ingest::pipeline::{ExtractReport, Outcome, extract_pending, open_index};
use lucubra_ingest::scan;

fn write(root: &Path, rel: &str, body: &[u8]) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

fn outcome_of<'a>(report: &'a ExtractReport, path: &str) -> &'a Outcome {
    &report
        .processed
        .iter()
        .find(|p| p.path == path)
        .unwrap_or_else(|| panic!("{path} not processed"))
        .outcome
}

#[test]
fn pipeline_extracts_native_and_logs_failures_once() {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let ws = Workspace::init(root.path(), data.path()).unwrap();
    write(
        root.path(),
        "inbox/cells.md",
        b"# Unit 1 Cells\nChloroplasts hold chlorophyll.\n",
    );
    write(
        root.path(),
        "inbox/marks.csv",
        b"name,mark\nasha,71\nravi,64\n",
    );
    write(root.path(), "inbox/drawing.dwg", b"AC1032\0\0\0\0");
    write(root.path(), "inbox/model.skp", b"\xFF\xFE\xFF\x0ES\0k\0");
    write(root.path(), "inbox/notes.pdf", b"%PDF-1.7\n");
    write(root.path(), "inbox/blob.zzz", b"\x00\x01");
    scan(&ws).unwrap();

    let first = extract_pending(&ws, None).unwrap();
    assert!(matches!(
        outcome_of(&first, "inbox/cells.md"),
        Outcome::Extracted { logged: true, .. }
    ));
    assert!(matches!(
        outcome_of(&first, "inbox/marks.csv"),
        Outcome::Extracted { logged: true, .. }
    ));
    assert_eq!(
        outcome_of(&first, "inbox/drawing.dwg"),
        &Outcome::Failed {
            code: "tool_missing".into(),
            outcome: "pending"
        }
    );
    assert_eq!(
        outcome_of(&first, "inbox/model.skp"),
        &Outcome::Failed {
            code: "unsupported".into(),
            outcome: "failed"
        }
    );
    assert_eq!(outcome_of(&first, "inbox/notes.pdf"), &Outcome::NoWorker);
    assert!(!first.processed.iter().any(|p| p.path == "inbox/blob.zzz"));

    let state = ws.replay().unwrap().state;
    let hash_of = |path: &str| {
        first
            .processed
            .iter()
            .find(|p| p.path == path)
            .unwrap()
            .hash
            .clone()
    };
    let dwg = state
        .get(col::FILES, &hash_of("inbox/drawing.dwg"))
        .unwrap();
    assert_eq!(dwg["status"], "pending");
    assert_eq!(dwg["tool"], "oda");
    let md_hash = hash_of("inbox/cells.md");
    assert_eq!(
        state.get(col::FILES, &md_hash).unwrap()["units_hint"][0],
        "Unit 1 Cells"
    );

    let hits = open_index(&ws).unwrap().search("chlorophyll", 5).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].hash, md_hash);

    // A second run touches nothing that is done or held.
    let second = extract_pending(&ws, None).unwrap();
    assert_eq!(second.up_to_date, 2);
    assert_eq!(second.held, 2);
    assert_eq!(second.processed.len(), 1, "only the PDF waits for a worker");
    let events = ws.replay().unwrap().ordered;
    let count = |kind: &str| events.iter().filter(|e| e.kind == kind).count();
    assert_eq!(count("file.extracted"), 2);
    assert_eq!(count("file.failed"), 2);
}

#[test]
fn pipeline_second_device_reextracts_without_logging() {
    let root = tempfile::tempdir().unwrap();
    let data_a = tempfile::tempdir().unwrap();
    let data_b = tempfile::tempdir().unwrap();
    let a = Workspace::init(root.path(), data_a.path()).unwrap();
    write(root.path(), "inbox/a.txt", b"Osmosis moves water.");
    scan(&a).unwrap();
    extract_pending(&a, None).unwrap();

    let b = Workspace::open(root.path(), data_b.path()).unwrap();
    let report = extract_pending(&b, None).unwrap();
    assert_eq!(
        report.processed[0].outcome,
        Outcome::Extracted {
            chunks: 1,
            logged: false
        }
    );
    assert_eq!(
        open_index(&b).unwrap().search("osmosis", 5).unwrap().len(),
        1
    );
}

#[test]
fn pipeline_reports_files_missing_from_disk() {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let ws = Workspace::init(root.path(), data.path()).unwrap();
    write(root.path(), "inbox/gone.md", b"# Gone\n");
    scan(&ws).unwrap();
    fs::remove_file(root.path().join("inbox/gone.md")).unwrap();
    let report = extract_pending(&ws, None).unwrap();
    assert_eq!(report.missing, ["inbox/gone.md"]);
    assert!(report.processed.is_empty());
}
