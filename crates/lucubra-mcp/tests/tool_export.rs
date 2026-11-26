// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_export` suite.

mod support;
use serde_json::json;
use support::{contract, ok, open};

#[tokio::test]
async fn export_contract_dashboard_pack_ics_brief() {
    let fx = open("claude-ai").await;
    contract(&fx, "lucubra_export", Some("dashboard")).await;
    ok(&fx, "lucubra_export", json!({"action": "dashboard"})).await;
    assert!(fx.root.path().join("dashboard/index.html").is_file());
    ok(&fx, "lucubra_export", json!({"action": "pack"})).await;
    assert!(fx.root.path().join("packs/SKILL.md").is_file());
    ok(&fx, "lucubra_export", json!({"action": "ics"})).await;
    assert!(fx.root.path().join("schedule/lucubra-plan.ics").is_file());
    ok(&fx, "lucubra_export", json!({"action": "brief"})).await;
}
