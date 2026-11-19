// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_profile`: onboarding, profile fields, subjects, and exams (`SPEC section 9`).

use lucubra_core::state::col;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::budget::{BudgetLimits, BudgetTier};
use crate::envelope::{ProgressEntry, ToolError, ToolErrorCode, VALID_A2_STYLES};
use crate::material::read_preferences_summary;
use crate::tools::orient::missing_required_fields;
use crate::tools::{
    ToolContext, ToolOutcome, check_keys, check_slug, opt_str, req_str, require_action,
};

/// JSON Schema arguments for `lucubra_profile`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProfileArgs {
    /// Action: next, answer, skip, get, or set.
    pub action: String,
    /// Profile field key (for answer, skip, or single-field set).
    #[serde(default)]
    pub field: Option<String>,
    /// Value for field (for answer or single-field set).
    #[serde(default)]
    pub value: Option<Value>,
    /// Map of multiple field -> value updates (for set).
    #[serde(default)]
    pub fields: Option<Map<String, Value>>,
    /// Reason for skipping an optional field.
    #[serde(default)]
    pub reason: Option<String>,
    /// Optional completed-step checkpoints to log alongside this call.
    #[serde(default)]
    pub progress: Option<Vec<ProgressEntry>>,
}

const REQUIRED_FIELDS: &[&str] = &[
    "level",
    "programme",
    "term_system",
    "current_term",
    "start_year",
    "completion_year",
    "subjects",
    "exams",
];

const OPTIONAL_FIELDS: &[&str] = &[
    "institution",
    "grading_scheme",
    "schedule_planning",
    "plan_tier",
    "plan_tier_by_host",
    "local_model",
    "integrity_mode",
    "explanation_language",
    "teaching_style",
    "timezone",
    "host_notes",
];

fn field_prompt(field: &str) -> (&'static str, &'static str) {
    match field {
        "level" => (
            "What education level are you studying at?",
            "\"undergraduate\"",
        ),
        "programme" => (
            "What degree or programme are you enrolled in?",
            "\"B.Tech CSE\"",
        ),
        "term_system" => (
            "Are your courses organized by semester, trimester, quarter, or annual?",
            "\"semester\"",
        ),
        "current_term" => ("Which term or year are you currently in?", "\"Semester 5\""),
        "start_year" => ("What year did you start this programme?", "2024"),
        "completion_year" => ("What year do you expect to complete it?", "2028"),
        "subjects" => (
            "List your active subjects as slugs or {slug, title} objects.",
            "[{\"slug\": \"stats\", \"title\": \"Statistics\"}]",
        ),
        "exams" => (
            "List upcoming exams with subject, id, and date.",
            "[{\"id\": \"stat201-mid\", \"subject\": \"stats\", \"date\": \"2026-12-10\"}]",
        ),
        _ => ("Provide field value.", "\"...\""),
    }
}

