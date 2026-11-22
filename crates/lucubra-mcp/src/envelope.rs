// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Response JSON envelopes, error codes (`S23`), budget wrapping, and
//! pre-validated `progress` checkpoints (`SPEC 8.2, 15`).

use lucubra_core::Workspace;
use lucubra_core::projection::is_slug;
use rmcp::model::{CallToolResponse, CallToolResult, ContentBlock};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::budget::{BudgetInfo, BudgetLimits, estimate_tokens};

/// Supported v1 teaching styles (Addendum A2).
pub const VALID_A2_STYLES: &[&str] = &[
    "tutor",
    "tester",
    "explainer",
    "socratic",
    "drill",
    "viva",
    "exam_sim",
    "revision",
];

/// Machine-readable error codes emitted inside `{ok: false, code, fix, ...}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolErrorCode {
    /// Missing required field, wrong JSON type, unknown key, or out-of-range value.
    InvalidArguments,
    /// Missing or unknown `action` variant.
    InvalidAction,
    /// Unrecognized `lucubra_*` tool name.
    UnknownTool,
    /// Subject or unit fails slug validation (including Windows reserved names).
    InvalidSlug,
    /// Path traversal, absolute path, `.lucubra/`, or symlink escape.
    InvalidPath,
    /// Referenced subject, unit, file, locator, transcript, or session was not found.
    NotFound,
    /// Precondition unmet (e.g., `step` without active session).
    InvalidState,
    /// Strict-mode grounding failure.
    GroundingFailed,
    /// Tutor mode blocked generating a solution for an assessment-tagged file.
    IntegrityRejected,
    /// Domain missing from `SOURCES.md`.
    UntrustedDomain,
    /// Tool action scoped to a later milestone (M4-M8).
    NotImplemented,
    /// Underlying workspace, SQLite, I/O, or worker failure.
    WorkspaceError,
}

/// Structured tool-level error holding a single actionable `fix` line (S23).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolError {
    /// Error code.
    pub code: ToolErrorCode,
    /// Single-line actionable fix instruction for the caller.
    pub fix: String,
    /// Optional supplementary context when distinct from `fix`.
    pub detail: Option<String>,
}

impl ToolError {
    /// Builds a structured error without extra detail.
    pub fn new(code: ToolErrorCode, fix: impl Into<String>) -> Self {
        Self {
            code,
            fix: fix.into().replace(['\r', '\n'], " "),
            detail: None,
        }
    }

    /// Attaches optional short context distinct from `fix`.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// Success response envelope serialized as compact JSON in MCP `CallToolResult`.
#[derive(Debug, Clone, Serialize)]
pub struct SuccessEnvelope {
    /// Always `true`.
    pub ok: bool,
    /// Tool-specific structured payload.
    pub data: Value,
    /// Suggested next action (forced to `"wrap_up"` once session budget hits `limit`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    /// Active session budget telemetry (`{served, limit}`).
    pub budget: BudgetInfo,
    /// Non-fatal warnings.
    pub warnings: Vec<String>,
}

/// Error response envelope serialized inside `CallToolResult::error` (S23).
#[derive(Debug, Clone, Serialize)]
pub struct ErrorEnvelope {
    /// Always `false`.
    pub ok: bool,
    /// Machine-readable error code.
    pub code: ToolErrorCode,
    /// Single-line actionable fix instruction.
    pub fix: String,
    /// Optional detail when distinct from `fix`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Active session budget telemetry (`{served, limit}`).
    pub budget: BudgetInfo,
    /// Non-fatal warnings.
    pub warnings: Vec<String>,
}

/// One checkpoint entry parsed from the optional `progress` array on any tool call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProgressEntry {
    /// Activity category (`step`, `recall`, `reading`, `practice`, `break`, `other`).
    pub kind: String,
    /// Target `<subject>/<unit>` using valid lowercase slugs.
    pub unit: String,
    /// Positive study duration in whole minutes (`1..=480`).
    pub minutes: u32,
    /// Optional short progress note (at most 500 characters).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Optional Addendum A2 teaching style identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
}

