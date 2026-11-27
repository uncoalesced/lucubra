// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Round-4 sessions scope (SPEC round 4, A1, A2, A7, A9): study scripts (S46),
//! teaching styles (S47), reminders (S54, S55) and the preferences file (S56).

mod support;

use std::time::{SystemTime, UNIX_EPOCH};

use lucubra_core::planner::iso_from_days;
use serde_json::json;
use support::{Fx, ok, open, refused};

fn today() -> i64 {
    (SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        / 86_400) as i64
}

fn read(fx: &Fx, rel: &str) -> String {
    std::fs::read_to_string(fx.root.path().join(rel)).unwrap_or_default()
}

fn files(fx: &Fx, dir: &str, prefix: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(fx.root.path().join(dir)) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.starts_with(prefix))
        .collect()
}

/// Proposes, replaces the blocks with two blocks today, and accepts.
async fn accept_plan(fx: &Fx) -> String {
    let day = iso_from_days(today());
    let prop = ok(
        fx,
        "lucubra_plan",
        json!({"action": "propose", "today": day}),
    )
    .await;
    let pid = prop["plan_id"].as_str().unwrap().to_owned();
    let blocks = json!([
        {"date": day, "subject": "torts", "unit": "negligence", "minutes": 25, "break_after": 5},
        {"date": day, "subject": "torts", "unit": "duty", "minutes": 20}
    ]);
    ok(
        fx,
        "lucubra_plan",
        json!({"action": "edit", "plan_id": pid, "blocks": blocks}),
    )
    .await;
    ok(
        fx,
        "lucubra_plan",
        json!({"action": "accept", "plan_id": pid, "overcommit_confirmed": true}),
    )
    .await;
    pid
}

#[tokio::test]
async fn s46_accepted_plan_writes_a_study_script_and_the_handoff_compares() {
    let fx = open("claude-ai").await;
    let pid = accept_plan(&fx).await;
    let scripts = files(&fx, "sessions", "SCRIPT_");
    assert_eq!(scripts.len(), 1, "{scripts:?}");
    let script = read(&fx, &format!("sessions/{}", scripts[0]));
    assert!(script.contains(&format!("Plan {pid}")), "{script}");
    assert!(script.contains("## Block 1: 18:00 to 18:25"), "{script}");
    assert!(script.contains("## Block 2: 18:30 to 18:50"), "{script}");
    assert!(
        script.contains("(../subjects/torts/negligence/UNIT.md)"),
        "{script}"
    );
    assert!(script.contains("- [ ] Break: 5 minutes"), "{script}");
    assert!(
        script.contains("A duty of care needs proximity."),
        "the strict item leads: {script}"
    );
    assert!(script.contains("## Answers"), "{script}");
    let block2 = script.split("## Block 2").nth(1).unwrap();
    let block2 = block2.split("## Answers").next().unwrap();
    let questions = block2
        .lines()
        .filter(|l| l.starts_with("  ") && l.contains(". "))
        .count();
    assert!(
        (3..=5).contains(&questions),
        "{questions} questions: {script}"
    );

    ok(
        &fx,
        "lucubra_session",
        json!({"action": "start", "plan_id": pid, "units": ["torts/negligence"]}),
    )
    .await;
    ok(
        &fx,
        "lucubra_session",
        json!({"action": "step", "unit": "torts/negligence", "minutes": 20}),
    )
    .await;
    ok(
        &fx,
        "lucubra_session",
        json!({"action": "step", "unit": "torts/remedies", "minutes": 5}),
    )
    .await;
    ok(
        &fx,
        "lucubra_session",
        json!({"action": "end", "summary": "Negligence done."}),
    )
    .await;
    let handoffs = files(&fx, "sessions", "SESSION_");
    let handoff = read(&fx, &format!("sessions/{}", handoffs[0]));
    assert!(handoff.contains("## Against the script"), "{handoff}");
    assert!(
        handoff.contains("- torts/negligence: planned 25 minutes, done 20"),
        "{handoff}"
    );
    assert!(
        handoff.contains("- torts/duty: planned 20 minutes, done 0"),
        "{handoff}"
    );
    assert!(
        handoff.contains("- torts/remedies: not in the script, done 5"),
        "{handoff}"
    );
}

#[tokio::test]
async fn s47_switching_style_changes_the_sequence_from_the_next_step() {
    let fx = open("claude-ai").await;
    assert_eq!(
        lucubra_mcp::envelope::VALID_A2_STYLES,
        lucubra_core::styles::ids().as_slice(),
        "the MCP style list must match the style files"
    );
    let start = ok(
        &fx,
        "lucubra_session",
        json!({"action": "start", "units": ["torts/negligence"], "style": "tutor"}),
    )
    .await;
    assert_eq!(start["guide"]["style"], "tutor");
    assert_eq!(
        start["guide"]["steps"],
        json!(["explain", "check", "advance", "recap"])
    );

    let tutor = ok(
        &fx,
        "lucubra_session",
        json!({"action": "step", "unit": "torts/negligence", "minutes": 5, "style": "tutor"}),
    )
    .await;
    assert_eq!(tutor["guide"]["style_changed"], false);
    let switched = ok(
        &fx,
        "lucubra_session",
        json!({"action": "step", "unit": "torts/negligence", "minutes": 5, "style": "tester"}),
    )
    .await;
    assert_eq!(switched["guide"]["style"], "tester");
    assert_eq!(switched["guide"]["style_changed"], true);
    assert_eq!(switched["guide"]["steps"][0], "question");
    let kept = ok(
        &fx,
        "lucubra_session",
        json!({"action": "step", "unit": "torts/negligence", "minutes": 5}),
    )
    .await;
    assert_eq!(
        kept["guide"]["style"], "tester",
        "the new style stays: {kept}"
    );

    let status = ok(&fx, "lucubra_status", json!({})).await;
    assert_eq!(
        status["metrics"]["steps_per_style"],
        json!({"tester": 1, "tutor": 1}),
        "{status}"
    );
    refused(
        &fx,
        "lucubra_session",
        json!({"action": "step", "unit": "torts/negligence", "minutes": 5, "style": "lecture"}),
        "invalid_arguments",
    )
    .await;
}