fn validate_and_apply_field(
    ctx: &ToolContext<'_>,
    key: &str,
    val: &Value,
) -> std::result::Result<(), ToolError> {
    if !REQUIRED_FIELDS.contains(&key) && !OPTIONAL_FIELDS.contains(&key) {
        return Err(ToolError::new(
            ToolErrorCode::InvalidArguments,
            format!(
                "unknown profile field '{key}'; call lucubra_profile(action=\"get\") to see supported fields"
            ),
        ));
    }

    match key {
        "plan_tier" => {
            let Some(s) = val.as_str() else {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "plan_tier must be one of: free, paid, local",
                ));
            };
            if BudgetTier::parse(s).is_none() {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "plan_tier must be one of: free, paid, local",
                ));
            }
            ctx.ws
                .append(
                    "profile.set",
                    json!({ "plan_tier": s.to_ascii_lowercase() }),
                )
                .map_err(ws_err)?;
        }
        "schedule_planning" | "local_model" => {
            if !val.is_boolean() {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    format!("{key} must be true or false"),
                ));
            }
            ctx.ws
                .append("profile.set", json!({ key: val }))
                .map_err(ws_err)?;
        }
        "teaching_style" => {
            let Some(s) = val.as_str() else {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    format!(
                        "teaching_style must be one of: {}",
                        VALID_A2_STYLES.join(", ")
                    ),
                ));
            };
            if !VALID_A2_STYLES.contains(&s) {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    format!(
                        "teaching_style must be one of: {}",
                        VALID_A2_STYLES.join(", ")
                    ),
                ));
            }
            ctx.ws
                .append("profile.set", json!({ "teaching_style": s }))
                .map_err(ws_err)?;
        }
        "subjects" => {
            let Some(arr) = val.as_array() else {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "subjects must be an array of slugs or {slug, title} objects",
                ));
            };
            let mut slugs = Vec::new();
            for (idx, item) in arr.iter().enumerate() {
                let (slug, title) = if let Some(s) = item.as_str() {
                    (s.trim(), None)
                } else if let Some(obj) = item.as_object() {
                    let s = obj
                        .get("slug")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or("");
                    let t = obj.get("title").and_then(Value::as_str).map(str::trim);
                    (s, t)
                } else {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        format!("subjects[{idx}] must be a slug string or {{slug, title}} object"),
                    ));
                };
                check_slug(&format!("subjects[{idx}]"), slug)?;
                let mut payload = json!({ "subject": slug });
                if let Some(t) = title.filter(|s| !s.is_empty()) {
                    payload["title"] = Value::String(t.to_owned());
                }
                ctx.ws.append("subject.created", payload).map_err(ws_err)?;
                slugs.push(slug.to_owned());
            }
            ctx.ws
                .append("profile.set", json!({ "subjects": slugs }))
                .map_err(ws_err)?;
        }
        "exams" => {
            let Some(arr) = val.as_array() else {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "exams must be an array of {id, subject, date, title?, remove?} objects",
                ));
            };
            let mut ids = Vec::new();
            for (idx, item) in arr.iter().enumerate() {
                let Some(obj) = item.as_object() else {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        format!("exams[{idx}] must be an object with {{id, subject, date}}"),
                    ));
                };
                let id = obj
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        ToolError::new(
                            ToolErrorCode::InvalidArguments,
                            format!("exams[{idx}].id must be a non-empty string"),
                        )
                    })?;
                if obj.get("remove").and_then(Value::as_bool) == Some(true) {
                    ctx.ws
                        .append("exam.removed", json!({ "id": id }))
                        .map_err(ws_err)?;
                    continue;
                }
                let mut payload = Value::Object(obj.clone());
                if let Some(subj) = obj.get("subject").and_then(Value::as_str) {
                    check_slug(&format!("exams[{idx}].subject"), subj)?;
                }
                payload["id"] = Value::String(id.to_owned());
                ctx.ws.append("exam.set", payload).map_err(ws_err)?;
                ids.push(id.to_owned());
            }
            ctx.ws
                .append("profile.set", json!({ "exams": ids }))
                .map_err(ws_err)?;
        }
        _ => {
            ctx.ws
                .append("profile.set", json!({ key: val }))
                .map_err(ws_err)?;
        }
    }
    Ok(())
}

fn ws_err(e: lucubra_core::Error) -> ToolError {
    ToolError::new(
        ToolErrorCode::WorkspaceError,
        format!("workspace event update failed: {e}"),
    )
}

