// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_search` suite.

mod support;
use serde_json::json;
use support::{contract, ok, open, refused};

#[tokio::test]
async fn search_contract_and_modes_after_ingest() {
    let fx = open("claude-ai").await;
    contract(&fx, "lucubra_search", None).await;
    ok(
        &fx,
        "lucubra_ingest",
        json!({"action": "scan", "extract": true}),
    )
    .await;
    for mode in ["word", "prefix", "substring"] {
        let hits = ok(
            &fx,
            "lucubra_search",
            json!({"query": "neighbour", "mode": mode}),
        )
        .await;
        assert!(hits.to_string().contains("neighbour"), "{mode}: {hits}");
    }
    refused(
        &fx,
        "lucubra_search",
        json!({"query": "x", "mode": "regex"}),
        "invalid_arguments",
    )
    .await;
}
