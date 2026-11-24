// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! M6 gate tests: grounded items (S05, S06, S27), one retry then rejection,
//! rubric grading, exam simulation, tutor mode (S26, S40) and Anki export
//! (S32), driven through the MCP tool surface.

use std::fs;
use std::io::Read;

use lucubra_core::Workspace;
use lucubra_core::state::col;
use lucubra_ingest::pipeline::open_index;
use lucubra_mcp::LucubraServer;
use rmcp::model::{
    CallToolResponse, ClientCapabilities, ContentBlock, Implementation, InitializeRequestParams,
};
use serde_json::{Value, json};

const NOTES: &str = "aaaa000000000000000000000000000000000000000000000000000000000001";
const SHEET: &str = "aaaa000000000000000000000000000000000000000000000000000000000002";
const PHOTO: &str = "aaaa000000000000000000000000000000000000000000000000000000000003";
const TASK: &str = "aaaa000000000000000000000000000000000000000000000000000000000004";
const PLAN: &str = "aaaa000000000000000000000000000000000000000000000000000000000005";

const PAGE_A: &str = "The Calvin cycle fixes carbon dioxide onto ribulose bisphosphate. \
Each molecule is reduced using ATP and NADPH from the light reactions, and";
const PAGE_B: &str = "only 1 in 6 triose phosphates leaves the cycle as product. Rubisco \
does not distinguish perfectly between carbon dioxide and oxygen.";

type Fixture = (tempfile::TempDir, tempfile::TempDir, LucubraServer);

async fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let ws = Workspace::init(root.path(), data.path()).unwrap();
    let files = [
        (NOTES, "subjects/bio/u1/notes.md"),
        (SHEET, "subjects/bio/u1/costs.xlsx"),
        (PHOTO, "subjects/bio/u1/board.png"),
        (TASK, "inbox/assignment 3.pdf"),
        (PLAN, "subjects/bio/u1/lab.dxf"),
    ];
    for (hash, path) in files {
        let abs = root.path().join(path);
        fs::create_dir_all(abs.parent().unwrap()).unwrap();
        fs::write(&abs, hash).unwrap();
        ws.append(
            "file.seen",
            json!({"path": path, "hash": hash, "bytes": 64}),
        )
        .unwrap();
        ws.append("file.extracted", json!({"hash": hash})).unwrap();
    }
    ws.append(
        "unit.created",
        json!({"subject": "bio", "unit": "u1", "title": "Photosynthesis"}),
    )
    .unwrap();
    ws.append(
        "transcript.saved",
        json!({"hash": PHOTO, "verified": false}),
    )
    .unwrap();
    let mut index = open_index(&ws).unwrap();
    for (hash, loc, text) in [
        (NOTES, "t:0-150", PAGE_A),
        (NOTES, "t:150-300", PAGE_B),
        (
            SHEET,
            "x:Costs!A1:B4",
            "A1: Item | B1: Cost\nA4: Total | B4: 12.5 (=SUM(B1:B3))",
        ),
        (
            PHOTO,
            "img:transcript",
            "Light reactions split water and release oxygen gas.",
        ),
        (
            TASK,
            "p:1",
            "Assignment 3. Explain photorespiration in C3 plants.",
        ),
        (PLAN, "l:summary", "Layers: Walls, Doors"),
        (PLAN, "l:Walls", "Walls layer: 12 lines"),
    ] {
        index.upsert_chunk(hash, loc, text).unwrap();
    }
    drop(index);
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

async fn submit(s: &LucubraServer, mode: &str, items: Value) -> Value {
    let (err, env) = call(
        s,
        "lucubra_submit",
        json!({"action": "items", "unit": "bio/u1", "mode": mode, "items": items}),
    )
    .await;
    assert!(!err, "{env}");
    env
}

fn src(hash: &str, location: &str, anchor: &str) -> Value {
    json!({"file_hash": hash, "location": location, "anchor": anchor})
}

fn first_created(env: &Value) -> String {
    env["data"]["created"][0]["item_id"]
        .as_str()
        .unwrap_or_else(|| panic!("no item created: {env}"))
        .to_owned()
}

