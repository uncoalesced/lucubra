// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_ingest` suite.

mod support;
use serde_json::json;
use support::{call, contract, ok, open};

#[tokio::test]
async fn ingest_contract_scan_and_status() {
    let fx = open("claude-ai").await;
    contract(&fx, "lucubra_ingest", Some("status")).await;
    let scan = ok(&fx, "lucubra_ingest", json!({"action": "scan"})).await;
    assert_eq!(scan["scan"]["new_or_changed"], 1, "{scan}");
    let status = ok(&fx, "lucubra_ingest", json!({"action": "status"})).await;
    assert!(status.is_object(), "{status}");
}

#[tokio::test]
async fn ingest_handoff_paste_and_undo_without_moves() {
    let fx = open("claude-ai").await;
    let cut = "```lucubra-handoff v1\nsession: s1\n";
    let (_, out) = call(
        &fx,
        "lucubra_ingest",
        json!({"action": "handoff", "text": cut}),
    )
    .await;
    assert!(out.to_string().contains("cut off"), "{out}");
    let (_, env) = call(
        &fx,
        "lucubra_ingest",
        json!({"action": "undo_moves", "scope": "last"}),
    )
    .await;
    assert!(env.is_object());
}
