// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! M9 gate S22: a study session driven from a phone through the Hermes Agent
//! messaging gateway. Orient states the per-message limit (Telegram
//! `sendMessage` takes 1 to 4096 characters), and every response of a short
//! session fits in one message.

use lucubra_core::Workspace;
use lucubra_mcp::LucubraServer;
use rmcp::model::{
    CallToolResponse, ClientCapabilities, ContentBlock, Implementation, InitializeRequestParams,
};
use serde_json::{Value, json};

/// Telegram Bot API `sendMessage` text limit, checked 2026-11-25.
const TELEGRAM_CHARS: usize = 4096;

async fn hermes() -> (tempfile::TempDir, tempfile::TempDir, LucubraServer) {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let ws = Workspace::init(root.path(), data.path()).unwrap();
    for (k, v) in [
        ("level", json!("undergraduate")),
        ("programme", json!("LLB")),
        ("term_system", json!("semester")),
        ("current_term", json!("Semester 3")),
        ("start_year", json!(2025)),
        ("completion_year", json!(2030)),
    ] {
        ws.append("profile.set", json!({ k: v })).unwrap();
    }
    ws.append(
        "unit.created",
        json!({"subject": "torts", "unit": "negligence", "title": "Negligence",
               "summary": "Duty, breach, causation and remoteness."}),
    )
    .unwrap();
    ws.append(
        "item.created",
        json!({"item_id": "i1", "unit": "torts/negligence", "type": "true_false",
               "prompt": "A duty of care needs proximity.", "answer": "true",
               "mode": "strict", "grounding": "verified", "difficulty": 2}),
    )
    .unwrap();
    let s = LucubraServer::from_workspace(ws, false);
    s.record_client_initialize(
        InitializeRequestParams::new(
            ClientCapabilities::default(),
            Implementation::new("hermes-agent", "0.9"),
        ),
        "2025-11-25",
    )
    .await
    .unwrap();
    (root, data, s)
}

async fn call(s: &LucubraServer, tool: &str, args: Value) -> (Value, usize) {
    let Value::Object(map) = args else {
        panic!("args must be an object")
    };
    let CallToolResponse::Complete(res) = s.handle_call_raw(tool, Some(map)).await.unwrap() else {
        panic!("expected complete response");
    };
    let ContentBlock::Text(text) = &res.content[0] else {
        panic!("expected text")
    };
    assert!(!res.is_error.unwrap_or(false), "{tool}: {}", text.text);
    (
        serde_json::from_str(&text.text).unwrap(),
        text.text.chars().count(),
    )
}

#[tokio::test]
async fn s22_hermes_session_fits_telegram_messages() {
    let (_r, _d, s) = hermes().await;
    let (orient, n) = call(&s, "lucubra_orient", json!({})).await;
    let host = &orient["data"]["host"];
    assert_eq!(host["id"], "hermes-agent");
    assert_eq!(host["message_chars"], TELEGRAM_CHARS);
    assert!(host["message_rule"].as_str().unwrap().contains("split"));
    assert!(n <= TELEGRAM_CHARS, "orient is {n} characters");

    let steps = [
        (
            "lucubra_session",
            json!({"action": "start", "units": ["torts/negligence"]}),
        ),
        ("lucubra_practice", json!({"action": "next"})),
        (
            "lucubra_practice",
            json!({"action": "answer", "item_id": "i1", "response": "true"}),
        ),
        (
            "lucubra_session",
            json!({"action": "step", "unit": "torts/negligence", "minutes": 10}),
        ),
        (
            "lucubra_session",
            json!({"action": "checkin", "sleep": 7, "quality": 4, "energy": 3}),
        ),
        ("lucubra_plan", json!({"action": "next"})),
        (
            "lucubra_session",
            json!({"action": "end", "summary": "Reviewed duty of care."}),
        ),
    ];
    for (tool, args) in steps {
        let label = format!("{tool} {}", args["action"]);
        let (_, n) = call(&s, tool, args).await;
        assert!(n <= TELEGRAM_CHARS, "{label} is {n} characters");
    }
}
