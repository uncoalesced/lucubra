// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_orient`: fast session orientation (`SPEC 8.2`, `S36`, `S37`).

use std::collections::BTreeMap;
use std::fs;

use lucubra_core::state::{State, col};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::budget::{BudgetLimits, estimate_tokens, truncate_chars};
use crate::envelope::{ProgressEntry, ToolError, ToolErrorCode};
use crate::instructions::INSTRUCTIONS_VERSION;
use crate::material::{read_preferences_summary, read_sources_summary};
use crate::tools::{ToolContext, ToolOutcome, check_keys};

/// JSON Schema arguments for `lucubra_orient`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OrientArgs {
    /// Optional completed-step checkpoints to log alongside this call.
    #[serde(default)]
    pub progress: Option<Vec<ProgressEntry>>,
}

const REQUIRED_ONBOARDING_FIELDS: &[&str] = &[
    "level",
    "programme",
    "term_system",
    "current_term",
    "start_year",
    "completion_year",
    "subjects",
    "exams",
];

fn count_inbox_entries(root: &std::path::Path) -> usize {
    let inbox = root.join("inbox");
    let Ok(rd) = fs::read_dir(inbox) else {
        return 0;
    };
    rd.filter_map(|e| e.ok())
        .filter(|e| !e.file_name().to_str().is_some_and(|n| n.starts_with('.')))
        .count()
}

pub fn missing_required_fields(state: &State) -> Vec<&'static str> {
    let profile = state.collections.get(col::PROFILE);
    let skipped = profile
        .and_then(|p| p.get("skipped_fields"))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();

    let mut missing = Vec::new();
    for &field in REQUIRED_ONBOARDING_FIELDS {
        if skipped.contains(&field) {
            continue;
        }
        let present = match field {
            "subjects" => {
                !state
                    .collections
                    .get(col::SUBJECTS)
                    .is_none_or(BTreeMap::is_empty)
                    || profile
                        .and_then(|p| p.get("subjects"))
                        .and_then(Value::as_array)
                        .is_some_and(|a| !a.is_empty())
            }
            "exams" => {
                !state
                    .collections
                    .get(col::EXAMS)
                    .is_none_or(BTreeMap::is_empty)
                    || profile
                        .and_then(|p| p.get("exams"))
                        .and_then(Value::as_array)
                        .is_some_and(|a| !a.is_empty())
            }
            _ => profile
                .and_then(|p| p.get(field))
                .is_some_and(|v| !v.is_null()),
        };
        if !present {
            missing.push(field);
        }
    }
    missing
}

