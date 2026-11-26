// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Pack crate suite on a real workspace: build and write a pack, then read
//! handoff blocks back from pasted text and from `inbox/` (S15, S16).

use lucubra_core::Workspace;
use lucubra_pack::build::{self, Options};
use lucubra_pack::ingest::{Outcome, ingest_inbox, ingest_text};
use serde_json::json;

const BLOCK: &str = "Session done.\n\n```lucubra-handoff v1\nsession: pack-suite-1\nhost: chatgpt-web\nsteps:\n  - unit: physics/optics, minutes: 20, activity: recall\ncheckin: sleep=7, quality=4, energy=3\nsummary: \"Optics recall.\"\nend: lucubra-handoff\n```\n";

fn workspace() -> (tempfile::TempDir, tempfile::TempDir, Workspace) {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let ws = Workspace::init(root.path(), data.path()).unwrap();
    ws.append("profile.set", json!({"programme": "B.Sc Physics"}))
        .unwrap();
    for unit in ["optics", "waves", "heat"] {
        ws.append(
            "unit.created",
            json!({"subject": "physics", "unit": unit, "title": unit,
                   "summary": format!("Key ideas of {unit}.")}),
        )
        .unwrap();
    }
    (root, data, ws)
}

#[test]
fn s15_pack_is_written_within_four_files_and_budget() {
    let (_r, _d, ws) = workspace();
    let state = ws.replay().unwrap().state;
    let pack = build::build(
        &state,
        Options {
            today: 20_000,
            budget: 6_000,
        },
    );
    assert!(pack.files.len() <= 4, "{:?}", pack.files.keys());
    assert!(pack.tokens() <= 6_000, "{} tokens", pack.tokens());
    assert!(pack.files.contains_key("SKILL.md"));
    let fingerprint = build::write(&ws, &pack).unwrap();
    assert_eq!(fingerprint, pack.fingerprint());
    for name in pack.files.keys() {
        assert!(ws.root().join("packs").join(name).is_file(), "{name}");
    }
    let state = ws.replay().unwrap().state;
    assert_eq!(
        build::last_fingerprint(&state).as_deref(),
        Some(fingerprint.as_str())
    );
}

#[test]
fn s15_pasted_block_is_ingested_once() {
    let (_r, _d, ws) = workspace();
    let first = ingest_text(&ws, BLOCK, "paste").unwrap();
    match first.as_slice() {
        [Outcome::Ingested(report)] => {
            assert_eq!(report.session, "pack-suite-1");
            assert_eq!(report.steps, 1);
        }
        other => panic!("{other:?}"),
    }
    let events = ws.replay().unwrap().ordered.len();
    let again = ingest_text(&ws, BLOCK, "paste").unwrap();
    assert!(matches!(again.as_slice(), [Outcome::Duplicate(s)] if s == "pack-suite-1"));
    assert_eq!(
        ws.replay().unwrap().ordered.len(),
        events,
        "duplicate wrote events"
    );
}

#[test]
fn s16_truncated_block_is_refused_and_writes_nothing() {
    let (_r, _d, ws) = workspace();
    let events = ws.replay().unwrap().ordered.len();
    let cut = BLOCK.replace("end: lucubra-handoff\n", "");
    let out = ingest_text(&ws, &cut, "paste").unwrap();
    match out.as_slice() {
        [Outcome::Failed(why)] => assert!(why.contains("cut off"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(ws.replay().unwrap().ordered.len(), events);
}

#[test]
fn s15_inbox_file_is_read_and_resaving_is_ignored() {
    let (_r, _d, ws) = workspace();
    std::fs::write(ws.root().join("inbox/handoff.md"), BLOCK).unwrap();
    let first = ingest_inbox(&ws).unwrap();
    assert!(
        first
            .iter()
            .any(|(_, o)| matches!(o.as_slice(), [Outcome::Ingested(_)])),
        "{first:?}"
    );
    std::fs::write(ws.root().join("inbox/handoff-copy.md"), BLOCK).unwrap();
    let second = ingest_inbox(&ws).unwrap();
    assert!(
        second
            .iter()
            .flat_map(|(_, o)| o)
            .all(|o| !matches!(o, Outcome::Ingested(_))),
        "{second:?}"
    );
}
