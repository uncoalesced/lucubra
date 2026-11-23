// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! M4 gate tests: onboarding without and with elicitation (S01, S34, S35),
//! schedule planning declined (S02), engine classification (S03), and layout
//! approval with moves and undo (SPEC section 10.4).

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use lucubra_core::Workspace;
use lucubra_mcp::LucubraServer;
use rmcp::model::{
    CallToolResponse, ClientCapabilities, ContentBlock, Implementation, InitializeRequestParams,
};
use serde_json::{Value, json};

mod common;

const REQUIRED: &[&str] = &[
    "level",
    "programme",
    "term_system",
    "current_term",
    "start_year",
    "completion_year",
    "subjects",
    "exams",
];

type Fixture = (tempfile::TempDir, tempfile::TempDir, Workspace);

fn workspace(files: &[(&str, &str)]) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    for (rel, body) in files {
        let p = root.path().join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }
    let ws = Workspace::init(root.path(), data.path()).unwrap();
    (root, data, ws)
}

async fn server(ws: Workspace, client: &str) -> LucubraServer {
    let s = LucubraServer::from_workspace(ws, false);
    s.record_client_initialize(
        InitializeRequestParams::new(
            ClientCapabilities::default(),
            Implementation::new(client, "1.0"),
        ),
        "2025-11-25",
    )
    .await
    .unwrap();
    s
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

fn answer_for(field: &str) -> Value {
    match field {
        "level" => json!("bachelors"),
        "programme" => json!("B.Com"),
        "term_system" => json!("semester"),
        "current_term" => json!("Semester 5"),
        "start_year" => json!(2024),
        "completion_year" => json!(2027),
        "subjects" => json!([
            {"slug": "statistics", "title": "STAT201 Statistics"},
            {"slug": "income-tax", "title": "COM305 Income Tax"}
        ]),
        _ => json!([{"id": "stat-final", "subject": "statistics", "date": "2026-12-10"}]),
    }
}

/// Answers every required field conversationally; returns the number of
/// `answer` calls and the `next` payload seen for `subjects`.
async fn onboard(s: &LucubraServer) -> (usize, Value) {
    let mut answers = 0;
    let mut subjects_next = Value::Null;
    loop {
        let (err, env) = call(s, "lucubra_profile", json!({"action": "next"})).await;
        assert!(!err, "{env}");
        let Some(field) = env["data"]["next_field"].as_str().map(str::to_owned) else {
            break;
        };
        assert!(
            REQUIRED.contains(&field.as_str()),
            "optional field {field} asked early"
        );
        if field == "subjects" {
            subjects_next = env["data"].clone();
        }
        let (err, env) = call(
            s,
            "lucubra_profile",
            json!({"action": "answer", "field": field, "value": answer_for(&field)}),
        )
        .await;
        assert!(!err, "{field}: {env}");
        answers += 1;
    }
    (answers, subjects_next)
}

fn snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if p.is_dir() {
                if name != ".lucubra" && name != "subjects" {
                    stack.push(p);
                }
            } else if name != "AGENTS.md" {
                out.push((p.display().to_string(), fs::read(&p).unwrap()));
            }
        }
    }
    out.sort();
    out
}

fn events(root: &Path) -> Vec<Value> {
    let mut out = Vec::new();
    for e in fs::read_dir(root.join(".lucubra/events"))
        .unwrap()
        .flatten()
    {
        for line in fs::read_to_string(e.path()).unwrap().lines() {
            if let Ok(v) = serde_json::from_str::<Value>(line) {
                out.push(v);
            }
        }
    }
    out
}

const STUDY: &[(&str, &str)] = &[
    (
        "Statistics/STAT201 week1.md",
        "# Week 1\n\nMean, median and mode.",
    ),
    (
        "Income Tax/heads of income.md",
        "# Heads of income\n\nSalary and house property.",
    ),
];

