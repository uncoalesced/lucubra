// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! M8 gate tests: sleep view rules (S28), status metrics within budget, the
//! static dashboard snapshot, check-ins, domain pack questions and the
//! companion MCP recommendation (S44).

use lucubra_core::Workspace;
use lucubra_core::planner::{days_from_iso, iso_from_days};
use lucubra_mcp::LucubraServer;
use lucubra_mcp::budget::estimate_tokens;
use rmcp::model::{
    CallToolResponse, ClientCapabilities, ContentBlock, Implementation, InitializeRequestParams,
};
use serde_json::{Value, json};

type Fixture = (tempfile::TempDir, tempfile::TempDir, LucubraServer);

/// A workspace with `checkins` check-in days, the first `reviewed` of which
/// also have a card review, opened as `client`.
async fn fixture(client: &str, programme: &str, checkins: i64, reviewed: i64) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let ws = Workspace::init(root.path(), data.path()).unwrap();
    ws.append("profile.set", json!({"programme": programme}))
        .unwrap();
    ws.append(
        "file.seen",
        json!({"path": "studio/house.blend", "hash": "b".repeat(64), "bytes": 64}),
    )
    .unwrap();
    ws.append(
        "exam.set",
        json!({"id": "jury-1", "subject": "studio", "date": "2026-12-15", "exam_type": "viva"}),
    )
    .unwrap();
    let base = days_from_iso("2026-10-01").unwrap();
    for i in 0..checkins {
        ws.append(
            "checkin.logged",
            json!({"date": iso_from_days(base + i), "sleep": 5.0 + (i % 4) as f64,
                   "quality": 3, "energy": 3}),
        )
        .unwrap();
        if i < reviewed {
            let rating = if i % 4 >= 2 { "good" } else { "again" };
            ws.append(
                "card.reviewed",
                json!({"card_id": format!("c{i}"), "unit": "studio/u1",
                       "last_day": base + i, "last_rating": rating}),
            )
            .unwrap();
        }
    }
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
    (root, data, s)
}

async fn call(s: &LucubraServer, tool: &str, args: Value) -> (bool, Value, String) {
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
        text.text.clone(),
    )
}

#[tokio::test]
async fn s28_sleep_view_hidden_with_twelve_days() {
    let (_r, _d, s) = fixture("claude-ai", "B.Arch", 12, 12).await;
    let (err, env, _) = call(&s, "lucubra_status", json!({})).await;
    assert!(!err, "{env}");
    let sleep = &env["data"]["sleep_retention"];
    assert_eq!(sleep["available"], false);
    assert_eq!(sleep["message"], "not enough data yet");
    assert_eq!(sleep["paired_days"], 12);
}

#[tokio::test]
async fn s28_sleep_view_needs_thirty_paired_days() {
    let (_r, _d, s) = fixture("claude-ai", "B.Arch", 30, 29).await;
    let (_, env, _) = call(&s, "lucubra_status", json!({})).await;
    assert_eq!(env["data"]["sleep_retention"]["available"], false);

    let (_r, _d, s) = fixture("claude-ai", "B.Arch", 30, 30).await;
    let (_, env, _) = call(&s, "lucubra_status", json!({})).await;
    let sleep = &env["data"]["sleep_retention"];
    assert_eq!(sleep["available"], true, "{sleep}");
    assert_eq!(sleep["sample_size"], 30);
    assert_eq!(sleep["message"], "association in your data, not cause");
}

#[tokio::test]
async fn status_metrics_fit_the_free_budget_and_detail_fits_paid() {
    let (_r, _d, s) = fixture("claude-ai", "B.Arch", 30, 30).await;
    let (err, env, text) = call(&s, "lucubra_status", json!({})).await;
    assert!(!err, "{env}");
    let m = &env["data"]["metrics"];
    assert!(m["weeks"].as_array().unwrap().len() == 4);
    assert!(m["capacity_minutes"].as_u64().is_some());
    assert!(m["focus_span_minutes"].as_f64().is_some());
    assert!(
        m.get("retention_curve").is_none(),
        "compact form omits series"
    );
    assert_eq!(m["exams"][0]["exam_type"], "viva");
    assert!(
        m["exams"][0]["method"]
            .as_str()
            .unwrap()
            .contains("defend design decisions"),
        "architecture pack overrides the viva method"
    );
    assert!(
        estimate_tokens(&text) <= 2_000,
        "free status {}",
        estimate_tokens(&text)
    );

    let (err, env, text) = call(&s, "lucubra_status", json!({"detail": true})).await;
    assert!(!err, "{env}");
    assert_eq!(
        env["data"]["metrics"]["weeks"].as_array().unwrap().len(),
        12
    );
    assert!(env["data"]["metrics"]["retention_curve"].is_array());
    assert!(
        estimate_tokens(&text) <= 6_000,
        "detail status {}",
        estimate_tokens(&text)
    );

    let (err, env, _) = call(&s, "lucubra_status", json!({"details": true})).await;
    assert!(err, "unknown keys are refused: {env}");
}

