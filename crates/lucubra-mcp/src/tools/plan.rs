// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_plan`: validation and `schedule_planning == false` guard (`S02`; scheduler in M5).

use lucubra_core::state::col;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::budget::BudgetLimits;
use crate::envelope::{ProgressEntry, ToolError, ToolErrorCode};
use crate::tools::{
    ToolContext, ToolOutcome, check_keys, opt_bool, opt_u64, req_str, require_action,
};

/// JSON Schema arguments for `lucubra_plan`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanArgs {
    /// Action: propose, edit, accept, or horizon.
    pub action: String,
    /// Number of days in planning window (1-30).
    #[serde(default)]
    pub days: Option<u64>,
    /// Available study time in minutes (10-960).
    #[serde(default)]
    pub available_minutes: Option<u64>,
    /// RFC 3339 busy calendar blocks.
    #[serde(default)]
    pub busy_blocks: Option<Vec<Value>>,
    /// Plan identifier for edit or accept.
    #[serde(default)]
    pub plan_id: Option<String>,
    /// Edited study blocks array (for edit).
    #[serde(default)]
    pub blocks: Option<Vec<Value>>,
    /// Explicit user confirmation when accepting an overcommitted plan (S14).
    #[serde(default)]
    pub overcommit_confirmed: Option<bool>,
    /// Optional completed-step checkpoints to log alongside this call.
    #[serde(default)]
    pub progress: Option<Vec<ProgressEntry>>,
}

pub fn execute(
    args: &Map<String, Value>,
    ctx: &ToolContext<'_>,
    limits: BudgetLimits,
) -> std::result::Result<ToolOutcome, ToolError> {
    let action = require_action(args, &["propose", "edit", "accept", "horizon"])?;
    match action {
        "propose" | "horizon" => {
            check_keys(
                args,
                &[
                    "action",
                    "days",
                    "available_minutes",
                    "busy_blocks",
                    "progress",
                ],
                "lucubra_plan",
            )?;
            let _days = opt_u64(
                args,
                "days",
                1,
                30,
                "days must be an integer between 1 and 30",
            )?;
            let _mins = opt_u64(
                args,
                "available_minutes",
                10,
                960,
                "available_minutes must be an integer between 10 and 960",
            )?;
            if let Some(raw) = args.get("busy_blocks")
                && !raw.is_array()
            {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "busy_blocks must be an array of {start, end, label?} objects",
                ));
            }
            if matches!(action, "propose" | "horizon")
                && let Ok(r) = ctx.ws.replay()
                && r.state.get(col::PROFILE, "schedule_planning") == Some(&Value::Bool(false))
            {
                return Ok(ToolOutcome {
                    data: json!({
                        "schedule_planning": false,
                        "message": "Schedule planning is turned off in this profile; focus on in-session sequencing only (S02).",
                    }),
                    next: Some("in_session_only".into()),
                    warnings: Vec::new(),
                    per_call_limit: limits.response_limit,
                    suppress_wrap_up: false,
                    active_session_override: None,
                });
            }
        }
        "edit" => {
            check_keys(
                args,
                &["action", "plan_id", "blocks", "progress"],
                "lucubra_plan(edit)",
            )?;
            let _pid = req_str(
                args,
                "plan_id",
                "provide 'plan_id' returned by lucubra_plan(action=\"propose\")",
            )?;
            if !args.get("blocks").is_some_and(Value::is_array) {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "provide 'blocks' array for lucubra_plan(action=\"edit\")",
                ));
            }
        }
        "accept" => {
            check_keys(
                args,
                &["action", "plan_id", "overcommit_confirmed", "progress"],
                "lucubra_plan(accept)",
            )?;
            let _pid = req_str(
                args,
                "plan_id",
                "provide 'plan_id' returned by lucubra_plan(action=\"propose\")",
            )?;
            let _ov = opt_bool(
                args,
                "overcommit_confirmed",
                "overcommit_confirmed must be true or false",
            )?;
        }
        _ => unreachable!(),
    }

    Err(ToolError::new(
        ToolErrorCode::NotImplemented,
        "Study planning and scheduling are implemented in milestone M5; use lucubra_read and lucubra_session to study directly for now.",
    ))
}