/// Pre-validates an optional `progress` field from raw JSON arguments *before* running
/// any primary tool logic.
pub fn parse_progress_array(
    args: &Map<String, Value>,
) -> std::result::Result<Vec<ProgressEntry>, ToolError> {
    let Some(raw) = args.get("progress") else {
        return Ok(Vec::new());
    };
    let Some(items) = raw.as_array() else {
        return Err(ToolError::new(
            ToolErrorCode::InvalidArguments,
            "progress must be an array of {kind, unit, minutes, note?, style?} objects",
        ));
    };
    let mut parsed = Vec::with_capacity(items.len());
    for (idx, item) in items.iter().enumerate() {
        let Some(obj) = item.as_object() else {
            return Err(ToolError::new(
                ToolErrorCode::InvalidArguments,
                format!("progress[{idx}] must be an object with {{kind, unit, minutes}}"),
            ));
        };
        for key in obj.keys() {
            if !matches!(key.as_str(), "kind" | "unit" | "minutes" | "note" | "style") {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    format!(
                        "progress[{idx}] has unknown field '{key}'; allowed keys: kind, unit, minutes, note, style"
                    ),
                ));
            }
        }

        let Some(kind) = obj.get("kind").and_then(Value::as_str).map(str::trim) else {
            return Err(ToolError::new(
                ToolErrorCode::InvalidArguments,
                format!("progress[{idx}].kind must be a non-empty string (e.g., \"step\")"),
            ));
        };
        if kind.is_empty() || kind.chars().count() > 32 {
            return Err(ToolError::new(
                ToolErrorCode::InvalidArguments,
                format!("progress[{idx}].kind must be 1 to 32 characters"),
            ));
        }

        let Some(unit) = obj.get("unit").and_then(Value::as_str).map(str::trim) else {
            return Err(ToolError::new(
                ToolErrorCode::InvalidArguments,
                format!(
                    "progress[{idx}].unit must be a '<subject>/<unit>' string using lowercase slugs"
                ),
            ));
        };
        if !validate_unit_ref(unit) {
            return Err(ToolError::new(
                ToolErrorCode::InvalidSlug,
                format!(
                    "progress[{idx}].unit must be '<subject>/<unit>' using lowercase slugs (1-64 a-z0-9_-)"
                ),
            ));
        }

        let Some(min_u64) = obj.get("minutes").and_then(Value::as_u64) else {
            return Err(ToolError::new(
                ToolErrorCode::InvalidArguments,
                format!("progress[{idx}].minutes must be an integer between 1 and 480"),
            ));
        };
        let Ok(minutes) = u32::try_from(min_u64) else {
            return Err(ToolError::new(
                ToolErrorCode::InvalidArguments,
                format!("progress[{idx}].minutes must be an integer between 1 and 480"),
            ));
        };
        if !(1..=480).contains(&minutes) {
            return Err(ToolError::new(
                ToolErrorCode::InvalidArguments,
                format!("progress[{idx}].minutes must be an integer between 1 and 480"),
            ));
        }

        let note = match obj.get("note") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => {
                let t = s.trim();
                if t.chars().count() > 500 {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        format!("progress[{idx}].note exceeds 500 characters"),
                    ));
                }
                (!t.is_empty()).then(|| t.to_owned())
            }
            Some(_) => {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    format!("progress[{idx}].note must be a string"),
                ));
            }
        };

        let style = match obj.get("style") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => {
                let t = s.trim();
                if !VALID_A2_STYLES.contains(&t) {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        format!(
                            "progress[{idx}].style must be one of: {}",
                            VALID_A2_STYLES.join(", ")
                        ),
                    ));
                }
                Some(t.to_owned())
            }
            Some(_) => {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    format!("progress[{idx}].style must be a string"),
                ));
            }
        };

        parsed.push(ProgressEntry {
            kind: kind.to_owned(),
            unit: unit.to_owned(),
            minutes,
            note,
            style,
        });
    }
    Ok(parsed)
}