#[tokio::test]
async fn s44_blender_file_gets_a_recommendation_and_nothing_is_installed() {
    let (root, _d, s) = fixture("claude-ai", "B.Arch", 0, 0).await;
    let (_, env, _) = call(&s, "lucubra_status", json!({})).await;
    let recs = env["data"]["recommendations"].as_array().unwrap();
    let blender = recs
        .iter()
        .find(|r| r["name"] == "Blender MCP")
        .expect("Blender recommended");
    assert_eq!(blender["found"], json!([".blend"]));
    assert!(
        blender["note"]
            .as_str()
            .unwrap()
            .contains("does not operate")
    );
    assert!(
        blender["note"]
            .as_str()
            .unwrap()
            .contains("installs nothing")
    );
    assert_eq!(env["data"]["domain_pack"]["id"], "architecture");
    // Nothing new appears in the workspace besides Lucubra's own state.
    let top: Vec<String> = std::fs::read_dir(root.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(!top.iter().any(|n| n.contains("blender")), "{top:?}");
}

#[tokio::test]
async fn checkin_and_pack_questions_through_tools() {
    let (_r, _d, s) = fixture("claude-ai", "B.Arch", 0, 0).await;
    let (err, env, _) = call(
        &s,
        "lucubra_session",
        json!({"action": "checkin", "sleep": 6.5, "quality": 3, "energy": 2, "date": "2026-11-24"}),
    )
    .await;
    assert!(!err, "{env}");
    assert_eq!(env["data"]["logged"]["date"], "2026-11-24");
    let (err, _, _) = call(
        &s,
        "lucubra_session",
        json!({"action": "checkin", "sleep": 6.5, "quality": 9, "energy": 2}),
    )
    .await;
    assert!(err, "quality out of range is refused");

    let (_, env, _) = call(&s, "lucubra_profile", json!({"action": "get"})).await;
    let pending = env["data"]["domain_pack_questions"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(pending, 2);
    let (err, env, _) = call(
        &s,
        "lucubra_profile",
        json!({"action": "answer", "field": "pack_design_tools", "value": "Blender"}),
    )
    .await;
    assert!(!err, "{env}");
    let (err, _, _) = call(
        &s,
        "lucubra_profile",
        json!({"action": "skip", "field": "pack_jury_date"}),
    )
    .await;
    assert!(!err);
    let (_, env, _) = call(&s, "lucubra_profile", json!({"action": "get"})).await;
    assert_eq!(env["data"]["domain_pack_questions"], json!([]));
    let (err, _, _) = call(
        &s,
        "lucubra_profile",
        json!({"action": "answer", "field": "pack_osce_stations", "value": "12"}),
    )
    .await;
    assert!(err, "another pack's question is not a field here");
}

#[tokio::test]
async fn snapshot_written_on_session_end_and_by_export() {
    let (root, _d, s) = fixture("claude-ai", "B.Arch", 0, 0).await;
    let path = root.path().join("dashboard/index.html");
    let (err, env, _) = call(&s, "lucubra_session", json!({"action": "start"})).await;
    assert!(!err, "{env}");
    assert!(!path.exists());
    let (err, env, _) = call(&s, "lucubra_session", json!({"action": "end"})).await;
    assert!(!err, "{env}");
    let html = std::fs::read_to_string(&path).unwrap();
    assert!(html.contains("Sleep and retention"));
    assert!(!html.contains("<script") && !html.contains("http"));

    std::fs::remove_file(&path).unwrap();
    let (err, env, _) = call(&s, "lucubra_export", json!({"action": "dashboard"})).await;
    assert!(!err, "{env}");
    assert_eq!(env["data"]["file"], "dashboard/index.html");
    assert!(path.is_file());
}
