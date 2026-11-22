// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_practice`: FSRS practice and exam simulation validation (engine in M6).

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::budget::BudgetLimits;
use crate::envelope::{ProgressEntry, ToolError, ToolErrorCode};
use crate::tools::{
    ToolContext, ToolOutcome, check_keys, check_slug, opt_str, opt_u64, req_str, require_action,
};

/// JSON Schema arguments for `lucubra_practice`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PracticeArgs {
    /// Action: next, answer, or exam_sim.
    pub action: String,
    /// Optional subject slug filter.
    #[serde(default)]
    pub subject: Option<String>,
    /// Optional unit slug filter.
    #[serde(default)]
    pub unit: Option<String>,
    /// Practice mode: strict (default) or extended.
    #[serde(default)]
    pub mode: Option<String>,
    /// Maximum items in queue (1-20).
    #[serde(default)]
    pub limit: Option<u64>,
    /// Practice item identifier (for answer).
    #[serde(default)]
    pub item_id: Option<String>,
    /// User response payload (for answer).
    #[serde(default)]
    pub response: Option<Value>,
    /// Optional FSRS self-rating: again, hard, good, easy.
    #[serde(default)]
    pub rating: Option<String>,
    /// Exam id (for exam_sim).
    #[serde(default)]
    pub exam_id: Option<String>,
    /// Timed duration in minutes (5-300, for exam_sim).
    #[serde(default)]
    pub duration_minutes: Option<u64>,
    /// Number of items in exam_sim (1-100).
    #[serde(default)]
    pub count: Option<u64>,
    /// Optional completed-step checkpoints to log alongside this call.
    #[serde(default)]
    pub progress: Option<Vec<ProgressEntry>>,
}

pub fn execute(
    args: &Map<String, Value>,
    _ctx: &ToolContext<'_>,
    _limits: BudgetLimits,
) -> std::result::Result<ToolOutcome, ToolError> {
    let action = require_action(args, &["next", "answer", "exam_sim"])?;
    match action {
        "next" => {
            check_keys(
                args,
                &["action", "subject", "unit", "mode", "limit", "progress"],
                "lucubra_practice(next)",
            )?;
            if let Some(s) = opt_str(args, "subject")? {
                check_slug("subject", s)?;
            }
            if let Some(u) = opt_str(args, "unit")? {
                check_slug("unit", u)?;
            }
            if let Some(m) = opt_str(args, "mode")?
                && !matches!(m, "strict" | "extended")
            {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "mode must be 'strict' or 'extended'",
                ));
            }
            let _limit = opt_u64(
                args,
                "limit",
                1,
                20,
                "limit must be an integer between 1 and 20",
            )?;
        }
        "answer" => {
            check_keys(
                args,
                &["action", "item_id", "response", "rating", "progress"],
                "lucubra_practice(answer)",
            )?;
            let _item_id = req_str(
                args,
                "item_id",
                "provide both 'item_id' and 'response' for action 'answer'",
            )?;
            if args.get("response").is_none_or(Value::is_null) {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "provide both 'item_id' and 'response' for action 'answer'",
                ));
            }
            if let Some(r) = opt_str(args, "rating")?
                && !matches!(r, "again" | "hard" | "good" | "easy")
            {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "rating must be one of: again, hard, good, easy",
                ));
            }
        }
        "exam_sim" => {
            check_keys(
                args,
                &[
                    "action",
                    "subject",
                    "exam_id",
                    "mode",
                    "duration_minutes",
                    "count",
                    "progress",
                ],
                "lucubra_practice(exam_sim)",
            )?;
            if let Some(s) = opt_str(args, "subject")? {
                check_slug("subject", s)?;
            }
            let _eid = opt_str(args, "exam_id")?;
            if let Some(m) = opt_str(args, "mode")?
                && !matches!(m, "strict" | "extended")
            {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "mode must be 'strict' or 'extended'",
                ));
            }
            let _dur = opt_u64(
                args,
                "duration_minutes",
                5,
                300,
                "duration_minutes must be an integer between 5 and 300",
            )?;
            let _cnt = opt_u64(
                args,
                "count",
                1,
                100,
                "count must be an integer between 1 and 100",
            )?;
        }
        _ => unreachable!(),
    }

    Err(ToolError::new(
        ToolErrorCode::NotImplemented,
        "Practice and spaced repetition (FSRS) are implemented in milestone M6.",
    ))
}
