// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! M5 gate tests: triage (S13), overcommit (S14), session recovery, end and
//! handoff (S10, S11), calendar read and write (S24, S25, S33) and
//! in-session sequencing with planning off (S39).

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;

use lucubra_core::Workspace;
use lucubra_core::hlc::FixedClock;
use lucubra_core::planner::{days_from_iso, iso_from_days};
use lucubra_mcp::LucubraServer;
use rmcp::model::{
    CallToolResponse, ClientCapabilities, ContentBlock, Implementation, InitializeRequestParams,
};
use serde_json::{Value, json};

const TODAY: &str = "2026-12-01";

fn day(offset: i64) -> String {
    iso_from_days(days_from_iso(TODAY).unwrap() + offset)
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

async fn server(ws: Workspace) -> LucubraServer {
    let s = LucubraServer::from_workspace(ws, false);
    s.record_client_initialize(
        InitializeRequestParams::new(
            ClientCapabilities::default(),
            Implementation::new("claude-ai", "1.0"),
        ),
        "2025-11-25",
    )
    .await
    .unwrap();
    s
}

/// A subject with `units` units of `pages` pages each, all classified, and
/// an exam `exam_in` days from TODAY.
fn seed(ws: &Workspace, subject: &str, units: usize, pages: u64, exam_in: i64) {
    ws.append("subject.created", json!({"subject": subject}))
        .unwrap();
    for u in 0..units {
        let hash = format!("{subject}{u:04}").repeat(4);
        let path = format!("{subject}/unit {u}.pdf");
        ws.append(
            "file.seen",
            json!({"path": path, "hash": hash, "bytes": 10}),
        )
        .unwrap();
        ws.append(
            "file.extracted",
            json!({"hash": hash, "format": "pdf", "chunks": 1, "pages": pages}),
        )
        .unwrap();
        ws.append(
            "file.classified",
            json!({"hash": hash, "subject": subject, "unit": format!("u{u}"), "by": "engine"}),
        )
        .unwrap();
    }
    ws.append(
        "exam.set",
        json!({"id": format!("{subject}-final"), "subject": subject, "date": day(exam_in)}),
    )
    .unwrap();
}

/// Three finished sessions of `minutes` each, so learned capacity = minutes.
fn history(ws: &Workspace, minutes: u64) {
    for i in 0..3 {
        let sid = format!("01HIST{i}");
        ws.append("session.started", json!({"session_id": sid}))
            .unwrap();
        ws.append(
            "step.completed",
            json!({"session_id": sid, "minutes": minutes}),
        )
        .unwrap();
        ws.append(
            "session.ended",
            json!({"session_id": sid, "reason": "planned"}),
        )
        .unwrap();
    }
}

fn fixture() -> (tempfile::TempDir, tempfile::TempDir, Workspace) {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let ws = Workspace::init(root.path(), data.path()).unwrap();
    (root, data, ws)
}

fn all_text(root: &Path) -> String {
    let mut out = String::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in fs::read_dir(dir).unwrap().flatten() {
            if e.path().is_dir() {
                stack.push(e.path());
            } else {
                out.push_str(&String::from_utf8_lossy(&fs::read(e.path()).unwrap()));
            }
        }
    }
    out
}

#[tokio::test]
async fn s13_triage_states_the_shortfall_and_plans_top_units_only() {
    let (_r, _d, ws) = fixture();
    // 8 units x 100 pages x 500 tokens x 6 min per 1k tokens = 8 x 300 minutes.
    seed(&ws, "statistics", 8, 100, 3);
    history(&ws, 180);
    let s = server(ws).await;
    let (err, env) = call(
        &s,
        "lucubra_plan",
        json!({"action": "propose", "days": 4, "available_minutes": 180, "today": TODAY}),
    )
    .await;
    assert!(!err, "{env}");
    let t = &env["data"]["triage"][0];
    assert_eq!(t["required_minutes"], 2400, "{env}");
    assert_eq!(t["available_minutes"], 540);
    assert!(!t["uncovered_units"].as_array().unwrap().is_empty());
    assert!(
        t["message"]
            .as_str()
            .unwrap()
            .contains("40.0 hours needed, 9.0 hours available")
    );
    assert_eq!(env["next"], "explain_triage");
    let planned: u64 = env["data"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["minutes"].as_u64().unwrap())
        .sum();
    assert!(planned <= 540, "planned {planned}");
}

