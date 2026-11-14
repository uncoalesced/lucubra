// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! End-to-end extraction through the real Python worker (GATES M2-3, M2-5).
//! Uses the repository's `.venv` (created by `uv sync`); skipped when absent.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use lucubra_core::Workspace;
use lucubra_core::state::col;
use lucubra_ingest::pipeline::{Outcome, extract_pending, open_index};
use lucubra_ingest::scan;
use lucubra_ingest::worker::{Supervisor, WorkerSpec};

fn venv_python() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let candidates = [
        root.join(".venv/Scripts/python.exe"),
        root.join(".venv/bin/python3"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

/// A minimal one-font PDF with one text line per page.
fn make_pdf(pages: &[&str]) -> Vec<u8> {
    let n = pages.len();
    let mut objects: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        format!(
            "<< /Type /Pages /Kids [{}] /Count {n} >>",
            (0..n)
                .map(|i| format!("{} 0 R", 4 + 2 * i))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ];
    for (i, text) in pages.iter().enumerate() {
        let stream = if text.is_empty() {
            String::new()
        } else {
            format!("BT /F1 12 Tf 72 720 Td ({text}) Tj ET")
        };
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
             /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
            5 + 2 * i
        ));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{stream}\nendstream",
            stream.len()
        ));
    }
    let mut out = String::from("%PDF-1.7\n");
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{body}\nendobj\n", i + 1));
    }
    let xref = out.len();
    out.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for offset in offsets {
        out.push_str(&format!("{offset:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    ));
    out.into_bytes()
}

#[test]
fn python_worker_extracts_pdf_and_queue_continues_past_bad_file() {
    let Some(python) = venv_python() else {
        eprintln!("skipped: no .venv; run `uv sync --group dev` first");
        return;
    };
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let ws = Workspace::init(root.path(), data.path()).unwrap();
    let inbox = root.path().join("inbox");
    fs::create_dir_all(&inbox).unwrap();
    fs::write(inbox.join("a-bad.pdf"), b"%PDF-1.7\nnot really a pdf").unwrap();
    fs::write(
        inbox.join("b-leaf.pdf"),
        make_pdf(&["Stomata open to let carbon dioxide into the leaf.", ""]),
    )
    .unwrap();
    scan(&ws).unwrap();

    let mut sup = Supervisor::new(WorkerSpec::python(&python), Duration::from_secs(60));
    let report = extract_pending(&ws, Some(&mut sup)).unwrap();
    let outcome = |path: &str| {
        report
            .processed
            .iter()
            .find(|p| p.path == path)
            .map(|p| p.outcome.clone())
            .unwrap()
    };
    assert_eq!(
        outcome("inbox/a-bad.pdf"),
        Outcome::Failed {
            code: "corrupt".into(),
            outcome: "failed"
        }
    );
    assert_eq!(
        outcome("inbox/b-leaf.pdf"),
        Outcome::Extracted {
            chunks: 1,
            logged: true
        }
    );
    assert_eq!(sup.starts, 1, "an error reply does not restart the worker");

    let hits = open_index(&ws).unwrap().search("stomata", 5).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].locator, "p:1");
    let file = ws
        .replay()
        .unwrap()
        .state
        .get(col::FILES, &hits[0].hash)
        .cloned()
        .unwrap();
    assert_eq!(file["needs_vision"], serde_json::json!([2]));
    assert!(
        file["extractor"]
            .as_str()
            .unwrap()
            .starts_with("pypdfium2@")
    );
}
