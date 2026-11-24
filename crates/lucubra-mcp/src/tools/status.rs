// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_status`: live workspace counts, logged minutes, and honest retention placeholders (`S28`).

use lucubra_core::state::col;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::budget::BudgetLimits;
use crate::envelope::{ProgressEntry, ToolError, ToolErrorCode};
use crate::tools::{ToolContext, ToolOutcome, check_keys, check_slug, opt_bool, opt_str};

/// JSON Schema arguments for `lucubra_status`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatusArgs {
    /// Optional subject slug filter.
    #[serde(default)]
    pub subject: Option<String>,
    /// Include sleep-vs-retention placeholder section (default: true).
    #[serde(default)]
    pub include_sleep: Option<bool>,
    /// Optional completed-step checkpoints to log alongside this call.
    #[serde(default)]
    pub progress: Option<Vec<ProgressEntry>>,
}

/// Exam readiness per subject: mean practice mastery of its units, from
/// strict items only (S27). Subjects without practice are left out.
fn readiness(state: &lucubra_core::state::State) -> Value {
    let today = crate::tools::practice::today_day();
    let mut units: std::collections::BTreeSet<String> = state
        .collection(col::UNITS)
        .map(|(k, _)| k.clone())
        .collect();
    units.extend(
        state
            .collection(col::ITEMS)
            .filter_map(|(_, i)| i.get("unit").and_then(Value::as_str).map(str::to_owned)),
    );
    let mut by_subject: std::collections::BTreeMap<String, Vec<f64>> = Default::default();
    for unit in units {
        if let Some(m) = lucubra_core::planner::unit_mastery(state, &unit, today) {
            let subject = unit.split('/').next().unwrap_or_default().to_owned();
            by_subject.entry(subject).or_default().push(m);
        }
    }
    by_subject
        .into_iter()
        .map(|(s, v)| (s, json!(v.iter().sum::<f64>() / v.len() as f64)))
        .collect::<Map<String, Value>>()
        .into()
}

pub fn execute(
    args: &Map<String, Value>,
    ctx: &ToolContext<'_>,
    limits: BudgetLimits,
) -> std::result::Result<ToolOutcome, ToolError> {
    check_keys(
        args,
        &["subject", "include_sleep", "progress"],
        "lucubra_status",
    )?;
    let subject_filter = opt_str(args, "subject")?;
    if let Some(s) = subject_filter {
        check_slug("subject", s)?;
    }
    let include_sleep =
        opt_bool(args, "include_sleep", "include_sleep must be true or false")?.unwrap_or(true);

    let ws_status = ctx.ws.status().map_err(|e| {
        ToolError::new(
            ToolErrorCode::WorkspaceError,
            format!("failed reading workspace status: {e}"),
        )
    })?;
    let state = ctx
        .ws
        .replay()
        .map_err(|e| {
            ToolError::new(
                ToolErrorCode::WorkspaceError,
                format!("failed replaying workspace state: {e}"),
            )
        })?
        .state;

    let subjects_count = state.collection(col::SUBJECTS).count();
    let units_count = state
        .collection(col::UNITS)
        .filter(|(k, _)| match subject_filter {
            Some(s) => k.starts_with(&format!("{s}/")),
            None => true,
        })
        .count();

    let mut extracted = 0usize;
    let mut pending = 0usize;
    let mut failed = 0usize;
    let mut online_only = 0usize;
    for (_, file) in state.collection(col::FILES) {
        match file.get("status").and_then(Value::as_str) {
            Some("extracted") => extracted += 1,
            Some("pending") => {
                pending += 1;
                if file.get("code").and_then(Value::as_str) == Some("online_only") {
                    online_only += 1;
                }
            }
            Some("failed") => failed += 1,
            _ => {}
        }
    }

    let mut sessions_count = 0usize;
    let mut logged_minutes_total = 0u64;
    for (k, sess) in state.collection(col::SESSIONS) {
        if k != "_unattached" {
            sessions_count += 1;
        }
        logged_minutes_total += sess
            .get("logged_minutes")
            .and_then(Value::as_u64)
            .unwrap_or(0);
    }

    let sleep_retention = if include_sleep {
        json!({
            "available": false,
            "paired_days": 0,
            "required_days": 30,
            "message": "not enough data yet",
        })
    } else {
        Value::Null
    };

    let questions = crate::tools::assessment_questions(ctx.ws, &state);
    let next = (!questions.is_empty()).then(|| "confirm_assessment".to_owned());
    Ok(ToolOutcome {
        data: json!({
            "workspace_id": ws_status.workspace_id,
            "events_count": ws_status.events,
            "subjects_count": subjects_count,
            "units_count": units_count,
            "files": {
                "extracted": extracted,
                "pending": pending,
                "failed": failed,
                "online_only": online_only,
            },
            "sessions_count": sessions_count,
            "logged_minutes_total": logged_minutes_total,
            "host_renders_artifacts": ctx.host.profile.renders_artifacts,
            "sleep_retention": sleep_retention,
            "readiness": readiness(&state),
            "needs_user": questions,
        }),
        next,
        warnings: Vec::new(),
        per_call_limit: limits.response_limit,
        suppress_wrap_up: false,
        active_session_override: None,
    })
}
