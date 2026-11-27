// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_plan` suite.

mod support;
use serde_json::json;
use support::{contract, ok, open, refused};

#[tokio::test]
async fn plan_contract_next_and_unknown_plan() {
    let fx = open("claude-ai").await;
    contract(&fx, "lucubra_plan", Some("next")).await;
    let next = ok(&fx, "lucubra_plan", json!({"action": "next"})).await;
    assert!(next.is_object(), "{next}");
    refused(
        &fx,
        "lucubra_plan",
        json!({"action": "accept", "plan_id": "no-such-plan"}),
        "not_found",
    )
    .await;
}