/// Returns true iff `s` has the exact shape `<subject>/<unit>` with two valid slugs.
pub fn validate_unit_ref(s: &str) -> bool {
    let Some((subject, unit)) = s.split_once('/') else {
        return false;
    };
    !unit.contains('/') && is_slug(subject) && is_slug(unit)
}

/// Appends validated `step.completed` events immediately (`sync_all` inside `Workspace::append`).
pub fn append_progress_entries(
    ws: &Workspace,
    active_session_id: Option<&str>,
    entries: &[ProgressEntry],
) -> std::result::Result<Vec<String>, ToolError> {
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let mut warnings = Vec::new();
    if active_session_id.is_none() {
        warnings.push(
            "progress recorded without an active session; call lucubra_session(action=\"start\") before studying."
                .to_owned(),
        );
    }
    for entry in entries {
        let mut payload = json!({
            "kind": entry.kind,
            "unit": entry.unit,
            "minutes": entry.minutes,
        });
        if let Some(sid) = active_session_id {
            payload["session_id"] = Value::String(sid.to_owned());
        }
        if let Some(note) = &entry.note {
            payload["note"] = Value::String(note.clone());
        }
        if let Some(style) = &entry.style {
            payload["style"] = Value::String(style.clone());
        }
        ws.append("step.completed", payload).map_err(|e| {
            ToolError::new(
                ToolErrorCode::WorkspaceError,
                format!("failed to persist progress checkpoint: {e}"),
            )
        })?;
    }
    Ok(warnings)
}

/// Wraps tool output into a budget-enforced [`CallToolResponse`] and returns `(response, tokens_added)`.
#[allow(clippy::too_many_arguments)]
pub fn finish_success(
    mut data: Value,
    next: Option<String>,
    mut warnings: Vec<String>,
    limits: BudgetLimits,
    per_call_limit: usize,
    prior_served: u64,
    suppress_wrap_up: bool,
) -> (CallToolResponse, u64) {
    let mut effective_next = next;
    let draft = SuccessEnvelope {
        ok: true,
        data: data.clone(),
        next: effective_next.clone(),
        budget: BudgetInfo {
            served: prior_served,
            limit: limits.wrap_up_limit,
        },
        warnings: warnings.clone(),
    };
    let raw = serde_json::to_string(&draft).unwrap_or_else(|_| "{}".into());
    if estimate_tokens(&raw) > per_call_limit {
        data = json!({
            "truncated": true,
            "message": "Response exceeded tier token budget; request smaller slice or page offset."
        });
        warnings.push(format!(
            "Response trimmed to stay within {per_call_limit} tokens."
        ));
    }

    // Two-pass accounting so reported `budget.served` reflects final serialized size.
    let pass1 = SuccessEnvelope {
        ok: true,
        data: data.clone(),
        next: effective_next.clone(),
        budget: BudgetInfo {
            served: prior_served,
            limit: limits.wrap_up_limit,
        },
        warnings: warnings.clone(),
    };
    let s1 = serde_json::to_string(&pass1).unwrap_or_else(|_| "{}".into());
    let mut added = estimate_tokens(&s1) as u64;

    if !suppress_wrap_up && prior_served + added >= limits.wrap_up_limit {
        effective_next = Some("wrap_up".into());
        let wrap_warn = format!(
            "Session served-token budget reached ({}/{}); call lucubra_session(action=\"end\") to save the session handoff.",
            prior_served + added,
            limits.wrap_up_limit
        );
        if !warnings.iter().any(|w| w.contains("served-token budget")) {
            warnings.push(wrap_warn);
        }
    }

    let pass2 = SuccessEnvelope {
        ok: true,
        data,
        next: effective_next,
        budget: BudgetInfo {
            served: prior_served + added,
            limit: limits.wrap_up_limit,
        },
        warnings,
    };
    let final_json = serde_json::to_string(&pass2).unwrap_or_else(|_| "{}".into());
    added = estimate_tokens(&final_json) as u64;

    let result = CallToolResult::success(vec![ContentBlock::text(final_json)]);
    (result.into(), added)
}

