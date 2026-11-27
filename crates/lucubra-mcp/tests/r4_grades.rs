// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Round-4 terms and grades (SPEC round 4, A4): target CGPA (S50), a failed
//! subject carried as pending (S49, the CLI half is in `workspace_cli.rs`) and
//! subject prerequisites.

mod support;

use serde_json::json;
use support::{Fx, ok, open, refused};

async fn subjects(fx: &Fx, value: serde_json::Value) {
    ok(
        fx,
        "lucubra_profile",
        json!({"action": "set", "field": "subjects", "value": value}),
    )
    .await;
}

async fn result(fx: &Fx, subject: &str, term: &str, grade: &str, credits: f64) {
    ok(
        fx,
        "lucubra_profile",
        json!({"action": "result", "subject": subject, "term": term, "grade": grade, "credits": credits}),
    )
    .await;
}

#[tokio::test]
async fn s50_target_cgpa_gives_needed_grades_and_planner_weights() {
    let fx = open("claude-ai").await;
    subjects(
        &fx,
        json!([{"slug": "torts", "credits": 4}, {"slug": "contract", "credits": 2}]),
    )
    .await;
    ok(
        &fx,
        "lucubra_profile",
        json!({"action": "grading_scheme", "preset": "ten_point"}),
    )
    .await;
    // Semester 2 is past; the seeded current term is Semester 3.
    result(&fx, "crim", "Semester 2", "A", 4.0).await;
    let set = ok(
        &fx,
        "lucubra_profile",
        json!({"action": "target", "cgpa": 8.5}),
    )
    .await;
    let t = &set["grades"]["target"];
    assert_eq!(t["reachable"], true, "{set}");
    // (8.5 * 10 - 8 * 4) / 6 = 8.83 over the 6 pending credits.
    assert_eq!(t["needed_average"], 8.83, "{set}");
    let plan = t["plan"].as_array().unwrap();
    assert_eq!(plan.len(), 2, "{set}");
    for need in plan {
        assert!(need["grade"].is_string() && need["min_marks"].is_number());
    }
    assert!(t["projected"].as_f64().unwrap() >= 8.5, "{set}");
    assert_eq!(
        t["weights"],
        json!({"contract": 1.25, "torts": 1.5}),
        "the higher-credit subject moves the CGPA most: {set}"
    );

    let status = ok(&fx, "lucubra_status", json!({})).await;
    assert_eq!(status["grades"]["cgpa"], 8.0, "{status}");
    assert_eq!(status["grades"]["sgpa"]["Semester 2"], 8.0, "{status}");

    let high = ok(
        &fx,
        "lucubra_profile",
        json!({"action": "target", "cgpa": 10}),
    )
    .await;
    let t = &high["grades"]["target"];
    assert_eq!(t["reachable"], false, "{high}");
    // (32 + 10 * 6) / 10 = 9.2 at best.
    assert_eq!(t["best_reachable"], 9.2, "{high}");

    refused(
        &fx,
        "lucubra_profile",
        json!({"action": "result", "subject": "torts", "term": "Semester 3", "grade": "Z", "credits": 4}),
        "invalid_arguments",
    )
    .await;
    refused(
        &fx,
        "lucubra_profile",
        json!({"action": "result", "subject": "torts", "term": "Semester 3", "grade": "A", "credits": 0}),
        "invalid_arguments",
    )
    .await;
    refused(
        &fx,
        "lucubra_profile",
        json!({"action": "target", "cgpa": 11}),
        "invalid_arguments",
    )
    .await;
    refused(
        &fx,
        "lucubra_profile",
        json!({"action": "grading_scheme", "preset": "gpa4"}),
        "invalid_arguments",
    )
    .await;
}

#[tokio::test]
async fn s49_failed_subject_stays_pending_in_the_next_term() {
    let fx = open("claude-ai").await;
    subjects(&fx, json!([{"slug": "torts", "credits": 4}])).await;
    result(&fx, "torts", "Semester 2", "F", 4.0).await;
    let set = ok(
        &fx,
        "lucubra_profile",
        json!({"action": "target", "cgpa": 6}),
    )
    .await;
    let plan = &set["grades"]["target"]["plan"];
    assert_eq!(
        plan[0]["subject"], "torts",
        "a backlog subject is pending in the current term: {set}"
    );
    assert!(
        fx.root
            .path()
            .join("subjects/torts/negligence/UNIT.md")
            .is_file()
    );
}

#[tokio::test]
async fn prerequisites_are_logged_and_shown_in_the_subject_index() {
    let fx = open("claude-ai").await;
    subjects(
        &fx,
        json!([{"slug": "torts"}, {"slug": "advanced-torts", "requires": ["torts"]}]),
    )
    .await;
    let index = std::fs::read_to_string(
        fx.root
            .path()
            .join("subjects/advanced-torts/agents_advanced-torts.md"),
    )
    .unwrap_or_default();
    assert!(
        index.contains("Prerequisites: [[subjects/torts/agents_torts|"),
        "{index}"
    );
    refused(
        &fx,
        "lucubra_profile",
        json!({"action": "set", "field": "subjects", "value": [{"slug": "torts", "requires": ["torts"]}]}),
        "invalid_arguments",
    )
    .await;
}