pub fn execute(
    args: &Map<String, Value>,
    ctx: &ToolContext<'_>,
    limits: BudgetLimits,
) -> std::result::Result<ToolOutcome, ToolError> {
    let action = require_action(args, &["next", "answer", "skip", "get", "set"])?;
    match action {
        "next" => {
            check_keys(args, &["action", "progress"], "lucubra_profile(next)")?;
            let state = ctx.ws.replay().map_err(ws_err)?.state;
            let missing = missing_required_fields(&state);
            let next_field = missing.first().copied();
            let (prompt, example) = next_field.map(field_prompt).unwrap_or(("", ""));
            Ok(ToolOutcome {
                data: json!({
                    "done": missing.is_empty(),
                    "next_field": next_field,
                    "prompt": (!prompt.is_empty()).then_some(prompt),
                    "example": (!example.is_empty()).then_some(example),
                    "missing_required": missing,
                }),
                next: Some(
                    if missing.is_empty() {
                        "ready"
                    } else {
                        "answer"
                    }
                    .into(),
                ),
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: None,
            })
        }
        "get" => {
            check_keys(args, &["action", "progress"], "lucubra_profile(get)")?;
            let state = ctx.ws.replay().map_err(ws_err)?.state;
            let pref = read_preferences_summary(ctx.ws.root());
            Ok(ToolOutcome {
                data: json!({
                    "profile": state.collections.get(col::PROFILE),
                    "subjects": state.collections.get(col::SUBJECTS),
                    "exams": state.collections.get(col::EXAMS),
                    "settings": state.collections.get(col::SETTINGS),
                    "preferences_configured": pref.is_some(),
                }),
                next: None,
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: None,
            })
        }
        "answer" => {
            check_keys(
                args,
                &["action", "field", "value", "progress"],
                "lucubra_profile(answer)",
            )?;
            let field = req_str(
                args,
                "field",
                "provide both 'field' and 'value' for action 'answer'; call lucubra_profile(action=\"next\") to see the current field",
            )?;
            let Some(value) = args.get("value").filter(|v| !v.is_null()) else {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "provide both 'field' and 'value' for action 'answer'; call lucubra_profile(action=\"next\") to see the current field",
                ));
            };
            validate_and_apply_field(ctx, field, value)?;
            ctx.ws.project().map_err(ws_err)?;
            let state = ctx.ws.replay().map_err(ws_err)?.state;
            let missing = missing_required_fields(&state);
            let next_field = missing.first().copied();
            Ok(ToolOutcome {
                data: json!({
                    "saved": field,
                    "done": missing.is_empty(),
                    "next_field": next_field,
                    "missing_required": missing,
                }),
                next: Some(
                    if missing.is_empty() {
                        "ready"
                    } else {
                        "answer"
                    }
                    .into(),
                ),
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: None,
            })
        }
        "skip" => {
            check_keys(
                args,
                &["action", "field", "reason", "progress"],
                "lucubra_profile(skip)",
            )?;
            let field = req_str(
                args,
                "field",
                "provide 'field' naming the optional field to skip",
            )?;
            if REQUIRED_FIELDS.contains(&field) {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    format!(
                        "field '{field}' is required for onboarding and cannot be skipped; use action 'answer' instead"
                    ),
                ));
            }
            if !OPTIONAL_FIELDS.contains(&field) {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    format!(
                        "unknown profile field '{field}'; call lucubra_profile(action=\"get\") to see supported fields"
                    ),
                ));
            }
            let reason = opt_str(args, "reason")?;
            ctx.ws
                .append(
                    "profile.skipped",
                    json!({ "field": field, "reason": reason }),
                )
                .map_err(ws_err)?;
            let state = ctx.ws.replay().map_err(ws_err)?.state;
            let mut skipped: Vec<String> = state
                .get(col::PROFILE, "skipped_fields")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            if !skipped.iter().any(|s| s == field) {
                skipped.push(field.to_owned());
                ctx.ws
                    .append("profile.set", json!({ "skipped_fields": skipped }))
                    .map_err(ws_err)?;
            }
            Ok(ToolOutcome {
                data: json!({ "skipped": field }),
                next: None,
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: None,
            })
        }
        "set" => {
            check_keys(
                args,
                &["action", "field", "value", "fields", "progress"],
                "lucubra_profile(set)",
            )?;
            let mut updates: Vec<(String, Value)> = Vec::new();
            if let Some(raw_map) = args.get("fields") {
                let Some(obj) = raw_map.as_object() else {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        "field 'fields' must be a JSON object mapping profile keys to values",
                    ));
                };
                for (k, v) in obj {
                    updates.push((k.clone(), v.clone()));
                }
            }
            if let Some(f) = opt_str(args, "field")? {
                let Some(v) = args.get("value") else {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        "provide 'value' alongside 'field' in lucubra_profile(action=\"set\")",
                    ));
                };
                updates.push((f.to_owned(), v.clone()));
            }
            if updates.is_empty() {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "provide 'fields' map or {field, value} for lucubra_profile(action=\"set\")",
                ));
            }
            for (k, v) in &updates {
                validate_and_apply_field(ctx, k, v)?;
            }
            ctx.ws.project().map_err(ws_err)?;
            let updated_keys: Vec<String> = updates.into_iter().map(|(k, _)| k).collect();
            Ok(ToolOutcome {
                data: json!({ "updated": updated_keys }),
                next: None,
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: None,
            })
        }
        _ => unreachable!(),
    }
}
