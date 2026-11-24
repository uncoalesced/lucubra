// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! M7 gate tests: pack export (S15, S21), handoff ingest from `inbox/` and
//! paste with idempotence (S15), truncated blocks (S16) and dropped items
//! whose quote is not in the source (S17).

use std::fs;

use lucubra_core::Workspace;
use lucubra_core::state::col;
use lucubra_ingest::pipeline::open_index;
use lucubra_mcp::LucubraServer;
use rmcp::model::{
    CallToolResponse, ClientCapabilities, ContentBlock, Implementation, InitializeRequestParams,
};
use serde_json::{Value, json};

const NOTES: &str = "9a1c22e0000000000000000000000000000000000000000000000000000000aa";
const PAGE: &str = "The arithmetic mean is the sum of the values divided by their count. \
The median is the middle value once the data are sorted.";

type Fixture = (tempfile::TempDir, tempfile::TempDir, LucubraServer);

async fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let ws = Workspace::init(root.path(), data.path()).unwrap();
    ws.append(
        "file.seen",
        json!({"path": "subjects/stats/u3/notes.pdf", "hash": NOTES, "bytes": 64}),
    )
    .unwrap();
    ws.append(
        "file.classified",
        json!({"hash": NOTES, "subject": "stats", "unit": "u3"}),
    )
    .unwrap();
    ws.append(
        "unit.created",
        json!({"subject": "stats", "unit": "u3", "title": "Averages",
               "summary": "Mean, median and mode."}),
    )
    .unwrap();
    let mut index = open_index(&ws).unwrap();
    index.upsert_chunk(NOTES, "p:14", PAGE).unwrap();
    drop(index);
    fs::create_dir_all(root.path().join("inbox")).unwrap();
    let s = LucubraServer::from_workspace(ws, false);
    s.record_client_initialize(
        InitializeRequestParams::new(
            ClientCapabilities::default(),
            Implementation::new("claude-code", "1.0"),
        ),
        "2025-11-25",
    )
    .await
    .unwrap();
    (root, data, s)
}

async fn call(s: &LucubraServer, tool: &str, args: Value) -> (bool, Value) {
    let Value::Object(map) = args else {
        panic!("args must be an object")
    };
    let CallToolResponse::Complete(res) = s.handle_call_raw(tool, Some(map)).await.unwrap() else {
        panic!("expected complete response");
    };
    let ContentBlock::Text(text) = &res.content[0] else {
        panic!("expected text")
    };
    (
        res.is_error.unwrap_or(false),
        serde_json::from_str(&text.text).unwrap(),
    )
}

/// One practice item answered wrongly, so it has an FSRS card.
async fn item_with_card(s: &LucubraServer) -> String {
    let (err, env) = call(
        s,
        "lucubra_submit",
        json!({"action": "items", "unit": "stats/u3", "items": [{
            "type": "true_false", "prompt": "The median needs sorted data.", "answer": true,
            "sources": [{"file_hash": NOTES, "location": "p:14",
                         "anchor": "the middle value once the data are sorted"}]}]}),
    )
    .await;
    assert!(!err, "{env}");
    let id = env["data"]["created"][0]["item_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (_, ans) = call(
        s,
        "lucubra_practice",
        json!({"action": "answer", "item_id": id, "response": false}),
    )
    .await;
    assert_eq!(ans["data"]["correct"], false, "{ans}");
    id
}

fn block(session: &str, item: &str, anchor: &str) -> String {
    format!(
        r#"Great session. Save this:

```lucubra-handoff v1
session: {session}
host: chatgpt-web
started: 2026-11-24T19:05+05:30
ended: 2026-11-24T19:52+05:30
steps:
  - unit: stats/u3, minutes: 25, activity: recall
items:
  - id: {item}, response: "true", result: correct
cards:
  - id: c_{item}, rating: good, at: 2026-11-24T19:40+05:30
new_items:
  - type: cloze, unit: stats/u3, prompt: "The ____ is the middle value.", answer: "median", source: "S:stats/U:u3/F:9a1c22e0/p:14", anchor: "The median is the middle value"
  - type: cloze, unit: stats/u3, prompt: "The mean is the ____ value.", answer: "largest", source: "S:stats/U:u3/F:9a1c22e0/p:14", anchor: "{anchor}"
checkin: sleep=6.5, quality=3, energy=2
summary: "Averages reviewed."
end: lucubra-handoff
```
"#
    )
}

