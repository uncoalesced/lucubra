// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_submit` suite.

mod support;
use serde_json::json;
use support::{call, contract, ok, open, refused};

#[tokio::test]
async fn submit_contract_and_unit_summary() {
    let fx = open("claude-ai").await;
    contract(&fx, "lucubra_submit", Some("tag")).await;
    ok(
        &fx,
        "lucubra_submit",
        json!({"action": "unit_summary", "subject": "torts", "unit": "duty",
        "title": "Duty of care", "summary": "When a duty of care arises."}),
    )
    .await;
    let unit = ok(
        &fx,
        "lucubra_read",
        json!({"action": "unit", "subject": "torts", "unit": "duty"}),
    )
    .await;
    assert!(unit.to_string().contains("Duty of care"), "{unit}");
    refused(
        &fx,
        "lucubra_submit",
        json!({"action": "unit_summary", "subject": "Bad Slug", "unit": "x", "summary": "s"}),
        "invalid_slug",
    )
    .await;
}

#[tokio::test]
async fn submit_ungrounded_strict_item_is_not_stored() {
    let fx = open("claude-ai").await;
    let (_, env) = call(&fx, "lucubra_submit", json!({"action": "items", "subject": "torts", "unit": "negligence",
        "items": [{"type": "true_false", "prompt": "Invented fact.", "answer": "true", "mode": "strict",
                   "sources": [{"file_hash": "ffff", "location": "h:1", "anchor": "nothing"}]}]})).await;
    let stored = env["data"]["created"].as_array().map_or(0, Vec::len);
    assert_eq!(stored, 0, "{env}");
}