/// Wraps a [`ToolError`] into `CallToolResult::error` with compact `{ok: false, code, fix, budget, warnings}`
/// and returns `(response, tokens_added)`.
pub fn finish_error(
    err: ToolError,
    mut warnings: Vec<String>,
    limits: BudgetLimits,
    prior_served: u64,
    suppress_wrap_up: bool,
) -> (CallToolResponse, u64) {
    let pass1 = ErrorEnvelope {
        ok: false,
        code: err.code,
        fix: err.fix.clone(),
        detail: err.detail.clone(),
        budget: BudgetInfo {
            served: prior_served,
            limit: limits.wrap_up_limit,
        },
        warnings: warnings.clone(),
    };
    let s1 = serde_json::to_string(&pass1).unwrap_or_else(|_| "{}".into());
    let mut added = estimate_tokens(&s1) as u64;
    if !suppress_wrap_up && prior_served + added >= limits.wrap_up_limit {
        let wrap_warn = format!(
            "Session served-token budget reached ({}/{}); call lucubra_session(action=\"end\") to save the session handoff.",
            prior_served + added,
            limits.wrap_up_limit
        );
        if !warnings.iter().any(|w| w.contains("served-token budget")) {
            warnings.push(wrap_warn);
        }
    }
    let final_env = ErrorEnvelope {
        ok: false,
        code: err.code,
        fix: err.fix,
        detail: err.detail,
        budget: BudgetInfo {
            served: prior_served + added,
            limit: limits.wrap_up_limit,
        },
        warnings,
    };
    let final_json = serde_json::to_string(&final_env).unwrap_or_else(|_| "{}".into());
    added = estimate_tokens(&final_json) as u64;
    let result = CallToolResult::error(vec![ContentBlock::text(final_json)]);
    (result.into(), added)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_progress_array_validates_fields_strictly() {
        let valid: Map<String, Value> = serde_json::from_value(json!({
            "progress": [{
                "kind": "recall",
                "unit": "stats/u1",
                "minutes": 15,
                "style": "tutor",
                "note": "Checked Bayes theorem examples"
            }]
        }))
        .unwrap();
        let entries = parse_progress_array(&valid).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].unit, "stats/u1");

        let bad_slug: Map<String, Value> = serde_json::from_value(json!({
            "progress": [{"kind": "step", "unit": "stats/../u1", "minutes": 10}]
        }))
        .unwrap();
        let err = parse_progress_array(&bad_slug).unwrap_err();
        assert_eq!(err.code, ToolErrorCode::InvalidSlug);

        let zero_min: Map<String, Value> = serde_json::from_value(json!({
            "progress": [{"kind": "step", "unit": "stats/u1", "minutes": 0}]
        }))
        .unwrap();
        assert_eq!(
            parse_progress_array(&zero_min).unwrap_err().code,
            ToolErrorCode::InvalidArguments
        );
    }

    #[test]
    fn finish_success_triggers_wrap_up_when_threshold_crossed() {
        let limits = crate::budget::BudgetTier::Free.limits();
        let (resp, added) = finish_success(
            json!({"hello": "world"}),
            Some("ready".into()),
            Vec::new(),
            limits,
            limits.response_limit,
            24_995,
            false,
        );
        assert!(added > 0);
        let CallToolResponse::Complete(res) = resp else {
            panic!("expected Complete");
        };
        let text = match &res.content[0] {
            ContentBlock::Text(t) => &t.text,
            _ => panic!("expected text"),
        };
        let v: Value = serde_json::from_str(text).unwrap();
        assert_eq!(v["next"], "wrap_up");
        assert!(v["warnings"].as_array().unwrap().len() == 1);
    }
}