#[tokio::test]
async fn s15_s21_pack_export_within_limits() {
    let (root, _d, s) = fixture().await;
    item_with_card(&s).await;
    let (err, env) = call(&s, "lucubra_export", json!({"action": "pack"})).await;
    assert!(!err, "{env}");
    let files = env["data"]["files"].as_array().unwrap();
    assert!(files.len() <= 4, "{env}");
    let tokens = env["data"]["tokens"].as_u64().unwrap();
    assert!(tokens <= env["data"]["budget"].as_u64().unwrap());
    assert_eq!(env["data"]["changed_since_last_build"], true);
    let pack = fs::read_to_string(root.path().join("packs/PACK.md")).unwrap();
    assert!(pack.contains("S:stats/U:u3/F:9a1c22e0"), "{pack}");
    assert!(
        pack.contains("The median needs sorted data."),
        "due card listed"
    );
    let skill = fs::read_to_string(root.path().join("packs/SKILL.md")).unwrap();
    assert!(skill.starts_with("---\nname: lucubra-study-pack\n"));
    assert!(skill.contains("Gemini"));
    let (_, again) = call(&s, "lucubra_export", json!({"action": "pack"})).await;
    assert_eq!(again["data"]["changed_since_last_build"], false);
}

#[tokio::test]
async fn s15_s17_inbox_ingest_is_idempotent_and_drops_bad_quotes() {
    let (root, data, s) = fixture().await;
    let item = item_with_card(&s).await;
    let text = block("sess-001", &item, "The mean is the largest value of all");
    fs::write(root.path().join("inbox/handoff-1.md"), &text).unwrap();
    let (err, env) = call(
        &s,
        "lucubra_ingest",
        json!({"action": "scan", "extract": false}),
    )
    .await;
    assert!(!err, "{env}");
    let report = &env["data"]["handoffs"][0]["blocks"][0];
    assert_eq!(report["status"], "ingested", "{env}");
    assert_eq!(report["steps"], 1);
    assert_eq!(report["answers"], 1);
    assert_eq!(report["cards"], 1);
    assert_eq!(report["created"].as_array().unwrap().len(), 1);
    let dropped = &report["dropped"][0];
    assert_eq!(dropped["prompt"], "The mean is the ____ value.");
    assert!(
        dropped["reason"]
            .as_str()
            .unwrap()
            .contains("anchor not found"),
        "{dropped}"
    );

    // Saving the same block again changes nothing.
    let (_, again) = call(
        &s,
        "lucubra_ingest",
        json!({"action": "handoff", "text": text}),
    )
    .await;
    assert_eq!(
        again["data"]["blocks"][0]["status"], "already_ingested",
        "{again}"
    );

    drop(s);
    let ws = Workspace::open(root.path(), data.path()).unwrap();
    let state = ws.replay().unwrap().state;
    assert!(state.get(col::HANDOFFS, "sess-001").is_some());
    let session = state.get(col::SESSIONS, "sess-001").unwrap();
    assert_eq!(session["status"], "ended");
    assert_eq!(session["logged_minutes"], 25);
    let card = state.get(col::CARDS, &format!("c_{item}")).unwrap();
    assert_eq!(card["last_rating"], "good");
    assert_eq!(
        state.collection(col::ITEMS).count(),
        2,
        "one submitted, one from the pack"
    );
}

#[tokio::test]
async fn s16_truncated_block_is_not_ingested() {
    let (_r, _d, s) = fixture().await;
    let text = block("sess-002", "it_none", "x").replace("end: lucubra-handoff", "");
    let (err, env) = call(
        &s,
        "lucubra_ingest",
        json!({"action": "handoff", "text": text}),
    )
    .await;
    assert!(!err, "{env}");
    let b = &env["data"]["blocks"][0];
    assert_eq!(b["status"], "not_ingested");
    assert!(
        b["reason"]
            .as_str()
            .unwrap()
            .contains("copy the whole block again")
    );
    assert_eq!(env["next"], "copy_block_again");
    let (err, none) = call(
        &s,
        "lucubra_ingest",
        json!({"action": "handoff", "text": "just some chat text"}),
    )
    .await;
    assert!(err && none["code"] == "invalid_arguments", "{none}");
}
