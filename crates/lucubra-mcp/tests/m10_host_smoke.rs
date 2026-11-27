// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! M10 host smoke simulation. For each of the 11 host profiles, a seeded
//! workspace is opened with that host's `clientInfo` name and a core study
//! flow runs end to end. Every response must stay within the profile's
//! token budget. Real desktop and phone hosts are not tested here; GATES and
//! HOST_MATRIX record them as runs for Joel.

mod support;

use lucubra_mcp::budget::{BudgetTier, estimate_tokens};
use serde_json::json;
use support::{call, open};

/// `clientInfo` name, expected profile id and default tier (from the
/// detection table in `host.rs`).
const HOSTS: &[(&str, &str, BudgetTier)] = &[
    ("claude-ai", "claude-desktop", BudgetTier::Free),
    ("claude-code", "claude-code", BudgetTier::Paid),
    ("openai-codex", "codex", BudgetTier::Free),
    ("google-antigravity", "antigravity", BudgetTier::Free),
    ("cursor", "cursor", BudgetTier::Free),
    ("windsurf", "windsurf", BudgetTier::Free),
    (
        "Visual Studio Code Copilot",
        "vscode-copilot",
        BudgetTier::Free,
    ),
    ("LM Studio", "lm-studio", BudgetTier::Local),
    ("Hermes Agent", "hermes-agent", BudgetTier::Free),
    ("open-webui", "jan-open-webui", BudgetTier::Local),
    ("unknown-client", "generic", BudgetTier::Free),
];

#[tokio::test]
async fn m10_every_host_profile_runs_the_core_flow_within_budget() {
    for &(client, id, tier) in HOSTS {
        let fx = open(client).await;
        let limits = tier.limits();
        let flow = [
            ("lucubra_orient", json!({})),
            ("lucubra_profile", json!({"action": "next"})),
            ("lucubra_status", json!({})),
            ("lucubra_search", json!({"query": "negligence"})),
            (
                "lucubra_read",
                json!({"action": "unit", "subject": "torts", "unit": "negligence"}),
            ),
            ("lucubra_plan", json!({"action": "propose"})),
            (
                "lucubra_session",
                json!({"action": "start", "units": ["torts/negligence"]}),
            ),
            (
                "lucubra_session",
                json!({"action": "step", "unit": "torts/negligence", "minutes": 10}),
            ),
            ("lucubra_practice", json!({"action": "next"})),
            (
                "lucubra_session",
                json!({"action": "end", "summary": "Negligence reviewed."}),
            ),
            ("lucubra_export", json!({"action": "brief"})),
        ];
        let mut sizes = Vec::new();
        for (tool, args) in flow {
            let (is_error, env) = call(&fx, tool, args.clone()).await;
            assert!(!is_error, "{client}: {tool} {args} failed: {env}");
            let tokens = estimate_tokens(&env.to_string());
            let cap = if tool == "lucubra_orient" {
                limits.orient_limit
            } else {
                limits.response_limit
            };
            assert!(
                tokens <= cap,
                "{client}: {tool} used {tokens} tokens over the {tier:?} cap {cap}"
            );
            if tool == "lucubra_orient" {
                assert!(env.to_string().contains(id), "{client}: {env}");
            }
            sizes.push(format!("{}={tokens}", tool.trim_start_matches("lucubra_")));
        }
        println!("{client} ({tier:?}): {}", sizes.join(" "));
    }
}