pub fn execute(
    args: &Map<String, Value>,
    ctx: &ToolContext<'_>,
    limits: BudgetLimits,
) -> std::result::Result<ToolOutcome, ToolError> {
    check_keys(args, &["progress"], "lucubra_orient")?;
    let recovered = crate::tools::session::recover_idle(ctx.ws).map_err(|e| {
        ToolError::new(
            ToolErrorCode::WorkspaceError,
            format!("session recovery failed: {e}"),
        )
    })?;

    let state = ctx
        .ws
        .replay()
        .map_err(|e| {
            ToolError::new(
                ToolErrorCode::WorkspaceError,
                format!("failed to replay workspace state: {e}"),
            )
        })?
        .state;

    let workspace_id = state
        .get(col::WORKSPACE, "id")
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    let inbox_count = count_inbox_entries(ctx.ws.root());
    let mut preferences = read_preferences_summary(ctx.ws.root());
    let sources = read_sources_summary(ctx.ws.root());

    let mut extracted = 0usize;
    let mut pending = 0usize;
    let mut failed = 0usize;
    let mut needs_vision_files = 0usize;
    for (_, file) in state.collection(col::FILES) {
        match file.get("status").and_then(Value::as_str) {
            Some("extracted") => {
                extracted += 1;
                if file
                    .get("needs_vision")
                    .and_then(Value::as_array)
                    .is_some_and(|a| !a.is_empty())
                {
                    needs_vision_files += 1;
                }
            }
            Some("pending") => pending += 1,
            Some("failed") => failed += 1,
            _ => {}
        }
    }

    let mut all_subjects: Vec<Value> = Vec::new();
    for (slug, subj) in state.collection(col::SUBJECTS) {
        let title = subj
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or(slug.as_str());
        let next_exam = state
            .collection(col::EXAMS)
            .filter(|(_, ex)| ex.get("subject").and_then(Value::as_str) == Some(slug.as_str()))
            .filter_map(|(_, ex)| ex.get("date").and_then(Value::as_str))
            .min();
        all_subjects.push(json!({
            "slug": slug,
            "title": title,
            "next_exam": next_exam,
        }));
    }
    let total_subjects = all_subjects.len();

    // An open session, or the latest one if it was closed as recovered, is
    // offered for resumption (SPEC section 15).
    // The latest session (ids are ULIDs, so key order is start order) is
    // offered for resumption when it is open or was closed as recovered.
    let interrupted_session = state
        .collection(col::SESSIONS)
        .filter(|(k, _)| k.as_str() != "_unattached")
        .last()
        .filter(|(k, v)| {
            let status = v.get("status").and_then(Value::as_str);
            let reason = v.get("reason").and_then(Value::as_str);
            status == Some("active")
                || recovered.contains(*k)
                || (status == Some("ended") && reason == Some("recovered"))
        })
        .map(|(k, v)| {
            json!({
                "session_id": k,
                "units": v.get("units"),
                "style": v.get("style"),
                "logged_minutes": v.get("logged_minutes"),
            })
        });

    let mut compact_profile = Map::new();
    if let Some(prof) = state.collections.get(col::PROFILE) {
        for k in [
            "level",
            "programme",
            "term_system",
            "current_term",
            "schedule_planning",
            "integrity_mode",
            "teaching_style",
            "plan_tier",
            "local_model",
        ] {
            if let Some(v) = prof.get(k) {
                compact_profile.insert(k.to_owned(), v.clone());
            }
        }
    }

    let missing = missing_required_fields(&state);
    let suggested_next = if !missing.is_empty() {
        "onboard"
    } else if inbox_count > 0 {
        "scan_inbox"
    } else {
        "ready"
    };

    // Enforce orient_limit aggressively so S37 stays bounded even with 50 subjects.
    let mut cap = all_subjects.len().min(12);
    let mut warnings = Vec::new();
    let max_data_tokens = limits.orient_limit.saturating_sub(140);

    let data = loop {
        let shown: Vec<Value> = all_subjects.iter().take(cap).cloned().collect();
        let subjects_truncated = cap < total_subjects;
        let candidate = json!({
            "instructions_version": INSTRUCTIONS_VERSION,
            "workspace_id": workspace_id,
            "host": {
                "id": ctx.host.profile.id,
                "tier": crate::budget::resolve_tier(
                    state.collections.get(col::PROFILE),
                    ctx.host.profile.id,
                    ctx.host.profile.default_tier,
                ).as_str(),
                "renders_artifacts": ctx.host.profile.renders_artifacts,
                "capabilities": {
                    "elicitation_form": ctx.host.capabilities.elicitation_form,
                    "elicitation_url": ctx.host.capabilities.elicitation_url,
                    "sampling": ctx.host.capabilities.sampling,
                    "roots": ctx.host.capabilities.roots,
                },
            },
            "profile": compact_profile,
            "preferences": preferences,
            "sources_configured": sources.configured,
            "sources_domain_count": sources.domain_count,
            "subjects": shown,
            "total_subjects": total_subjects,
            "subjects_truncated": subjects_truncated,
            "inbox_count": inbox_count,
            "files": {
                "extracted": extracted,
                "pending": pending,
                "failed": failed,
                "needs_vision_files": needs_vision_files,
            },
            "interrupted_session": interrupted_session,
            "next_proposed_session": Value::Null,
        });
        let s = serde_json::to_string(&candidate).unwrap_or_default();
        if estimate_tokens(&s) <= max_data_tokens {
            if subjects_truncated {
                warnings.push(format!(
                    "Showing {cap} of {total_subjects} subjects to fit tier orient budget ({} tokens).",
                    limits.orient_limit
                ));
            }
            break candidate;
        }
        if cap > 3 {
            cap /= 2;
            continue;
        }
        if let Some(ref mut pref) = preferences
            && pref.text.chars().count() > 240
        {
            let (shorter, _) = truncate_chars(&pref.text, 240);
            pref.text = shorter;
            pref.truncated = true;
            continue;
        }
        if cap > 0 {
            cap = 0;
            continue;
        }
        break candidate;
    };

    Ok(ToolOutcome {
        data,
        next: Some(suggested_next.to_owned()),
        warnings,
        per_call_limit: limits.orient_limit,
        suppress_wrap_up: false,
        active_session_override: None,
    })
}
