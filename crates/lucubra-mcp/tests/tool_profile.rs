// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_profile` suite.

mod support;
use serde_json::json;
use support::{contract, ok, open, open_with, refused};

#[tokio::test]
async fn profile_contract_get_and_set() {
    let fx = open("claude-ai").await;
    contract(&fx, "lucubra_profile", Some("get")).await;
    let got = ok(&fx, "lucubra_profile", json!({"action": "get"})).await;
    assert!(got.to_string().contains("LLB"), "{got}");
    ok(
        &fx,
        "lucubra_profile",
        json!({"action": "set", "field": "plan_tier", "value": "paid"}),
    )
    .await;
    refused(
        &fx,
        "lucubra_profile",
        json!({"action": "set", "field": "no_such_field", "value": 1}),
        "invalid_arguments",
    )
    .await;
}

#[tokio::test]
async fn profile_next_asks_then_answer_records() {
    let fx = open_with("claude-ai", |_| {}).await;
    let next = ok(&fx, "lucubra_profile", json!({"action": "next"})).await;
    assert!(!next.is_null());
    ok(
        &fx,
        "lucubra_profile",
        json!({"action": "answer", "field": "programme", "value": "MBBS"}),
    )
    .await;
    let got = ok(&fx, "lucubra_profile", json!({"action": "get"})).await;
    assert!(got.to_string().contains("MBBS"), "{got}");
}
