// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! In-process helpers for the per-tool suites (`tool_<name>.rs`): a seeded
//! workspace opened as a named host, and a call wrapper returning the envelope.

#![allow(dead_code)]

use lucubra_core::Workspace;
use lucubra_mcp::LucubraServer;
use rmcp::model::{
    CallToolResponse, ClientCapabilities, ContentBlock, Implementation, InitializeRequestParams,
};
use serde_json::{Value, json};

pub struct Fx {
    pub root: tempfile::TempDir,
    _data: tempfile::TempDir,
    pub server: LucubraServer,
}

/// Seeds a law workspace: profile, one unit with a summary, one strict item,
/// an exam and a Markdown note in `inbox/`.
pub fn seed(ws: &Workspace) {
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
    ws.append(
        "exam.set",
        json!({"id": "torts-end", "subject": "torts", "date": "2027-04-20", "exam_type": "theory"}),
    )
    .unwrap();
    std::fs::write(
        ws.root().join("inbox/negligence.md"),
        "# Negligence\nDonoghue v Stevenson set out the neighbour principle.\n",
    )
    .unwrap();
    ws.project().unwrap();
}

/// Opens a seeded workspace as `client`.
pub async fn open(client: &str) -> Fx {
    open_with(client, seed).await
}

/// Opens a workspace prepared by `prepare` as `client`.
pub async fn open_with(client: &str, prepare: impl FnOnce(&Workspace)) -> Fx {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let ws = Workspace::init(root.path(), data.path()).unwrap();
    prepare(&ws);
    let server = LucubraServer::from_workspace(ws, false);
    server
        .record_client_initialize(
            InitializeRequestParams::new(
                ClientCapabilities::default(),
                Implementation::new(client, "1.0"),
            ),
            "2025-11-25",
        )
        .await
        .unwrap();
    Fx {
        root,
        _data: data,
        server,
    }
}

/// Calls a tool; returns `(is_error, envelope)`.
pub async fn call(fx: &Fx, tool: &str, args: Value) -> (bool, Value) {
    let Value::Object(map) = args else {
        panic!("args must be an object")
    };
    let CallToolResponse::Complete(res) = fx.server.handle_call_raw(tool, Some(map)).await.unwrap()
    else {
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

/// Calls a tool and asserts success; returns `data`.
pub async fn ok(fx: &Fx, tool: &str, args: Value) -> Value {
    let (err, env) = call(fx, tool, args.clone()).await;
    assert!(!err && env["ok"] != false, "{tool} {args}: {env}");
    env["data"].clone()
}

/// Calls a tool and asserts it is refused with `code` and a one-line fix (S23).
pub async fn refused(fx: &Fx, tool: &str, args: Value, code: &str) -> Value {
    let (_, env) = call(fx, tool, args.clone()).await;
    assert_eq!(env["ok"], false, "{tool} {args}: {env}");
    assert_eq!(env["code"], code, "{tool} {args}: {env}");
    let fix = env["fix"].as_str().unwrap_or_default();
    assert!(
        !fix.is_empty() && !fix.contains('\n'),
        "fix must be one line: {env}"
    );
    env
}

/// The common contract: unknown keys are refused next to a valid call, and
/// for tools with an `action`, a missing or unknown action is refused.
pub async fn contract(fx: &Fx, tool: &str, valid_action: Option<&str>) {
    let mut args = json!({"no_such_key": 1});
    if let Some(action) = valid_action {
        args["action"] = json!(action);
        refused(fx, tool, json!({}), "invalid_action").await;
        refused(
            fx,
            tool,
            json!({"action": "no_such_action"}),
            "invalid_action",
        )
        .await;
    }
    refused(fx, tool, args, "invalid_arguments").await;
}
