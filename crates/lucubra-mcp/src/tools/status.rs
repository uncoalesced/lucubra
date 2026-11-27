// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_status`: live workspace counts, logged minutes, dashboard metrics and
//! the sleep and retention view (SPEC 19, `S28`).

use lucubra_core::metrics;
use lucubra_core::planner::PlannerConfig;
use lucubra_core::replay::Replay;
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
    /// Include the sleep and retention view (default: true). It stays hidden
    /// until 30 days have both a check-in and review data.
    #[serde(default)]
    pub include_sleep: Option<bool>,
    /// Return full metric series (per-unit mastery, retention curve, 12 weeks)
    /// for host artifacts (default: false).
    #[serde(default)]
    pub detail: Option<bool>,
    /// Optional completed-step checkpoints to log alongside this call.
    #[serde(default)]
    pub progress: Option<Vec<ProgressEntry>>,
}

/// Dashboard metrics (SPEC 19). The compact form fits the Free budget; `detail`
/// adds the full series for host artifacts.
fn metrics_block(replay: &Replay, today: i64, detail: bool) -> Value {
    let cfg = PlannerConfig::default();
    let state = &replay.state;
    let pack = lucubra_core::domain::pack_for_state(state);
    let mut exams = metrics::readiness_per_exam(state, today);
    for exam in &mut exams {
        let kind = exam["exam"]
            .as_str()
            .and_then(|id| state.get(col::EXAMS, id))
            .and_then(|e| e.get("exam_type").or_else(|| e.get("type")))
            .and_then(Value::as_str);
        if let Some(kind) = kind {
            exam["exam_type"] = json!(kind);
            exam["method"] = json!(lucubra_core::domain::method_for(kind, pack));
        }
    }
    let mut out = json!({
        "exams": exams,
        "weeks": metrics::consistency(replay, today, if detail { 12 } else { 4 }),
        "plan_adherence": metrics::plan_adherence(replay, today)
            .map(|(share, days)| json!({"share": share, "days": days})),
        "capacity_minutes": metrics::capacity(&cfg, state),
        "focus_span_minutes": metrics::focus_span(&cfg, replay),
        "minutes_per_subject": metrics::time_per_subject(replay),
        "steps_per_style": metrics::style_use(replay),
    });
    if detail {
        out["mastery_per_unit"] = json!(metrics::mastery_per_unit(state, today));
        out["retention_curve"] = json!(metrics::retention_curve(replay));
    }
    out
}

pub fn execute(
    args: &Map<String, Value>,
    ctx: &ToolContext<'_>,
    limits: BudgetLimits,
) -> std::result::Result<ToolOutcome, ToolError> {
    check_keys(
        args,
        &["subject", "include_sleep", "detail", "progress"],
        "lucubra_status",
    )?;
    let subject_filter = opt_str(args, "subject")?;
    if let Some(s) = subject_filter {
        check_slug("subject", s)?;
    }
    let include_sleep =
        opt_bool(args, "include_sleep", "include_sleep must be true or false")?.unwrap_or(true);
    let detail = opt_bool(args, "detail", "detail must be true or false")?.unwrap_or(false);

    let ws_status = ctx.ws.status().map_err(|e| {
        ToolError::new(
            ToolErrorCode::WorkspaceError,
            format!("failed reading workspace status: {e}"),
        )
    })?;
    let replay = ctx.ws.replay().map_err(|e| {
        ToolError::new(
            ToolErrorCode::WorkspaceError,
            format!("failed replaying workspace state: {e}"),
        )
    })?;
    let state = &replay.state;

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
        metrics::sleep_view(&replay)
    } else {
        Value::Null
    };
    let today = crate::tools::practice::today_day();
    let metrics = metrics_block(&replay, today, detail);

    let questions = crate::tools::assessment_questions(ctx.ws, state);
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
            "readiness": metrics::readiness_per_subject(state, today),
            "metrics": metrics,
            "domain_pack": lucubra_core::domain::pack_for_state(state).map(|p| json!({
                "id": p.meta.id,
                "name": p.meta.name,
                "questions": lucubra_core::domain::pending_questions(state),
            })),
            "recommendations": lucubra_core::domain::recommendations(state),
            "needs_user": questions,
        }),
        next,
        warnings: Vec::new(),
        per_call_limit: limits.response_limit,
        suppress_wrap_up: false,
        active_session_override: None,
    })
}
