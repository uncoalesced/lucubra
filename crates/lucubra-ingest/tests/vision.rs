// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Vision rendering and unverified transcript tests (GATES M2-5, M2-6).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use lucubra_core::Workspace;
use lucubra_core::state::col;
use lucubra_ingest::limits::RENDER_LONG_EDGE;
use lucubra_ingest::pipeline::{Outcome, extract_pending, open_index};
use lucubra_ingest::scan;
use lucubra_ingest::vision::{pending_vision_pages, render_dir, save_transcript};
use lucubra_ingest::worker::{Supervisor, WorkerSpec};

fn venv_python() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    [
        root.join(".venv/Scripts/python.exe"),
        root.join(".venv/bin/python3"),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

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

fn png_dimensions(path: &Path) -> (u32, u32) {
    let bytes = fs::read(path).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    (w, h)
}

#[test]
fn m2_5_vision_renders_once_and_transcript_clears_pending() {
    let Some(python) = venv_python() else {
        eprintln!("skipped: no .venv; run `uv sync --group dev` first");
        return;
    };
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let ws = Workspace::init(root.path(), data.path()).unwrap();
    fs::create_dir_all(root.path().join("inbox")).unwrap();
    fs::write(root.path().join("inbox/scan.pdf"), make_pdf(&["", ""])).unwrap();

    let scanned = scan(&ws).unwrap();
    let hash = scanned.hashed[0].hash.clone();
    let mut sup = Supervisor::new(WorkerSpec::python(&python), Duration::from_secs(60));
    let first = extract_pending(&ws, Some(&mut sup)).unwrap();
    assert_eq!(first.vision_rendered, 2);

    let rdir = render_dir(&ws, &hash).unwrap();
    for p in [1, 2] {
        let (w, h) = png_dimensions(&rdir.join(format!("p{p}.png")));
        assert!(w.max(h) <= RENDER_LONG_EDGE, "page {p}: {w}x{h}");
    }

    let t1 = save_transcript(&ws, &hash, Some(1), "Scanned cell biology notes.", false).unwrap();
    assert!(t1.ends_with("notes/scan.pdf.p1.transcript.md"));
    let t1_text = fs::read_to_string(&t1).unwrap();
    assert!(t1_text.contains("verified: false"));
    save_transcript(&ws, &hash, Some(2), "Mitosis stages overview.", false).unwrap();

    let hits = open_index(&ws).unwrap().search("mitosis", 5).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].locator, "p:2:transcript");

    let state = ws.replay().unwrap().state;
    assert!(pending_vision_pages(&state, &hash).is_empty());

    let second = extract_pending(&ws, Some(&mut sup)).unwrap();
    assert_eq!(second.vision_rendered, 0);
}

#[test]
fn m2_6_photo_transcript_saved_next_to_image_and_skipped_by_scan() {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let ws = Workspace::init(root.path(), data.path()).unwrap();
    fs::create_dir_all(root.path().join("inbox")).unwrap();
    // Minimal valid 1x1 PNG header bytes for routing.
    fs::write(
        root.path().join("inbox/photo-notes.png"),
        b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x02\x00\x00\x00\x90wS\xde",
    )
    .unwrap();

    let scanned = scan(&ws).unwrap();
    let hash = scanned.hashed[0].hash.clone();
    let report = extract_pending(&ws, None).unwrap();
    assert!(matches!(
        report.processed[0].outcome,
        Outcome::Extracted { logged: true, .. }
    ));
    let state = ws.replay().unwrap().state;
    assert_eq!(pending_vision_pages(&state, &hash), [1]);

    let saved = save_transcript(&ws, &hash, None, "Kirchhoff loop equation notes.", false).unwrap();
    assert_eq!(
        saved,
        root.path().join("inbox/photo-notes.png.transcript.md")
    );
    let content = fs::read_to_string(&saved).unwrap();
    assert!(content.contains("verified: false"));

    let state_after = ws.replay().unwrap().state;
    assert!(pending_vision_pages(&state_after, &hash).is_empty());
    assert_eq!(
        state_after
            .get(col::TRANSCRIPTS, &format!("{hash}/img"))
            .unwrap()["verified"],
        false
    );

    // Re-scanning must ignore the generated *.transcript.md file.
    let rescan = scan(&ws).unwrap();
    assert_eq!(rescan.logged(), 0);
    assert_eq!(rescan.unchanged, 1);
}
