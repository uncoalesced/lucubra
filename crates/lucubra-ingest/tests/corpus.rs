// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Manifest-driven corpus tests (GATES M2-2, M2-4, M2-5, M2-7, M2-10, M2-11, M2-13).
//!
//! Reads `LUCUBRA_FIXTURES_DIR` (or `tests/fixtures` relative to the repo root).
//! Skips cleanly when `MANIFEST.json` is not present yet.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use lucubra_core::Workspace;
use lucubra_core::state::col;
use lucubra_ingest::limits::RENDER_LONG_EDGE;
use lucubra_ingest::pipeline::{Outcome, extract_pending, open_index};
use lucubra_ingest::route::route_file;
use lucubra_ingest::scan;
use lucubra_ingest::vision::render_dir;
use lucubra_ingest::worker::{Supervisor, WorkerSpec};
use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

fn fixtures_dir() -> PathBuf {
    if let Some(env) = std::env::var_os("LUCUBRA_FIXTURES_DIR") {
        return PathBuf::from(env);
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn venv_python() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    [
        root.join(".venv/Scripts/python.exe"),
        root.join(".venv/bin/python3"),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

fn copy_all_files(src: &Path, dst: &Path) {
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = entry.file_name();
        let target = dst.join(&name);
        if path.is_dir() {
            copy_all_files(&path, &target);
        } else if path.is_file() {
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::copy(&path, &target).unwrap();
        }
    }
}

fn nfc(s: &str) -> String {
    s.nfc().collect()
}

#[test]
fn corpus_matches_manifest_routing_outcomes_and_search() {
    let dir = fixtures_dir();
    let manifest_path = dir.join("MANIFEST.json");
    if !manifest_path.is_file() {
        eprintln!(
            "skipped: {} not present (set LUCUBRA_FIXTURES_DIR or merge feat/m2-fixtures)",
            manifest_path.display()
        );
        return;
    }
    let Some(python) = venv_python() else {
        eprintln!("skipped: no .venv; run `uv sync --group dev` first");
        return;
    };

    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    let files = manifest["files"].as_array().unwrap();
    assert!(!files.is_empty());

    // M2-2: every manifest entry routes to its expected format identifier.
    for entry in files {
        let rel = entry["path"].as_str().unwrap();
        let expected_fmt = entry["format"].as_str().unwrap();
        let detected = route_file(&dir.join(rel)).unwrap();
        assert_eq!(
            detected.as_str(),
            expected_fmt,
            "routing mismatch for {rel}"
        );
    }

    // Copy non-JSON-config files (exclude classify/profile.json if non-source, or let it extract as code/json)
    // into a fresh workspace and run scan + extract_pending in one queue (M2-4, M2-5, M2-7, M2-10, M2-11, M2-13).
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let ws = Workspace::init(root.path(), data.path()).unwrap();
    let inbox = root.path().join("inbox");
    copy_all_files(&dir, &inbox);
    for meta_file in ["MANIFEST.json", "README.md", ".gitattributes"] {
        let p = inbox.join(meta_file);
        if p.is_file() {
            fs::remove_file(p).unwrap();
        }
    }

    let scanned = scan(&ws).unwrap();
    assert!(scanned.logged() >= files.len());
    let mut sup = Supervisor::new(WorkerSpec::python(&python), Duration::from_secs(120));
    let report = extract_pending(&ws, Some(&mut sup)).unwrap();
    assert!(report.missing.is_empty());

    let state = ws.replay().unwrap().state;
    let index = open_index(&ws).unwrap();

    let mut path_to_hash: BTreeMap<String, String> = BTreeMap::new();
    for (p, v) in state.collection(col::PATHS) {
        if let Some(h) = v.get("hash").and_then(Value::as_str) {
            path_to_hash.insert(p.clone(), h.to_owned());
        }
    }

    let mut classify_count = 0usize;
    let mut classify_with_title_or_text = 0usize;

    for entry in files {
        let rel = entry["path"].as_str().unwrap();
        if rel == "classify/profile.json" {
            continue;
        }
        let ws_rel = format!("inbox/{rel}");
        let hash = path_to_hash
            .get(&ws_rel)
            .unwrap_or_else(|| panic!("missing scanned path {ws_rel}"));
        let file_state = state
            .get(col::FILES, hash)
            .unwrap_or_else(|| panic!("missing file state for {ws_rel}"));
        let expect = &entry["expect"];
        let expected_outcome = expect["outcome"].as_str().unwrap();

        assert_eq!(
            file_state["status"].as_str().unwrap(),
            expected_outcome,
            "outcome mismatch for {rel}: {file_state}"
        );

        if matches!(expected_outcome, "failed" | "pending") {
            let expected_code = expect["code"].as_str().unwrap();
            assert_eq!(
                file_state["code"].as_str().unwrap(),
                expected_code,
                "failure code mismatch for {rel}"
            );
            if let Some(tool) = expect.get("tool").and_then(Value::as_str) {
                assert_eq!(file_state["tool"].as_str().unwrap(), tool);
            }
            continue;
        }

        // Gather all stored chunks for this file by reading chunks table through Index::chunk or search:
        let proc_item = report.processed.iter().find(|p| p.hash == *hash);
        if let Some(item) = proc_item {
            assert!(
                matches!(item.outcome, Outcome::Extracted { .. }),
                "expected extracted outcome for {rel}, got {:?}",
                item.outcome
            );
        }

        if let Some(expected_vision) = expect.get("needs_vision").and_then(Value::as_array) {
            let actual_vision: Vec<u64> = file_state["needs_vision"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_u64).collect())
                .unwrap_or_default();
            let want_vision: Vec<u64> = expected_vision.iter().filter_map(Value::as_u64).collect();
            assert_eq!(
                actual_vision, want_vision,
                "needs_vision mismatch for {rel}"
            );
            if entry["format"].as_str() == Some("pdf") {
                let rdir = render_dir(&ws, hash).unwrap();
                for page in &want_vision {
                    let png = rdir.join(format!("p{page}.png"));
                    assert!(png.is_file(), "missing rendered PNG for {rel} p{page}");
                    let bytes = fs::read(&png).unwrap();
                    let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
                    let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
                    assert!(
                        w.max(h) <= RENDER_LONG_EDGE,
                        "{rel} p{page} exceeds {RENDER_LONG_EDGE}px: {w}x{h}"
                    );
                }
            }
        }

        // Pull all indexed chunks for this hash directly via a helper scan using chunk locators,
        // or inspect chunks matching this hash.
        let chunk_texts = collect_file_text(&ws, hash);
        let joined_nfc = nfc(&chunk_texts.join("\n"));

        if let Some(contains) = expect.get("contains").and_then(Value::as_array) {
            for needle in contains {
                let needle_nfc = nfc(needle.as_str().unwrap());
                assert!(
                    joined_nfc.contains(&needle_nfc),
                    "{rel}: extracted text missing expected string {:?}",
                    needle_nfc
                );
            }
        }

        if let Some(notes) = expect.get("notes_contains").and_then(Value::as_array) {
            for needle in notes {
                let needle_nfc = nfc(needle.as_str().unwrap());
                assert!(
                    joined_nfc.contains(&needle_nfc),
                    "{rel}: missing speaker note {:?}",
                    needle_nfc
                );
            }
        }

        if let Some(hints) = expect.get("units_hint").and_then(Value::as_array) {
            let actual_hints: BTreeSet<&str> = file_state["units_hint"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            for h in hints {
                assert!(
                    actual_hints.contains(h.as_str().unwrap()),
                    "{rel}: missing units_hint {:?}",
                    h
                );
            }
        }

        if let Some(words) = expect.get("search_words").and_then(Value::as_array) {
            for word in words {
                let w = word.as_str().unwrap();
                let word_hits = index.search(w, 20).unwrap();
                assert!(
                    word_hits.iter().any(|hit| hit.hash == *hash),
                    "{rel}: word search failed for {w:?}"
                );
                if w.chars().count() >= 3 {
                    let sub_hits = index.search_substring(w, 20).unwrap();
                    assert!(
                        sub_hits.iter().any(|hit| hit.hash == *hash),
                        "{rel}: trigram substring search failed for {w:?}"
                    );
                }
            }
        }

        if let Some(sheets) = expect.get("sheets").and_then(Value::as_array) {
            for sheet in sheets {
                let sname = sheet["name"].as_str().unwrap();
                let sum = index
                    .chunk(hash, &format!("x:{sname}!summary"))
                    .unwrap()
                    .unwrap_or_else(|| panic!("{rel}: missing summary chunk for {sname}"));
                if let Some(nr) = sheet.get("named_ranges").and_then(Value::as_array) {
                    for name in nr {
                        assert!(
                            sum.contains(name.as_str().unwrap()),
                            "{rel}: summary missing named range {name:?}: {sum}"
                        );
                    }
                }
                if let Some(formulas) = sheet.get("formulas").and_then(Value::as_object) {
                    for (cell_ref, formula) in formulas {
                        let f = formula.as_str().unwrap().trim_start_matches('=');
                        assert!(
                            joined_nfc.contains(cell_ref) && joined_nfc.contains(f),
                            "{rel}: sheet {sname} missing formula {cell_ref}={f}"
                        );
                    }
                }
                if let Some(values) = sheet.get("values").and_then(Value::as_object) {
                    for (cell_ref, val) in values {
                        let v_str = match val {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        assert!(
                            joined_nfc.contains(cell_ref) && joined_nfc.contains(&v_str),
                            "{rel}: sheet {sname} missing value {cell_ref}={v_str}"
                        );
                    }
                }
            }
        }

        if rel.starts_with("classify/") && rel.ends_with(".pdf") {
            classify_count += 1;
            let has_title = file_state
                .get("title")
                .and_then(Value::as_str)
                .is_some_and(|s| !s.is_empty());
            if has_title || !chunk_texts.is_empty() {
                classify_with_title_or_text += 1;
            }
        }
    }

    if classify_count > 0 {
        assert_eq!(classify_count, 40, "expected 40 classify PDFs");
        assert_eq!(classify_with_title_or_text, 40);
    }

    // M2-10: verify NFD vs NFC and chillu vs ZWJ cross-matching if present in corpus.
    let nfd_hits = index.search("അവൻ", 20).unwrap();
    if !nfd_hits.is_empty() {
        let atomic_hits = index.search("അവൻ", 20).unwrap();
        let zwj_hits = index.search("അവന്\u{200D}", 20).unwrap();
        let atomic_hashes: BTreeSet<&str> = atomic_hits.iter().map(|h| h.hash.as_str()).collect();
        let zwj_hashes: BTreeSet<&str> = zwj_hits.iter().map(|h| h.hash.as_str()).collect();
        assert_eq!(atomic_hashes, zwj_hashes);
    }
}

fn collect_file_text(ws: &Workspace, hash: &str) -> Vec<String> {
    let db = ws
        .cache_dir()
        .unwrap()
        .join(lucubra_ingest::index::INDEX_FILE);
    let conn = rusqlite::Connection::open(db).unwrap();
    let mut stmt = conn
        .prepare("SELECT text FROM chunks WHERE hash = ?1 ORDER BY ord")
        .unwrap();
    stmt.query_map([hash], |r| r.get::<_, String>(0))
        .unwrap()
        .map(std::result::Result::unwrap)
        .collect()
}
