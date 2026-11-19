// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_export`: session brief export (live) and host-aware deferred export stubs (`A6`).

use lucubra_core::state::col;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::budget::BudgetLimits;
use crate::envelope::{ProgressEntry, ToolError, ToolErrorCode};
use crate::tools::{
    ToolContext, ToolOutcome, check_keys, check_slug, opt_str, req_str, require_action,
};

/// JSON Schema arguments for `lucubra_export`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportArgs {
    /// Action: brief, pack, anki, ics, dashboard, or mock_test.
    pub action: String,
    /// Subject slug (required for mock_test; optional for others).
    #[serde(default)]
    pub subject: Option<String>,
    /// Session id filter (for brief).
    #[serde(default)]
    pub session_id: Option<String>,
    /// Exam id filter (for mock_test).
    #[serde(default)]
    pub exam_id: Option<String>,
    /// Output format: apkg or tsv (for anki); md or pdf (for pack/mock_test).
    #[serde(default)]
    pub format: Option<String>,
    /// Optional completed-step checkpoints to log alongside this call.
    #[serde(default)]
    pub progress: Option<Vec<ProgressEntry>>,
}

pub fn execute(
    args: &Map<String, Value>,
    ctx: &ToolContext<'_>,
    limits: BudgetLimits,
) -> std::result::Result<ToolOutcome, ToolError> {
    let action = require_action(
        args,
        &["brief", "pack", "anki", "ics", "dashboard", "mock_test"],
    )?;
    check_keys(
        args,
        &[
            "action",
            "subject",
            "session_id",
            "exam_id",
            "format",
            "progress",
        ],
        "lucubra_export",
    )?;
    let _eid = opt_str(args, "exam_id")?;
    let format = opt_str(args, "format")?;

    if action == "mock_test" {
        let subj = req_str(
            args,
            "subject",
            "provide 'subject' slug for action 'mock_test'",
        )?;
        check_slug("subject", subj)?;
        if let Some(fmt) = format
            && !matches!(fmt, "md" | "pdf")
        {
            return Err(ToolError::new(
                ToolErrorCode::InvalidArguments,
                "format for action 'mock_test' must be 'md' or 'pdf'",
            ));
        }
        return Err(ToolError::new(
            ToolErrorCode::NotImplemented,
            "Mock test export (lucubra_export mock_test) is implemented in milestone M6.",
        )
        .with_detail(ctx.host.profile.export_recipe));
    }

    if let Some(s) = opt_str(args, "subject")? {
        check_slug("subject", s)?;
    }

    match action {
        "brief" => {
            let state = ctx
                .ws
                .replay()
                .map_err(|e| {
                    ToolError::new(
                        ToolErrorCode::WorkspaceError,
                        format!("failed reading workspace state: {e}"),
                    )
                })?
                .state;
            let requested_sid = opt_str(args, "session_id")?;
            let chosen = if let Some(sid) = requested_sid {
                state
                    .get(col::SESSIONS, sid)
                    .map(|v| (sid.to_owned(), v.clone()))
            } else {
                state
                    .collection(col::SESSIONS)
                    .filter(|(k, _)| k.as_str() != "_unattached")
                    .last()
                    .map(|(k, v)| (k.clone(), v.clone()))
            };
            let markdown = match &chosen {
                Some((sid, s)) => {
                    let mins = s.get("logged_minutes").and_then(Value::as_u64).unwrap_or(0);
                    let steps = s.get("steps_count").and_then(Value::as_u64).unwrap_or(0);
                    let sum = s
                        .get("summary")
                        .and_then(Value::as_str)
                        .unwrap_or("No summary recorded.");
                    format!(
                        "# Session Brief ({sid})\n\n- Logged minutes: {mins}\n- Steps completed: {steps}\n\n{sum}\n"
                    )
                }
                None => "# Workspace Brief\n\nNo study sessions recorded yet.\n".to_owned(),
            };
            Ok(ToolOutcome {
                data: json!({
                    "session_id": chosen.as_ref().map(|(k, _)| k),
                    "host": ctx.host.profile.id,
                    "export_recipe": ctx.host.profile.export_recipe,
                    "markdown": markdown,
                }),
                next: None,
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: None,
            })
        }
        "anki" => {
            if let Some(fmt) = format
                && !matches!(fmt, "apkg" | "tsv")
            {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "format for action 'anki' must be 'apkg' or 'tsv'",
                ));
            }
            Err(ToolError::new(
                ToolErrorCode::NotImplemented,
                "Anki export is implemented in milestone M6.",
            )
            .with_detail(ctx.host.profile.export_recipe))
        }
        "ics" => Err(ToolError::new(
            ToolErrorCode::NotImplemented,
            "Calendar (.ics) export is implemented in milestone M5.",
        )
        .with_detail(ctx.host.profile.export_recipe)),
        "pack" => {
            if let Some(fmt) = format
                && !matches!(fmt, "md" | "pdf")
            {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "format for action 'pack' must be 'md' or 'pdf'",
                ));
            }
            Err(ToolError::new(
                ToolErrorCode::NotImplemented,
                "Portable Study-Pack compilation is implemented in milestone M7.",
            )
            .with_detail(ctx.host.profile.export_recipe))
        }
        "dashboard" => Err(ToolError::new(
            ToolErrorCode::NotImplemented,
            "Self-contained HTML dashboard export is implemented in milestone M8.",
        )
        .with_detail(ctx.host.profile.export_recipe)),
        _ => unreachable!(),
    }
}