#[tokio::test]
async fn s54_no_reminders_without_opt_in() {
    let fx = open("claude-ai").await;
    accept_plan(&fx).await;
    let ics = read(&fx, "schedule/lucubra-plan.ics");
    assert!(ics.contains("BEGIN:VEVENT"), "{ics}");
    assert!(!ics.contains("VALARM"), "{ics}");
    let orient = ok(&fx, "lucubra_orient", json!({})).await;
    assert!(orient.get("reminders").is_none(), "{orient}");
}

#[tokio::test]
async fn s55_review_day_reminders_only() {
    let fx = support::open_with("claude-ai", |ws| {
        support::seed(ws);
        ws.append(
            "card.reviewed",
            json!({"card_id": "c1", "unit": "torts/negligence", "due_day": today() + 2}),
        )
        .unwrap();
    })
    .await;
    accept_plan(&fx).await;
    ok(
        &fx,
        "lucubra_profile",
        json!({"action": "preference", "field": "reminders_review_due", "value": true}),
    )
    .await;
    let ics = read(&fx, "schedule/lucubra-plan.ics");
    let review_date = iso_from_days(today() + 2).replace('-', "");
    let events: Vec<&str> = ics.split("BEGIN:VEVENT").skip(1).collect();
    let review: Vec<&&str> = events
        .iter()
        .filter(|e| e.contains("UID:review-"))
        .collect();
    assert_eq!(review.len(), 1, "{ics}");
    assert!(
        review[0].contains(&format!("DTSTART;VALUE=DATE:{review_date}")),
        "{ics}"
    );
    assert!(review[0].contains("BEGIN:VALARM"), "{ics}");
    for e in events.iter().filter(|e| !e.contains("UID:review-")) {
        assert!(!e.contains("VALARM"), "only review days alarm: {e}");
    }
    let orient = ok(&fx, "lucubra_orient", json!({})).await;
    assert_eq!(
        orient["reminders"]["opted_in"],
        json!(["review_due"]),
        "{orient}"
    );
    assert!(
        orient["reminders"]["scheduled_task_recipe"].is_string(),
        "{orient}"
    );

    ok(
        &fx,
        "lucubra_profile",
        json!({"action": "preference", "field": "reminders_study_blocks", "value": true}),
    )
    .await;
    let ics = read(&fx, "schedule/lucubra-plan.ics");
    let alarms = ics.matches("BEGIN:VALARM").count();
    assert_eq!(alarms, 3, "one review day and two study blocks: {ics}");
}

#[tokio::test]
async fn s56_preferences_file_is_projected_capped_and_respected() {
    let fx = open("claude-ai").await;
    refused(
        &fx,
        "lucubra_profile",
        json!({"action": "preference", "field": "diagnosis", "value": "x"}),
        "invalid_arguments",
    )
    .await;
    ok(
        &fx,
        "lucubra_profile",
        json!({"action": "preference", "field": "one_question_at_a_time", "value": true}),
    )
    .await;
    ok(
        &fx,
        "lucubra_profile",
        json!({"action": "preference", "field": "teaching_style", "value": "socratic"}),
    )
    .await;
    let prefs = read(&fx, "PREFERENCES.md");
    assert!(prefs.contains("- One question at a time: yes"), "{prefs}");
    assert!(prefs.chars().count() <= 2_400);

    let orient = ok(&fx, "lucubra_orient", json!({})).await;
    assert!(
        orient["preferences"]
            .to_string()
            .contains("One question at a time"),
        "{orient}"
    );
    let start = ok(
        &fx,
        "lucubra_session",
        json!({"action": "start", "units": ["torts/negligence"]}),
    )
    .await;
    assert_eq!(start["guide"]["one_question_at_a_time"], true, "{start}");
    assert_eq!(
        start["guide"]["style"], "socratic",
        "the preferred style applies: {start}"
    );

    let edited = format!("{prefs}- My own note: explain with diagrams\n");
    std::fs::write(fx.root.path().join("PREFERENCES.md"), &edited).unwrap();
    ok(
        &fx,
        "lucubra_profile",
        json!({"action": "preference", "field": "read_aloud", "value": true}),
    )
    .await;
    let preserved = fx.root.path().join(".lucubra/preserved");
    let kept = walk(&preserved)
        .into_iter()
        .any(|p| std::fs::read_to_string(p).is_ok_and(|t| t.contains("explain with diagrams")));
    assert!(
        kept,
        "the user's edit must be preserved before regeneration"
    );
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = entry.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}