#[tokio::test]
async fn s01_first_run_without_elicitation_moves_nothing_before_approval() {
    let (root, _d, ws) = workspace(STUDY);
    let s = server(ws, "claude-ai").await;
    let (_, env) = call(&s, "lucubra_ingest", json!({"action": "scan"})).await;
    assert_eq!(env["ok"], true, "{env}");

    let (answers, subjects_next) = onboard(&s).await;
    assert_eq!(answers, REQUIRED.len(), "one call per required field");
    let suggested: Vec<&str> = subjects_next["suggestions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v["slug"].as_str())
        .collect();
    assert!(
        suggested.contains(&"statistics") && suggested.contains(&"income-tax"),
        "{suggested:?}"
    );
    let host_logged = events(root.path())
        .iter()
        .any(|e| e.to_string().contains("claude-desktop"));
    assert!(
        host_logged,
        "host detected from the handshake without asking"
    );

    let before = snapshot(root.path());
    call(&s, "lucubra_ingest", json!({"action": "scan"})).await;
    let (err, env) = call(&s, "lucubra_ingest", json!({"action": "approve_layout"})).await;
    assert!(!err, "{env}");
    assert_eq!(env["data"]["approved"], false);
    assert_eq!(env["data"]["moves_total"], 2, "{env}");
    assert!(env["data"]["tree"]["statistics"].is_object(), "{env}");
    assert_eq!(
        snapshot(root.path()),
        before,
        "no file moved before approval"
    );
}

#[tokio::test]
async fn s02_declined_scheduling_is_never_raised_again() {
    let (_root, _d, ws) = workspace(STUDY);
    let s = server(ws, "claude-ai").await;
    let (err, _) = call(
        &s,
        "lucubra_profile",
        json!({"action": "answer", "field": "schedule_planning", "value": false}),
    )
    .await;
    assert!(!err);
    onboard(&s).await;
    let (_, orient) = call(&s, "lucubra_orient", json!({})).await;
    assert_eq!(
        orient["data"]["profile"]["schedule_planning"], false,
        "{orient}"
    );
    assert_ne!(orient["data"]["suggested_next"], "plan");
    let (_, plan) = call(&s, "lucubra_plan", json!({"action": "propose", "days": 7})).await;
    assert_eq!(plan["data"]["schedule_planning"], false, "{plan}");
    let (_, next) = call(&s, "lucubra_profile", json!({"action": "next"})).await;
    assert!(next["data"]["next_field"].is_null(), "{next}");
}

#[tokio::test]
async fn s03_engine_classifies_course_coded_files_and_batches_the_rest() {
    let mut files: Vec<(String, String)> = (0..20)
        .map(|i| {
            (
                format!("inbox/STAT201 lecture {i}.md"),
                format!("# Lecture {i}\n\nRegression."),
            )
        })
        .chain((0..20).map(|i| {
            (
                format!("inbox/com305_tax_{i}.md"),
                format!("# Tax {i}\n\nDeductions."),
            )
        }))
        .collect();
    files.push(("inbox/scan_0001.md".into(), "# Untitled".into()));
    files.push(("inbox/scan_0002.md".into(), "# Untitled two".into()));
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let (root, _d, ws) = workspace(&refs);
    let s = server(ws, "claude-ai").await;
    call(
        &s,
        "lucubra_profile",
        json!({"action": "answer", "field": "subjects", "value": answer_for("subjects")}),
    )
    .await;
    let (_, scan) = call(&s, "lucubra_ingest", json!({"action": "scan"})).await;
    let by_engine = scan["data"]["classify"]["by_engine"].as_u64().unwrap();
    assert!(
        by_engine >= 38,
        "engine classified {by_engine} of 40: {scan}"
    );
    assert_eq!(scan["next"], "classify");

    let (_, status) = call(&s, "lucubra_ingest", json!({"action": "status"})).await;
    let batch = &status["data"]["classification_batch"];
    assert_eq!(batch["total"], 2, "{batch}");
    assert_eq!(
        batch["items"][0]["head"]["trust"],
        "untrusted_study_material"
    );

    let (err, sub) = call(
        &s,
        "lucubra_submit",
        json!({"action": "classification", "classifications": [
            {"path": "inbox/scan_0001.md", "subject": "statistics"},
            {"path": "inbox/STAT201 lecture 3.md", "subject": "income-tax"}
        ]}),
    )
    .await;
    assert!(!err, "{sub}");
    assert_eq!(sub["data"]["applied"], 1);
    assert_eq!(sub["data"]["needs_user"][0]["engine_subject"], "statistics");
    let (_, sub) = call(
        &s,
        "lucubra_submit",
        json!({"action": "classification", "classifications": [
            {"path": "inbox/STAT201 lecture 3.md", "subject": "income-tax", "confirmed_by_user": true}
        ]}),
    )
    .await;
    assert_eq!(sub["data"]["applied"], 1, "{sub}");
    let by_user = events(root.path())
        .iter()
        .filter(|e| e["type"] == "file.classified" && e["data"]["by"] == "user")
        .count();
    assert_eq!(by_user, 1);
}

#[tokio::test]
async fn s34_exam_dates_are_asked_when_no_timetable_exists() {
    let (root, _d, ws) = workspace(STUDY);
    let timetable = root
        .path()
        .join("schedule")
        .read_dir()
        .is_ok_and(|mut d| d.next().is_some());
    assert!(!timetable, "fixture must have no timetable");
    let s = server(ws, "claude-ai").await;
    for field in &REQUIRED[..REQUIRED.len() - 1] {
        let (_, next) = call(&s, "lucubra_profile", json!({"action": "next"})).await;
        assert_eq!(next["data"]["next_field"], *field);
        call(
            &s,
            "lucubra_profile",
            json!({"action": "answer", "field": field, "value": answer_for(field)}),
        )
        .await;
    }
    let (_, next) = call(&s, "lucubra_profile", json!({"action": "next"})).await;
    assert_eq!(next["data"]["next_field"], "exams");
    assert!(
        next["data"]["prompt"].as_str().unwrap().contains("exam"),
        "{next}"
    );
}

#[tokio::test]
async fn moves_apply_after_approval_and_undo_restores() {
    let (root, _d, ws) = workspace(STUDY);
    let original = fs::read(root.path().join("Statistics/STAT201 week1.md")).unwrap();
    let s = server(ws, "claude-ai").await;
    call(&s, "lucubra_ingest", json!({"action": "scan"})).await;
    onboard(&s).await;
    call(&s, "lucubra_ingest", json!({"action": "scan"})).await;

    let (err, bad) = call(
        &s,
        "lucubra_ingest",
        json!({"action": "approve_layout", "moves": [
            {"from": "Statistics/STAT201 week1.md", "to": "../escape.md"}
        ]}),
    )
    .await;
    assert!(!err, "{bad}");
    assert_eq!(bad["data"]["moved_total"], 0);
    assert_eq!(bad["data"]["refused"].as_array().unwrap().len(), 1);

    let (err, done) = call(
        &s,
        "lucubra_ingest",
        json!({"action": "approve_layout", "accept_proposal": true, "folders": {"Income Tax": "index"}}),
    )
    .await;
    assert!(!err, "{done}");
    assert_eq!(done["data"]["moved_total"], 1, "{done}");
    let moved = root
        .path()
        .join("subjects/statistics/general/sources/STAT201 week1.md");
    assert_eq!(fs::read(&moved).unwrap(), original);
    assert!(
        root.path().join("Income Tax/heads of income.md").is_file(),
        "indexed folder stays"
    );
    let index =
        fs::read_to_string(root.path().join("subjects/income-tax/agents_income-tax.md")).unwrap();
    assert!(index.contains("Income Tax/heads of income.md"), "{index}");

    let (err, undone) = call(
        &s,
        "lucubra_ingest",
        json!({"action": "undo_moves", "scope": "all"}),
    )
    .await;
    assert!(!err, "{undone}");
    assert_eq!(undone["data"]["moved_total"], 1);
    assert_eq!(
        fs::read(root.path().join("Statistics/STAT201 week1.md")).unwrap(),
        original
    );
}

/// Drives the real binary over stdio as a client that supports form
/// elicitation and answers (or declines) the onboarding form.
fn stdio_onboard(ws: &Path, data: &Path, reply: Value) -> Value {
    let mut child = Command::new(common::lucubra_binary())
        .args(["mcp", "-w"])
        .arg(ws)
        .env("LUCUBRA_DATA_DIR", data)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut send = |v: Value| writeln!(stdin, "{v}").and_then(|()| stdin.flush()).unwrap();
    let mut read = || {
        let mut line = String::new();
        assert!(
            stdout.read_line(&mut line).unwrap() > 0,
            "server closed stdout"
        );
        serde_json::from_str::<Value>(line.trim()).unwrap()
    };
    send(
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-11-25",
        "capabilities": {"elicitation": {"form": {}}},
        "clientInfo": {"name": "Visual Studio Code", "version": "1.0"}}}),
    );
    assert_eq!(read()["id"], 1);
    send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    send(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "lucubra_profile", "arguments": {"action": "next"}}}));
    let mut forms = 0;
    let result = loop {
        let msg = read();
        if msg["method"] == "elicitation/create" {
            forms += 1;
            assert_eq!(msg["params"]["mode"], "form", "{msg}");
            send(json!({"jsonrpc": "2.0", "id": msg["id"], "result": reply.clone()}));
        } else if msg["id"] == 2 {
            break msg;
        }
    };
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(forms, 1, "exactly one onboarding form");
    let text = result["result"]["content"][0]["text"].as_str().unwrap();
    serde_json::from_str(text).unwrap()
}