#[tokio::test]
async fn s14_overcommit_warns_once_and_the_plan_stands() {
    let (root, _d, ws) = fixture();
    seed(&ws, "statistics", 2, 20, 10);
    let s = server(ws).await;
    let (_, prop) = call(
        &s,
        "lucubra_plan",
        json!({"action": "propose", "today": TODAY}),
    )
    .await;
    assert_eq!(prop["data"]["capacity_minutes"], 90, "{prop}");
    let pid = prop["data"]["plan_id"].as_str().unwrap().to_owned();
    let blocks: Vec<Value> = (0..2)
        .flat_map(|d| {
            (0..4).map(move |_| {
                json!({"date": day(d), "subject": "statistics", "unit": "u0", "minutes": 60})
            })
        })
        .collect();
    let (err, _) = call(
        &s,
        "lucubra_plan",
        json!({"action": "edit", "plan_id": pid, "blocks": blocks}),
    )
    .await;
    assert!(!err);
    let (err, acc) = call(
        &s,
        "lucubra_plan",
        json!({"action": "accept", "plan_id": pid}),
    )
    .await;
    assert!(!err, "{acc}");
    let warnings = acc["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1, "{acc}");
    let w = warnings[0].as_str().unwrap();
    assert!(w.contains("240") && w.contains("90"), "{w}");
    assert_eq!(acc["data"]["accepted"], true);
    let log = all_text(&root.path().join(".lucubra/events"));
    for kind in ["plan.proposed", "plan.edited", "plan.accepted"] {
        assert!(log.contains(kind), "missing {kind}");
    }
    let ics = fs::read_to_string(root.path().join("schedule/lucubra-plan.ics")).unwrap();
    assert_eq!(ics.matches("SUMMARY:Study").count(), 8);
}

#[tokio::test]
async fn s10_s11_idle_session_is_recovered_and_handoffs_are_written() {
    let (root, _d, ws) = fixture();
    seed(&ws, "statistics", 2, 20, 10);
    let three_hours_ago = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
        - 3 * 60 * 60 * 1000;
    let old = ws.with_clock(FixedClock(three_hours_ago));
    old.append(
        "session.started",
        json!({"session_id": "01CUT", "units": ["statistics/u0"]}),
    )
    .unwrap();
    old.append(
        "step.completed",
        json!({"session_id": "01CUT", "minutes": 25, "unit": "statistics/u0"}),
    )
    .unwrap();
    let s = server(old).await;
    let (_, orient) = call(&s, "lucubra_orient", json!({})).await;
    assert_eq!(
        orient["data"]["interrupted_session"]["session_id"], "01CUT",
        "{orient}"
    );
    let log = all_text(&root.path().join(".lucubra/events"));
    assert!(log.contains("\"recovered\""));
    let sessions = root.path().join("sessions");
    assert_eq!(
        fs::read_dir(&sessions).unwrap().count(),
        1,
        "recovered handoff"
    );

    let (_, start) = call(
        &s,
        "lucubra_session",
        json!({"action": "start", "units": ["statistics/u1"]}),
    )
    .await;
    assert_eq!(start["ok"], true, "{start}");
    call(
        &s,
        "lucubra_session",
        json!({"action": "step", "kind": "recall", "minutes": 20, "unit": "statistics/u1"}),
    )
    .await;
    let (err, end) = call(
        &s,
        "lucubra_session",
        json!({"action": "end", "reason": "user", "summary": "Covered regression basics."}),
    )
    .await;
    assert!(!err, "{end}");
    let file = end["data"]["handoff_file"].as_str().unwrap();
    let text = fs::read_to_string(root.path().join(file)).unwrap();
    assert!(
        text.contains("Summary (written by the assistant)"),
        "{text}"
    );
    assert!(text.contains("20 minutes logged"), "{text}");
    assert!(end["data"]["next_session"].is_string(), "{end}");
}

/// Serves one iCal response on a local port and returns the address.
fn serve_ics(body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((mut conn, _)) = listener.accept() {
            let mut buf = [0u8; 2048];
            let _ = conn.read(&mut buf);
            let _ = write!(
                conn,
                "HTTP/1.1 200 OK\r\nContent-Type: text/calendar\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    format!("http://127.0.0.1:{port}/private-xyz123/basic.ics")
}

#[tokio::test]
async fn s24_s25_calendar_busy_time_is_respected_and_the_address_stays_private() {
    let (root, _data, ws) = fixture();
    seed(&ws, "statistics", 4, 40, 20);
    let url = serve_ics(
        "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART:20261202T080000Z\r\nDTEND:20261202T230000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
    );
    lucubra_core::calendar::store_url(&ws, Some(&url)).unwrap();
    let s = server(ws).await;
    let (err, env) = call(
        &s,
        "lucubra_plan",
        json!({"action": "propose", "days": 3, "today": TODAY}),
    )
    .await;
    assert!(!err, "{env}");
    assert_eq!(env["data"]["calendar"], "ical", "{env}");
    let busy_day = day(1);
    assert!(
        env["data"]["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|b| b["date"] != busy_day.as_str()),
        "{env}"
    );
    assert!(!env.to_string().contains("private-xyz123"));
    assert!(!all_text(root.path()).contains("private-xyz123"));

    // S25: the feed is gone now; the plan still works and explains the fallback.
    let (_, env) = call(
        &s,
        "lucubra_plan",
        json!({"action": "propose", "days": 3, "today": TODAY}),
    )
    .await;
    let warning = env["warnings"][0].as_str().unwrap();
    assert!(warning.contains("Calendar not read") && warning.contains("connector"));
    assert!(!warning.contains("private-xyz123"));
}

#[tokio::test]
async fn s33_changed_exam_date_regenerates_the_calendar() {
    let (root, _d, ws) = fixture();
    seed(&ws, "statistics", 2, 20, 10);
    let s = server(ws).await;
    let (_, prop) = call(
        &s,
        "lucubra_plan",
        json!({"action": "propose", "today": TODAY}),
    )
    .await;
    let pid = prop["data"]["plan_id"].as_str().unwrap().to_owned();
    call(
        &s,
        "lucubra_plan",
        json!({"action": "accept", "plan_id": pid}),
    )
    .await;
    let ics_path = root.path().join("schedule/lucubra-plan.ics");
    let old = day(10).replace('-', "");
    assert!(fs::read_to_string(&ics_path).unwrap().contains(&old));
    let (_, ans) = call(
        &s,
        "lucubra_profile",
        json!({"action": "answer", "field": "exams", "value": [
            {"id": "statistics-final", "subject": "statistics", "date": day(12)}
        ]}),
    )
    .await;
    assert_eq!(ans["next"], "replan", "{ans}");
    let ics = fs::read_to_string(&ics_path).unwrap();
    assert!(ics.contains(&day(12).replace('-', "")));
    assert!(!ics.contains(&format!("VALUE=DATE:{old}")));
}

#[tokio::test]
async fn s39_planning_off_gives_in_session_sequencing_only() {
    let (_r, _d, ws) = fixture();
    seed(&ws, "statistics", 2, 20, 10);
    ws.append("profile.set", json!({"schedule_planning": false}))
        .unwrap();
    let s = server(ws).await;
    let (_, prop) = call(
        &s,
        "lucubra_plan",
        json!({"action": "propose", "today": TODAY}),
    )
    .await;
    assert_eq!(prop["data"]["schedule_planning"], false);
    let (_, hz) = call(
        &s,
        "lucubra_plan",
        json!({"action": "horizon", "today": TODAY}),
    )
    .await;
    assert_eq!(hz["data"]["schedule_planning"], false);
    let (err, next) = call(
        &s,
        "lucubra_plan",
        json!({"action": "next", "today": TODAY}),
    )
    .await;
    assert!(!err, "{next}");
    assert_eq!(next["data"]["study_now"]["subject"], "statistics");
}