fn id_of(v: &Value) -> String {
    v["item_id"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn s05_s27_transcript_and_extended_labels() {
    let (_r, _d, s) = fixture().await;
    let env = submit(
        &s,
        "strict",
        json!([
            {"type": "cloze", "prompt": "Light reactions release ____ gas.",
             "answer": "oxygen",
             "sources": [src(PHOTO, "img:transcript", "split water and release oxygen gas")]},
            {"type": "true_false", "prompt": "Rubisco tells CO2 and O2 apart perfectly.",
             "answer": false,
             "sources": [src(NOTES, "t:150-300", "Rubisco does not distinguish perfectly")]}
        ]),
    )
    .await;
    let created = env["data"]["created"].as_array().unwrap();
    assert_eq!(created[0]["grounding"], "unverified_transcript", "{env}");
    assert_eq!(created[1]["grounding"], "verified", "{env}");
    let strict_wrong = id_of(&created[1]);

    let ext = submit(
        &s,
        "extended",
        json!([{"type": "mcq_single", "prompt": "Which enzyme fixes CO2 first in C4 plants?",
                "options": ["PEP carboxylase", "Rubisco"], "answer": "PEP carboxylase"}]),
    )
    .await;
    assert_eq!(ext["data"]["created"][0]["grounding"], "beyond_notes");
    let ext_id = first_created(&ext);

    let (_, strict_q) = call(
        &s,
        "lucubra_practice",
        json!({"action": "next", "limit": 10}),
    )
    .await;
    let queue = strict_q["data"]["queue"].as_array().unwrap();
    assert_eq!(queue.len(), 2, "extended items stay out of strict practice");
    let photo = queue
        .iter()
        .find(|q| q["grounding"] == "unverified_transcript")
        .unwrap();
    assert_eq!(photo["label"], "from an unverified transcript");
    assert!(queue.iter().all(|q| q.get("answer").is_none()));

    let (_, ext_q) = call(
        &s,
        "lucubra_practice",
        json!({"action": "next", "mode": "extended", "limit": 10}),
    )
    .await;
    let labelled = ext_q["data"]["queue"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["item_id"] == ext_id.as_str())
        .unwrap()
        .clone();
    assert_eq!(labelled["label"], "beyond your notes");

    // Readiness counts strict items only (S27).
    let (_, ok) = call(
        &s,
        "lucubra_practice",
        json!({"action": "answer", "item_id": ext_id, "response": "PEP carboxylase"}),
    )
    .await;
    assert_eq!(ok["data"]["correct"], true, "{ok}");
    let (_, wrong) = call(
        &s,
        "lucubra_practice",
        json!({"action": "answer", "item_id": strict_wrong, "response": "true"}),
    )
    .await;
    assert_eq!(wrong["data"]["correct"], false, "{wrong}");
    let (_, status) = call(&s, "lucubra_status", json!({})).await;
    let readiness = status["data"]["readiness"]["bio"].as_f64().unwrap();
    // One wrong strict answer: A = 0, and the new card's R today is 1.
    assert!((readiness - 0.6).abs() < 0.01, "readiness {readiness}");
}

#[tokio::test]
async fn s06_cells_names_and_retry_then_drop() {
    let (_r, _d, s) = fixture().await;
    let good = submit(
        &s,
        "strict",
        json!([
            {"type": "numeric", "prompt": "What is the total cost in the sheet?",
             "answer": 12.5, "tolerance": {"abs": 0.01},
             "sources": [src(SHEET, "x:Costs!B4", "12.5")]},
            {"type": "short_answer", "prompt": "Which layer holds the walls?",
             "answer": "Walls", "rubric": [{"criterion": "names the layer", "max_points": 1}],
             "sources": [src(PLAN, "l:Walls", "Walls")]}
        ]),
    )
    .await;
    assert_eq!(
        good["data"]["created"].as_array().unwrap().len(),
        2,
        "{good}"
    );

    let item = json!([{"type": "numeric", "prompt": "What does cell B4 hold?",
                       "answer": 13, "sources": [src(SHEET, "x:Costs!B4", "13")]}]);
    let first = submit(&s, "strict", item.clone()).await;
    let retry = first["data"]["retry_allowed"][0].clone();
    assert!(retry["reason"].as_str().unwrap().contains("B4"), "{first}");
    assert_eq!(first["next"], "regenerate_items");
    let second = submit(&s, "strict", item).await;
    assert_eq!(second["data"]["dropped"][0]["item_key"], retry["item_key"]);
    assert!(second["data"]["created"].as_array().unwrap().is_empty());

    let missing = submit(
        &s,
        "strict",
        json!([
            {"type": "numeric", "prompt": "C9?", "answer": 1,
             "sources": [src(SHEET, "x:Costs!C9", "1")]},
            {"type": "short_answer", "prompt": "Layer?", "answer": "Wall",
             "rubric": [{"criterion": "c", "max_points": 1}],
             "sources": [src(PLAN, "l:Wall", "Wall")]}
        ]),
    )
    .await;
    let reasons: Vec<&str> = missing["data"]["retry_allowed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["reason"].as_str().unwrap())
        .collect();
    assert!(reasons[0].contains("does not exist"), "{reasons:?}");
    assert!(reasons[1].contains("no layer"), "{reasons:?}");
}

#[tokio::test]
async fn grounding_text_adjacent_and_fabricated() {
    let (_r, _d, s) = fixture().await;
    let env = submit(
        &s,
        "strict",
        json!([
            {"type": "cloze", "prompt": "Only ____ in 6 triose phosphates leave.",
             "answer": "1",
             "sources": [src(NOTES, "t:0-150", "from the light reactions, and only 1 in 6 triose phosphates")]},
            {"type": "cloze", "prompt": "Rubisco ____ CO2 and O2.",
             "answer": "distinguishes",
             "sources": [src(NOTES, "t:150-300", "Rubisco distinguishes perfectly between carbon dioxide and oxygen")]}
        ]),
    )
    .await;
    assert_eq!(env["data"]["created"].as_array().unwrap().len(), 1, "{env}");
    assert_eq!(env["data"]["retry_allowed"][0]["index"], 1);
}

#[tokio::test]
async fn rubric_flow_checks_host_grades() {
    let (_r, _d, s) = fixture().await;
    let env = submit(
        &s,
        "strict",
        json!([{"type": "long_answer", "prompt": "Explain why photorespiration happens.",
                "answer": "Rubisco also binds oxygen.",
                "rubric": [{"criterion": "names rubisco", "max_points": 2},
                           {"criterion": "mentions oxygen", "max_points": 1}],
                "sources": [src(NOTES, "t:150-300",
                    "does not distinguish perfectly between carbon dioxide and oxygen")]}]),
    )
    .await;
    let id = first_created(&env);
    let (err, ans) = call(
        &s,
        "lucubra_practice",
        json!({"action": "answer", "item_id": id, "response": "Because rubisco grabs O2."}),
    )
    .await;
    assert!(!err, "{ans}");
    assert_eq!(ans["next"], "submit_grades");
    assert_eq!(ans["data"]["rubric"].as_array().unwrap().len(), 2);

    let grade = |scores: Value, total: f64| {
        json!({"action": "grades",
               "grades": [{"item_id": id, "criteria_scores": scores, "total": total}]})
    };
    let (err, bad) = call(
        &s,
        "lucubra_submit",
        grade(
            json!([{"criterion": "names rubisco", "score": 3},
                   {"criterion": "mentions oxygen", "score": 1}]),
            4.0,
        ),
    )
    .await;
    assert!(err && bad["code"] == "invalid_arguments", "{bad}");
    let (err, missing) = call(
        &s,
        "lucubra_submit",
        grade(json!([{"criterion": "names rubisco", "score": 2}]), 2.0),
    )
    .await;
    assert!(
        err && missing["fix"].as_str().unwrap().contains("mentions oxygen"),
        "{missing}"
    );
    let (err, ok) = call(
        &s,
        "lucubra_submit",
        grade(
            json!([{"criterion": "names rubisco", "score": 2},
                   {"criterion": "mentions oxygen", "score": 0}]),
            2.0,
        ),
    )
    .await;
    assert!(!err, "{ok}");
    assert_eq!(ok["data"]["graded_by"], "host");
    let score = ok["data"]["stored"][0]["score"].as_f64().unwrap();
    assert!((score - 2.0 / 3.0).abs() < 1e-9);
}

#[tokio::test]
async fn exam_sim_staircase_and_completion() {
    let (root, data, s) = fixture().await;
    let mk = |n: u32, d: u32| {
        json!({"type": "mcq_single", "prompt": format!("Question {n}"), "answer": "A",
               "options": ["A", "B"], "difficulty": d,
               "sources": [src(NOTES, "t:0-150", "The Calvin cycle fixes carbon dioxide")]})
    };
    submit(
        &s,
        "strict",
        json!([mk(1, 2), mk(2, 3), mk(3, 4), mk(4, 5)]),
    )
    .await;
    let (err, start) = call(
        &s,
        "lucubra_practice",
        json!({"action": "exam_sim", "subject": "bio", "count": 3}),
    )
    .await;
    assert!(!err, "{start}");
    let sim = start["data"]["sim_id"].as_str().unwrap().to_owned();
    assert_eq!(start["data"]["item"]["difficulty"], 3.0);
    assert!(start["data"]["item"].get("answer").is_none());

    let answer = |item: String, response: &str| json!({"action": "exam_sim", "sim_id": sim, "item_id": item, "response": response});
    let (_, step2) = call(
        &s,
        "lucubra_practice",
        answer(id_of(&start["data"]["item"]), "A"),
    )
    .await;
    assert_eq!(step2["data"]["item"]["difficulty"], 4.0, "up: {step2}");
    let (_, step3) = call(
        &s,
        "lucubra_practice",
        answer(id_of(&step2["data"]["item"]), "B"),
    )
    .await;
    // A wrong answer lowers the step to 3; items left are difficulty 2 and 5.
    assert_eq!(step3["data"]["item"]["difficulty"], 2.0, "down: {step3}");
    let (_, done) = call(
        &s,
        "lucubra_practice",
        answer(id_of(&step3["data"]["item"]), "A"),
    )
    .await;
    assert_eq!(done["data"]["completed"], true, "{done}");
    assert_eq!(done["data"]["answered"], 3);
    assert_eq!(done["data"]["correct_count"], 2);

    drop(s);
    let ws = Workspace::open(root.path(), data.path()).unwrap();
    let state = ws.replay().unwrap().state;
    assert_eq!(
        state.get(col::EXAM_SIMS, &sim).unwrap()["status"],
        "completed"
    );
}

#[tokio::test]
async fn s26_s40_tutor_mode() {
    let (_r, _d, s) = fixture().await;
    let (_, status) = call(&s, "lucubra_status", json!({})).await;
    let asks = status["data"]["needs_user"].as_array().unwrap();
    assert!(
        asks.iter().any(|a| a["path"] == "inbox/assignment 3.pdf"),
        "{status}"
    );
    assert_eq!(status["next"], "confirm_assessment");

    let (err, tagged) = call(
        &s,
        "lucubra_submit",
        json!({"action": "tag", "path": "inbox/assignment 3.pdf", "assessment": true}),
    )
    .await;
    assert!(!err, "{tagged}");
    let (_, status) = call(&s, "lucubra_status", json!({})).await;
    assert!(status["data"]["needs_user"].as_array().unwrap().is_empty());

    let read_task = json!({"action": "chunk", "file_hash": TASK, "locator": "p:1"});
    let (_, read) = call(&s, "lucubra_read", read_task.clone()).await;
    assert_eq!(read["data"]["tutor_mode"]["active"], true, "{read}");
    let (_, hits) = call(&s, "lucubra_search", json!({"query": "photorespiration"})).await;
    let hit = hits["data"]["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["file_hash"] == TASK)
        .unwrap_or_else(|| panic!("{hits}"))
        .clone();
    assert_eq!(hit["tutor_mode"]["active"], true);

    let (err, refused) = call(
        &s,
        "lucubra_submit",
        json!({"action": "items", "unit": "bio/u1", "items": [{
            "type": "solution", "solution": true, "prompt": "Model answer for Assignment 3",
            "answer": "Photorespiration happens because...",
            "sources": [src(TASK, "p:1", "Explain photorespiration in C3 plants")]}]}),
    )
    .await;
    assert!(err && refused["code"] == "integrity_rejected", "{refused}");

    let (err, off) = call(
        &s,
        "lucubra_submit",
        json!({"action": "tag", "file_hash": TASK, "assessment": false}),
    )
    .await;
    assert!(!err, "{off}");
    let (_, read) = call(&s, "lucubra_read", read_task).await;
    assert!(read["data"]["tutor_mode"].is_null(), "switched off: {read}");
}

#[tokio::test]
async fn s32_anki_export() {
    let (root, _d, s) = fixture().await;
    let export = |fmt: &str| json!({"action": "anki", "subject": "bio", "format": fmt});
    let (err, none) = call(&s, "lucubra_export", export("apkg")).await;
    assert!(err && none["code"] == "not_found", "{none}");
    submit(
        &s,
        "strict",
        json!([
            {"type": "cloze", "prompt": "Rubisco fixes ____.", "answer": "carbon dioxide",
             "sources": [src(NOTES, "t:0-150", "fixes carbon dioxide onto ribulose bisphosphate")]},
            {"type": "mcq_single", "prompt": "Which gas competes with CO2?",
             "options": ["oxygen", "nitrogen"], "answer": "oxygen",
             "sources": [src(NOTES, "t:150-300", "between carbon dioxide and oxygen")]}
        ]),
    )
    .await;
    let (err, apkg) = call(&s, "lucubra_export", export("apkg")).await;
    assert!(!err, "{apkg}");
    assert_eq!(apkg["data"]["cards"], 2);
    let notice = apkg["data"]["notice"].as_str().unwrap();
    assert!(notice.contains("do not flow back"));
    let path = root.path().join(apkg["data"]["path"].as_str().unwrap());
    let mut zip = zip::ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
    let mut db = Vec::new();
    zip.by_name("collection.anki2")
        .unwrap()
        .read_to_end(&mut db)
        .unwrap();
    assert!(db.starts_with(b"SQLite format 3"));

    let (err, tsv) = call(&s, "lucubra_export", export("tsv")).await;
    assert!(!err, "{tsv}");
    let rel = tsv["data"]["path"].as_str().unwrap();
    let text = fs::read_to_string(root.path().join(rel)).unwrap();
    assert_eq!(text.lines().filter(|l| !l.starts_with('#')).count(), 2);

    // Exports never come back in as study material.
    let found = lucubra_ingest::discover::discover(root.path()).unwrap();
    assert!(found.files.iter().all(|f| !f.rel.starts_with("exports/")));
}
