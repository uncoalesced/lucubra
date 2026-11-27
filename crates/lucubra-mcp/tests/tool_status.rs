// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_status` suite.

mod support;
use serde_json::json;
use support::{contract, ok, open};

#[tokio::test]
async fn status_contract_compact_and_detail() {
    let fx = open("claude-ai").await;
    contract(&fx, "lucubra_status", None).await;
    let data = ok(&fx, "lucubra_status", json!({})).await;
    assert!(data.get("metrics").is_some(), "{data}");
    assert_eq!(data["domain_pack"]["id"], "humanities-law", "{data}");
    let detail = ok(&fx, "lucubra_status", json!({"detail": true})).await;
    assert!(detail.get("metrics").is_some(), "{detail}");
}
