// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_read` suite.

mod support;
use serde_json::json;
use support::{contract, ok, open, refused};

#[tokio::test]
async fn read_contract_index_subject_unit() {
    let fx = open("claude-ai").await;
    contract(&fx, "lucubra_read", Some("index")).await;
    let index = ok(&fx, "lucubra_read", json!({"action": "index"})).await;
    assert!(index.to_string().contains("torts"), "{index}");
    ok(
        &fx,
        "lucubra_read",
        json!({"action": "subject", "subject": "torts"}),
    )
    .await;
    let unit = ok(
        &fx,
        "lucubra_read",
        json!({"action": "unit", "subject": "torts", "unit": "negligence"}),
    )
    .await;
    assert!(unit.to_string().contains("Negligence"), "{unit}");
}

#[tokio::test]
async fn read_refuses_bad_slugs_and_missing_units() {
    let fx = open("claude-ai").await;
    refused(
        &fx,
        "lucubra_read",
        json!({"action": "unit", "subject": "../x", "unit": "y"}),
        "invalid_slug",
    )
    .await;
    refused(
        &fx,
        "lucubra_read",
        json!({"action": "unit", "subject": "torts", "unit": "missing"}),
        "not_found",
    )
    .await;
}
