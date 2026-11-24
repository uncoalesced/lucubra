// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_session`: interactive study session start, step, resume, and end (`SPEC 8.2, 15`).

use lucubra_core::state::{State, col};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::budget::BudgetLimits;
use crate::envelope::{
    ProgressEntry, ToolError, ToolErrorCode, VALID_A2_STYLES, validate_unit_ref,
};
use crate::tools::{
    ToolContext, ToolOutcome, check_keys, count_words, opt_bool, opt_str, opt_u64, req_str,
    require_action,
};

/// JSON Schema arguments for `lucubra_session`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionArgs {
    /// Action: start, step, end, or resume.
    pub action: String,
    /// Target session ULID (for resume, or explicit step/end).
    #[serde(default)]
    pub session_id: Option<String>,
    /// Optional linked plan id (for start).
    #[serde(default)]
    pub plan_id: Option<String>,
    /// Unit slugs "<subject>/<unit>" to focus on (for start).
    #[serde(default)]
    pub units: Option<Vec<String>>,
    /// Addendum A2 teaching style identifier (for start or step).
    #[serde(default)]
    pub style: Option<String>,
    /// Single unit "<subject>/<unit>" for step.
    #[serde(default)]
    pub unit: Option<String>,
    /// Activity category for step (default: "step").
    #[serde(default)]
    pub kind: Option<String>,
    /// Completed minutes in 1..=480 (required for step).
    #[serde(default)]
    pub minutes: Option<u64>,
    /// Optional note for step (<= 500 chars).
    #[serde(default)]
    pub note: Option<String>,
    /// End reason: planned, user, budget, or recovered (default: "user").
    #[serde(default)]
    pub reason: Option<String>,
    /// Concise session handoff summary (<= 150 words, for end).
    #[serde(default)]
    pub summary: Option<String>,
    /// Close any open session as "recovered" before starting a new one (for start).
    #[serde(default)]
    pub force_new: Option<bool>,
    /// Optional completed-step checkpoints to log alongside this call.
    #[serde(default)]
    pub progress: Option<Vec<ProgressEntry>>,
}

fn ws_err(e: lucubra_core::Error) -> ToolError {
    ToolError::new(
        ToolErrorCode::WorkspaceError,
        format!("session persistence error: {e}"),
    )
}

/// The highest-priority unit for the next session, as `<subject>/<unit>`.
fn next_proposal(state: &State) -> Option<String> {
    use lucubra_core::planner::{PlannerConfig, schedule, work_items};
    let cfg = PlannerConfig::default();
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400) as i64;
    let domain = state
        .get(col::PROFILE, "programme")
        .and_then(Value::as_str)
        .unwrap_or("");
    let items = work_items(&cfg, state, days, domain);
    let cap = cfg.default_capacity;
    schedule(&cfg, &items, &[cap], cap)
        .blocks
        .first()
        .map(|b| format!("{}/{}", b.subject, b.unit))
}

/// Closes open sessions idle longer than the recovery window as
/// `recovered` (SPEC section 15). Returns the recovered session ids.
pub fn recover_idle(ws: &lucubra_core::Workspace) -> lucubra_core::Result<Vec<String>> {
    use lucubra_core::calendar::parse_minutes;
    let cfg = lucubra_core::planner::PlannerConfig::default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 60) as i64;
    let idle = (cfg.recovery_idle_ms / 60_000) as i64;
    let state = ws.replay()?.state;
    let mut recovered = Vec::new();
    for (sid, s) in state.collection(col::SESSIONS) {
        if sid == "_unattached" || s.get("status").and_then(Value::as_str) != Some("active") {
            continue;
        }
        let last = ["last_ts", "started_ts"]
            .iter()
            .find_map(|k| s.get(*k).and_then(Value::as_str).and_then(parse_minutes));
        if last.is_some_and(|t| now - t > idle) {
            let mut payload = json!({"session_id": sid, "reason": "recovered"});
            if let Some(next) = next_proposal(&state) {
                payload["next_proposal"] = Value::String(next);
            }
            ws.append("session.ended", payload)?;
            recovered.push(sid.clone());
        }
    }
    if !recovered.is_empty() {
        ws.project()?;
    }
    Ok(recovered)
}

fn latest_active_session(state: &State) -> Option<String> {
    state
        .collection(col::SESSIONS)
        .filter(|(k, v)| {
            k.as_str() != "_unattached" && v.get("status").and_then(Value::as_str) == Some("active")
        })
        .map(|(k, _)| k.clone())
        .last()
}