/// Profile and subject events with ids, clocks and host metadata masked.
fn onboarding_events(root: &Path) -> Vec<Value> {
    let mut out: Vec<Value> = events(root)
        .into_iter()
        .filter(|e| e["type"] == "subject.created" || e["type"] == "profile.set")
        .map(|e| json!({"type": e["type"], "data": e["data"]}))
        .filter(|e| {
            e["data"].as_object().is_some_and(|d| {
                d.keys()
                    .all(|k| REQUIRED.contains(&k.as_str()) || k == "subject" || k == "title")
            })
        })
        .collect();
    out.sort_by_key(Value::to_string);
    out
}

#[tokio::test]
async fn s35_form_answers_write_the_same_events_as_conversation() {
    let form = json!({
        "level": "bachelors", "programme": "B.Com", "term_system": "semester",
        "current_term": "Semester 5", "start_year": 2024, "completion_year": 2027,
        "subjects": "STAT201 Statistics, COM305 Income Tax"
    });
    let (root, data, ws) = workspace(STUDY);
    drop(ws);
    let env = stdio_onboard(
        root.path(),
        data.path(),
        json!({"action": "accept", "content": form}),
    );
    assert_eq!(env["data"]["next_field"], "exams", "{env}");

    let (root2, _d2, ws2) = workspace(STUDY);
    let s = server(ws2, "claude-ai").await;
    for (field, value) in lucubra_mcp::elicit::answers(form.as_object().unwrap()) {
        call(
            &s,
            "lucubra_profile",
            json!({"action": "answer", "field": field, "value": value}),
        )
        .await;
    }
    let via_form = onboarding_events(root.path());
    assert!(!via_form.is_empty());
    assert_eq!(via_form, onboarding_events(root2.path()));

    let (root3, data3, ws3) = workspace(STUDY);
    drop(ws3);
    let env = stdio_onboard(root3.path(), data3.path(), json!({"action": "decline"}));
    assert_eq!(env["data"]["next_field"], "level", "{env}");
    assert!(onboarding_events(root3.path()).is_empty());
}
