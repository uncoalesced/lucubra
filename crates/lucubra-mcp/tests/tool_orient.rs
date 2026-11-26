// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_orient` suite.

mod support;
use serde_json::json;
use support::{contract, ok, open, open_with};

#[tokio::test]
async fn orient_contract_and_compact_view() {
    let fx = open("claude-ai").await;
    contract(&fx, "lucubra_orient", None).await;
    let data = ok(&fx, "lucubra_orient", json!({})).await;
    assert_eq!(data["host"]["id"], "claude-desktop");
    assert!(data.get("subjects").is_some(), "{data}");
    assert!(data.get("profile").is_some(), "{data}");
}

#[tokio::test]
async fn orient_works_on_an_empty_workspace() {
    let fx = open_with("cursor", |_| {}).await;
    let data = ok(&fx, "lucubra_orient", json!({})).await;
    assert_eq!(data["host"]["id"], "cursor");
}
