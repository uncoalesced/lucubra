// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_practice` suite.

mod support;
use serde_json::json;
use support::{contract, ok, open, refused};

#[tokio::test]
async fn practice_contract_next_and_answer() {
    let fx = open("claude-ai").await;
    contract(&fx, "lucubra_practice", Some("next")).await;
    let next = ok(&fx, "lucubra_practice", json!({"action": "next"})).await;
    assert!(next.to_string().contains("i1"), "{next}");
    let graded = ok(
        &fx,
        "lucubra_practice",
        json!({"action": "answer", "item_id": "i1", "response": "true"}),
    )
    .await;
    assert!(graded.to_string().contains("correct"), "{graded}");
    refused(
        &fx,
        "lucubra_practice",
        json!({"action": "answer", "item_id": "nope", "response": "x"}),
        "not_found",
    )
    .await;
}