pub fn execute(
    args: &Map<String, Value>,
    ctx: &ToolContext<'_>,
    limits: BudgetLimits,
) -> std::result::Result<ToolOutcome, ToolError> {
    let action = require_action(args, &["start", "step", "end", "resume"])?;
    let state = ctx.ws.replay().map_err(ws_err)?.state;

    match action {
        "start" => {
            check_keys(
                args,
                &[
                    "action",
                    "plan_id",
                    "units",
                    "style",
                    "force_new",
                    "progress",
                ],
                "lucubra_session(start)",
            )?;
            let plan_id = opt_str(args, "plan_id")?;
            let style = opt_str(args, "style")?;
            if let Some(st) = style
                && !VALID_A2_STYLES.contains(&st)
            {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    format!("style must be one of: {}", VALID_A2_STYLES.join(", ")),
                ));
            }
            let units = match args.get("units") {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::Array(arr)) => {
                    let mut list = Vec::with_capacity(arr.len());
                    for (idx, item) in arr.iter().enumerate() {
                        let Some(s) = item.as_str().map(str::trim) else {
                            return Err(ToolError::new(
                                ToolErrorCode::InvalidArguments,
                                format!("units[{idx}] must be a '<subject>/<unit>' string"),
                            ));
                        };
                        if !validate_unit_ref(s) {
                            return Err(ToolError::new(
                                ToolErrorCode::InvalidSlug,
                                format!(
                                    "units[{idx}] must be '<subject>/<unit>' using valid lowercase slugs"
                                ),
                            ));
                        }
                        list.push(s.to_owned());
                    }
                    list
                }
                Some(_) => {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        "units must be an array of '<subject>/<unit>' strings",
                    ));
                }
            };
            let force_new =
                opt_bool(args, "force_new", "force_new must be true or false")?.unwrap_or(false);

            let open_sid = ctx
                .active_session_id
                .map(str::to_owned)
                .or_else(|| latest_active_session(&state));
            if let Some(existing) = open_sid {
                if !force_new {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidState,
                        format!(
                            "an active session '{existing}' is already open; call lucubra_session(action=\"resume\") or pass force_new=true"
                        ),
                    ));
                }
                ctx.ws
                    .append(
                        "session.ended",
                        json!({ "session_id": existing, "reason": "recovered" }),
                    )
                    .map_err(ws_err)?;
            }

            let new_sid = ulid::Ulid::new().to_string();
            let mut payload = json!({
                "session_id": new_sid,
                "units": units,
            });
            if let Some(pid) = plan_id {
                payload["plan_id"] = Value::String(pid.to_owned());
            }
            if let Some(st) = style {
                payload["style"] = Value::String(st.to_owned());
            }
            ctx.ws.append("session.started", payload).map_err(ws_err)?;

            Ok(ToolOutcome {
                data: json!({
                    "session_id": new_sid,
                    "units": units,
                    "style": style,
                    "status": "active",
                }),
                next: Some("read".into()),
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: Some(Some(new_sid)),
            })
        }
        "step" => {
            check_keys(
                args,
                &[
                    "action",
                    "session_id",
                    "unit",
                    "kind",
                    "minutes",
                    "note",
                    "style",
                    "progress",
                ],
                "lucubra_session(step)",
            )?;
            let unit = req_str(
                args,
                "unit",
                "provide 'unit' as '<subject>/<unit>' for step logging",
            )?;
            if !validate_unit_ref(unit) {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidSlug,
                    "unit must be '<subject>/<unit>' using valid lowercase slugs",
                ));
            }
            let minutes = opt_u64(
                args,
                "minutes",
                1,
                480,
                "provide 'minutes' as an integer between 1 and 480",
            )?
            .ok_or_else(|| {
                ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "provide 'minutes' as an integer between 1 and 480",
                )
            })? as u32;
            let kind = opt_str(args, "kind")?.unwrap_or("step");
            if kind.chars().count() > 32 {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "kind must be 1 to 32 characters",
                ));
            }
            let note = opt_str(args, "note")?;
            if let Some(n) = note
                && n.chars().count() > 500
            {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "note exceeds 500 characters",
                ));
            }
            let style = opt_str(args, "style")?;
            if let Some(st) = style
                && !VALID_A2_STYLES.contains(&st)
            {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    format!("style must be one of: {}", VALID_A2_STYLES.join(", ")),
                ));
            }

            let sid = opt_str(args, "session_id")?
                .map(str::to_owned)
                .or_else(|| ctx.active_session_id.map(str::to_owned))
                .or_else(|| latest_active_session(&state))
                .ok_or_else(|| {
                    ToolError::new(
                        ToolErrorCode::InvalidState,
                        "no active session; call lucubra_session(action=\"start\") first",
                    )
                })?;

            let mut payload = json!({
                "session_id": sid,
                "kind": kind,
                "unit": unit,
                "minutes": minutes,
            });
            if let Some(n) = note {
                payload["note"] = Value::String(n.to_owned());
            }
            if let Some(st) = style {
                payload["style"] = Value::String(st.to_owned());
            }
            ctx.ws.append("step.completed", payload).map_err(ws_err)?;

            Ok(ToolOutcome {
                data: json!({
                    "session_id": sid,
                    "logged": {
                        "kind": kind,
                        "unit": unit,
                        "minutes": minutes,
                        "style": style,
                    }
                }),
                next: None,
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: Some(Some(sid)),
            })
        }
        "end" => {
            check_keys(
                args,
                &["action", "session_id", "reason", "summary", "progress"],
                "lucubra_session(end)",
            )?;
            let reason = opt_str(args, "reason")?.unwrap_or("user");
            if !matches!(reason, "planned" | "user" | "budget" | "recovered") {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "reason must be one of: planned, user, budget, recovered",
                ));
            }
            let summary = opt_str(args, "summary")?;
            if let Some(sum) = summary {
                let words = count_words(sum);
                if words > 150 {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        format!("summary has {words} words; shorten it to at most 150 words"),
                    ));
                }
            }
            let sid = opt_str(args, "session_id")?
                .map(str::to_owned)
                .or_else(|| ctx.active_session_id.map(str::to_owned))
                .or_else(|| latest_active_session(&state))
                .ok_or_else(|| {
                    ToolError::new(
                        ToolErrorCode::InvalidState,
                        "no active session to end; call lucubra_session(action=\"start\") first",
                    )
                })?;

            if let Some(sum) = summary {
                ctx.ws
                    .append(
                        "handoff.written",
                        json!({ "session_id": sid, "summary": sum }),
                    )
                    .map_err(ws_err)?;
            }
            let mut end_payload = json!({
                "session_id": sid,
                "reason": reason,
            });
            if let Some(next) = next_proposal(&state) {
                end_payload["next_proposal"] = Value::String(next);
            }
            if let Some(sum) = summary {
                end_payload["summary"] = Value::String(sum.to_owned());
            }
            ctx.ws
                .append("session.ended", end_payload)
                .map_err(ws_err)?;
            ctx.ws.project().map_err(ws_err)?;
            let updated_state = ctx.ws.replay().map_err(ws_err)?.state;
            let session = updated_state.get(col::SESSIONS, &sid);
            let handoff = session.and_then(lucubra_core::projection::session_file);
            let next_session = session.and_then(|s| s.get("next_proposal")).cloned();
            let logged_minutes = updated_state
                .get(col::SESSIONS, &sid)
                .and_then(|v| v.get("logged_minutes"))
                .and_then(Value::as_u64)
                .unwrap_or(0);

            Ok(ToolOutcome {
                data: json!({
                    "session_id": sid,
                    "status": "ended",
                    "reason": reason,
                    "logged_minutes": logged_minutes,
                    "summary": summary,
                    "handoff_file": handoff,
                    "next_session": next_session,
                }),
                next: None,
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: true,
                active_session_override: Some(None),
            })
        }
        "resume" => {
            check_keys(
                args,
                &["action", "session_id", "progress"],
                "lucubra_session(resume)",
            )?;
            let sid = if let Some(explicit) = opt_str(args, "session_id")? {
                if explicit == "_unattached" || state.get(col::SESSIONS, explicit).is_none() {
                    return Err(ToolError::new(
                        ToolErrorCode::NotFound,
                        format!("session '{explicit}' not found"),
                    ));
                }
                explicit.to_owned()
            } else {
                latest_active_session(&state).ok_or_else(|| {
                    ToolError::new(
                        ToolErrorCode::NotFound,
                        "no unclosed session to resume; call lucubra_session(action=\"start\")",
                    )
                })?
            };
            let entry = state
                .get(col::SESSIONS, &sid)
                .cloned()
                .unwrap_or(Value::Null);
            Ok(ToolOutcome {
                data: json!({
                    "session_id": sid,
                    "session": entry,
                }),
                next: Some("read".into()),
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: Some(Some(sid)),
            })
        }
        _ => unreachable!(),
    }
}
