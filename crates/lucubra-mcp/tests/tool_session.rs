// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_session` suite.

mod support;
use serde_json::json;
use support::{contract, ok, open, refused};

#[tokio::test]
async fn session_contract_and_full_cycle() {
    let fx = open("claude-ai").await;
    contract(&fx, "lucubra_session", Some("resume")).await;
    refused(
        &fx,
        "lucubra_session",
        json!({"action": "step", "unit": "torts/negligence", "minutes": 5}),
        "invalid_state",
    )
    .await;
    ok(
        &fx,
        "lucubra_session",
        json!({"action": "start", "units": ["torts/negligence"], "style": "tutor"}),
    )
    .await;
    ok(
        &fx,
        "lucubra_session",
        json!({"action": "step", "unit": "torts/negligence", "minutes": 10}),
    )
    .await;
    ok(
        &fx,
        "lucubra_session",
        json!({"action": "checkin", "sleep": 7, "quality": 4, "energy": 3}),
    )
    .await;
    refused(
        &fx,
        "lucubra_session",
        json!({"action": "checkin", "sleep": 7, "quality": 9, "energy": 3}),
        "invalid_arguments",
    )
    .await;
    ok(
        &fx,
        "lucubra_session",
        json!({"action": "end", "summary": "Done."}),
    )
    .await;
    assert!(
        fx.root
            .path()
            .join("sessions")
            .read_dir()
            .unwrap()
            .next()
            .is_some()
    );
    assert!(fx.root.path().join("dashboard/index.html").is_file());
}
