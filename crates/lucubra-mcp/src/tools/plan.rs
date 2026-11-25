// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_plan`: propose, edit and accept study plans, a horizon view, and
//! in-session sequencing (`next`) (SPEC sections 12 and 17; S02, S13, S14,
//! S24, S25, S39). Scheduling is deterministic: see `lucubra_core::planner`.

use std::io::Read;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lucubra_core::calendar::{busy_per_day, ics_intervals, parse_minutes, stored_url};
use lucubra_core::planner::{
    PlannerConfig, Schedule, days_from_iso, iso_from_days, overcommit, schedule, work_items,
};
use lucubra_core::state::{State, col};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::budget::BudgetLimits;
use crate::envelope::{ProgressEntry, ToolError, ToolErrorCode};
use crate::tools::{
    ToolContext, ToolOutcome, check_keys, check_slug, opt_bool, opt_str, opt_u64, req_str,
    require_action,
};

/// JSON Schema arguments for `lucubra_plan`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanArgs {
    /// Action: propose, edit, accept, horizon, or next (what to study now).
    pub action: String,
    /// Number of days in the planning window (1-30; default 7, horizon 30).
    #[serde(default)]
    pub days: Option<u64>,
    /// Free study time per day in minutes before busy blocks (10-960; default 240).
    #[serde(default)]
    pub available_minutes: Option<u64>,
    /// Busy blocks from the host's calendar: `[{start, end}]` in RFC 3339.
    #[serde(default)]
    pub busy_blocks: Option<Vec<Value>>,
    /// Today's date in the user's time zone, `YYYY-MM-DD` (default: UTC date).
    #[serde(default)]
    pub today: Option<String>,
    /// Plan identifier for edit or accept.
    #[serde(default)]
    pub plan_id: Option<String>,
    /// Edited blocks for edit: `[{date, subject, unit, minutes, break_after?}]`.
    #[serde(default)]
    pub blocks: Option<Vec<Value>>,
    /// Kept for compatibility; an overcommitted plan is accepted with one warning.
    #[serde(default)]
    pub overcommit_confirmed: Option<bool>,
    /// Optional completed-step checkpoints to log alongside this call.
    #[serde(default)]
    pub progress: Option<Vec<ProgressEntry>>,
}

const DEFAULT_FREE_MINUTES: u64 = 240;
const ICS_MAX_BYTES: u64 = 2 << 20;
const CALENDAR_FALLBACK: &str = "Lucubra could not read a calendar, so busy times are not included. College Google accounts often hide the secret iCal address; instead, read busy times with your own calendar connector and pass them as busy_blocks, or ask the user for their busy hours.";

fn ws_err(e: lucubra_core::Error) -> ToolError {
    ToolError::new(
        ToolErrorCode::WorkspaceError,
        format!("workspace update failed: {e}"),
    )
}

fn invalid(msg: impl Into<String>) -> ToolError {
    ToolError::new(ToolErrorCode::InvalidArguments, msg)
}

fn outcome(data: Value, next: &str, warnings: Vec<String>, limits: BudgetLimits) -> ToolOutcome {
    ToolOutcome {
        data,
        next: Some(next.into()),
        warnings,
        per_call_limit: limits.response_limit,
        suppress_wrap_up: false,
        active_session_override: None,
    }
}

fn today(args: &Map<String, Value>) -> std::result::Result<i64, ToolError> {
    match opt_str(args, "today")? {
        Some(s) => days_from_iso(s).ok_or_else(|| invalid("today must be a date like 2026-11-23")),
        None => {
            let secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            Ok((secs / 86_400) as i64)
        }
    }
}

/// Downloads an iCal feed. Errors never include the address, which works
/// like a password (S24).
fn fetch_ics(url: &str) -> std::result::Result<String, &'static str> {
    let local = url.starts_with("http://127.0.0.1") || url.starts_with("http://localhost");
    if !url.starts_with("https://") && !local {
        return Err("the calendar address must start with https://");
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into();
    let response = agent
        .get(url)
        .call()
        .map_err(|_| "the calendar could not be downloaded")?;
    let mut body = String::new();
    response
        .into_body()
        .into_reader()
        .take(ICS_MAX_BYTES)
        .read_to_string(&mut body)
        .map_err(|_| "the calendar download was not readable text")?;
    if !body.contains("BEGIN:VCALENDAR") {
        return Err("the address did not return an iCal calendar");
    }
    Ok(body)
}

fn capacity(cfg: &PlannerConfig, state: &State) -> u32 {
    lucubra_core::metrics::capacity(cfg, state)
}

struct Proposal {
    schedule: Schedule,
    start: i64,
    capacity: u32,
    calendar: &'static str,
    warnings: Vec<String>,
}

fn build(
    args: &Map<String, Value>,
    ctx: &ToolContext<'_>,
    state: &State,
    default_days: u64,
    focus_span: f64,
) -> std::result::Result<Proposal, ToolError> {
    let cfg = PlannerConfig {
        focus_span,
        ..PlannerConfig::default()
    };
    let days = opt_u64(
        args,
        "days",
        1,
        30,
        "days must be an integer between 1 and 30",
    )?
    .unwrap_or(default_days) as usize;
    let free = opt_u64(
        args,
        "available_minutes",
        10,
        960,
        "available_minutes must be an integer between 10 and 960",
    )?
    .unwrap_or(DEFAULT_FREE_MINUTES) as u32;
    let start = today(args)?;
    let mut intervals = Vec::new();
    let mut warnings = Vec::new();
    let mut calendar = "none";
    if let Some(raw) = args.get("busy_blocks") {
        let blocks = raw
            .as_array()
            .ok_or_else(|| invalid("busy_blocks must be an array of {start, end} objects"))?;
        for (idx, b) in blocks.iter().enumerate() {
            let get = |k: &str| b.get(k).and_then(Value::as_str).and_then(parse_minutes);
            let (Some(s), Some(e)) = (get("start"), get("end")) else {
                return Err(invalid(format!(
                    "busy_blocks[{idx}] needs RFC 3339 'start' and 'end', like 2026-11-24T09:00:00Z"
                )));
            };
            intervals.push((s, e));
        }
        calendar = "host";
    }
    if let Some(url) = stored_url(ctx.ws) {
        match fetch_ics(&url) {
            Ok(ics) => {
                intervals.extend(ics_intervals(&ics));
                calendar = "ical";
            }
            Err(why) => warnings.push(format!("Calendar not read: {why}. {CALENDAR_FALLBACK}")),
        }
    } else if calendar == "none" {
        warnings.push(CALENDAR_FALLBACK.into());
    }
    let busy = busy_per_day(&intervals, start, days);
    let day_minutes: Vec<u32> = busy.iter().map(|b| free.saturating_sub(*b)).collect();
    let domain = state
        .get(col::PROFILE, "programme")
        .and_then(Value::as_str)
        .unwrap_or("");
    let items = work_items(&cfg, state, start, domain);
    let capacity = capacity(&cfg, state);
    Ok(Proposal {
        schedule: schedule(&cfg, &items, &day_minutes, capacity),
        start,
        capacity,
        calendar,
        warnings,
    })
}

fn blocks_json(p: &Proposal) -> Vec<Value> {
    p.schedule
        .blocks
        .iter()
        .map(|b| {
            json!({
                "date": iso_from_days(p.start + i64::from(b.day)),
                "subject": b.subject,
                "unit": b.unit,
                "minutes": b.minutes,
                "break_after": b.break_after,
            })
        })
        .collect()
}

fn triage_json(p: &Proposal) -> Vec<Value> {
    p.schedule
        .triage
        .iter()
        .map(|t| {
            let uncovered = if t.uncovered.is_empty() {
                "none".to_owned()
            } else {
                t.uncovered.join(", ")
            };
            json!({
                "subject": t.subject,
                "exam_date": iso_from_days(p.start + i64::from(t.exam_day)),
                "required_minutes": t.required,
                "available_minutes": t.available,
                "uncovered_units": t.uncovered,
                "message": format!(
                    "Not enough time before the {} exam: about {:.1} hours needed, {:.1} hours available. Only the highest-priority units are planned; not covered: {uncovered}.",
                    t.subject,
                    f64::from(t.required) / 60.0,
                    f64::from(t.available) / 60.0,
                ),
            })
        })
        .collect()
}

fn daily_totals(blocks: &[Value]) -> Vec<(String, u32)> {
    let mut totals: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    for b in blocks {
        if let (Some(d), Some(m)) = (
            b.get("date").and_then(Value::as_str),
            b.get("minutes").and_then(Value::as_u64),
        ) {
            *totals.entry(d.to_owned()).or_default() += m as u32;
        }
    }
    totals.into_iter().collect()
}

fn planning_off(state: &State) -> bool {
    state.get(col::PROFILE, "schedule_planning") == Some(&Value::Bool(false))
}

fn plan_not_found(pid: &str) -> ToolError {
    ToolError::new(
        ToolErrorCode::NotFound,
        format!("no plan '{pid}'; call lucubra_plan(action=\"propose\") first"),
    )
}

fn edited_blocks(raw: &[Value]) -> std::result::Result<Vec<Value>, ToolError> {
    let mut blocks = Vec::with_capacity(raw.len());
    for (idx, b) in raw.iter().enumerate() {
        let s = |k: &str| b.get(k).and_then(Value::as_str);
        let date = s("date")
            .filter(|d| days_from_iso(d).is_some())
            .ok_or_else(|| invalid(format!("blocks[{idx}].date must be YYYY-MM-DD")))?;
        let subject = s("subject").unwrap_or("");
        check_slug(&format!("blocks[{idx}].subject"), subject)?;
        let unit = s("unit").unwrap_or("general");
        check_slug(&format!("blocks[{idx}].unit"), unit)?;
        let minutes = b
            .get("minutes")
            .and_then(Value::as_u64)
            .filter(|m| (5..=480).contains(m))
            .ok_or_else(|| invalid(format!("blocks[{idx}].minutes must be 5 to 480")))?;
        let pause = b
            .get("break_after")
            .and_then(Value::as_u64)
            .unwrap_or(minutes / 5);
        blocks.push(json!({
            "date": date, "subject": subject, "unit": unit,
            "minutes": minutes, "break_after": pause,
        }));
    }
    Ok(blocks)
}

pub fn execute(
    args: &Map<String, Value>,
    ctx: &ToolContext<'_>,
    limits: BudgetLimits,
) -> std::result::Result<ToolOutcome, ToolError> {
    let action = require_action(args, &["propose", "edit", "accept", "horizon", "next"])?;
    let replay = ctx.ws.replay().map_err(ws_err)?;
    let focus = lucubra_core::metrics::focus_span(&PlannerConfig::default(), &replay);
    let state = replay.state;
    match action {
        "propose" | "horizon" => {
            check_keys(
                args,
                &[
                    "action",
                    "days",
                    "available_minutes",
                    "busy_blocks",
                    "today",
                    "progress",
                ],
                "lucubra_plan",
            )?;
            if planning_off(&state) {
                return Ok(outcome(
                    json!({
                        "schedule_planning": false,
                        "message": "Schedule planning is turned off in this profile; focus on in-session sequencing only (S02). Use lucubra_plan(action=\"next\") for what to study now.",
                    }),
                    "in_session_only",
                    Vec::new(),
                    limits,
                ));
            }
            let p = build(
                args,
                ctx,
                &state,
                if action == "horizon" { 30 } else { 7 },
                focus,
            )?;
            if p.schedule.blocks.is_empty() && p.schedule.triage.is_empty() {
                return Ok(outcome(
                    json!({"planned": false, "message": "Nothing to plan yet: scan and classify study material first."}),
                    "scan_inbox",
                    p.warnings,
                    limits,
                ));
            }
            let blocks = blocks_json(&p);
            let triage = triage_json(&p);
            let totals: Vec<Value> = daily_totals(&blocks)
                .into_iter()
                .map(|(d, m)| json!({"date": d, "minutes": m}))
                .collect();
            if action == "horizon" {
                return Ok(outcome(
                    json!({"daily_minutes": totals, "triage": triage, "capacity_minutes": p.capacity}),
                    "propose",
                    p.warnings,
                    limits,
                ));
            }
            let plan_id = ulid::Ulid::new().to_string();
            ctx.ws
                .append(
                    "plan.proposed",
                    json!({
                        "plan_id": plan_id,
                        "start": iso_from_days(p.start),
                        "blocks": blocks,
                        "triage": triage,
                        "capacity": p.capacity,
                    }),
                )
                .map_err(ws_err)?;
            let next = if triage.is_empty() {
                "accept"
            } else {
                "explain_triage"
            };
            Ok(outcome(
                json!({
                    "plan_id": plan_id,
                    "capacity_minutes": p.capacity,
                    "calendar": p.calendar,
                    "daily_minutes": totals,
                    "blocks": blocks,
                    "triage": triage,
                    "note": "Block lengths follow a Pomodoro-style heuristic, not a research-backed method. Show the plan, let the user edit it, then accept.",
                }),
                next,
                p.warnings,
                limits,
            ))
        }
        "edit" => {
            check_keys(
                args,
                &["action", "plan_id", "blocks", "progress"],
                "lucubra_plan(edit)",
            )?;
            let pid = req_str(
                args,
                "plan_id",
                "provide 'plan_id' returned by lucubra_plan(action=\"propose\")",
            )?;
            if state.get(col::PLANS, pid).is_none() {
                return Err(plan_not_found(pid));
            }
            let Some(raw) = args.get("blocks").and_then(Value::as_array) else {
                return Err(invalid(
                    "provide 'blocks' array for lucubra_plan(action=\"edit\")",
                ));
            };
            let blocks = edited_blocks(raw)?;
            ctx.ws
                .append("plan.edited", json!({"plan_id": pid, "blocks": blocks}))
                .map_err(ws_err)?;
            Ok(outcome(
                json!({"plan_id": pid, "edited_blocks": blocks.len()}),
                "accept",
                Vec::new(),
                limits,
            ))
        }
        "accept" => {
            check_keys(
                args,
                &["action", "plan_id", "overcommit_confirmed", "progress"],
                "lucubra_plan(accept)",
            )?;
            let pid = req_str(
                args,
                "plan_id",
                "provide 'plan_id' returned by lucubra_plan(action=\"propose\")",
            )?;
            opt_bool(
                args,
                "overcommit_confirmed",
                "overcommit_confirmed must be true or false",
            )?;
            let Some(plan) = state.get(col::PLANS, pid) else {
                return Err(plan_not_found(pid));
            };
            let blocks = plan
                .get("blocks")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let cfg = PlannerConfig::default();
            let cap = plan
                .get("capacity")
                .and_then(Value::as_u64)
                .map_or_else(|| capacity(&cfg, &state), |c| c as u32);
            let daily: Vec<u32> = daily_totals(&blocks).into_iter().map(|(_, m)| m).collect();
            let warnings: Vec<String> = overcommit(&cfg, &daily, cap).into_iter().collect();
            ctx.ws
                .append("plan.accepted", json!({"plan_id": pid}))
                .map_err(ws_err)?;
            ctx.ws.project().map_err(ws_err)?;
            Ok(outcome(
                json!({
                    "plan_id": pid,
                    "accepted": true,
                    "blocks": blocks.len(),
                    "calendar_file": "schedule/lucubra-plan.ics",
                }),
                "start_session",
                warnings,
                limits,
            ))
        }
        "next" => {
            check_keys(args, &["action", "today", "progress"], "lucubra_plan(next)")?;
            let cfg = PlannerConfig::default();
            let start = today(args)?;
            let domain = state
                .get(col::PROFILE, "programme")
                .and_then(Value::as_str)
                .unwrap_or("");
            let items = work_items(&cfg, &state, start, domain);
            let cap = capacity(&cfg, &state);
            let first = schedule(&cfg, &items, &[cap], cap)
                .blocks
                .into_iter()
                .next();
            let study_now = first.map(|b| {
                json!({"subject": b.subject, "unit": b.unit, "minutes": b.minutes, "break_after": b.break_after})
            });
            Ok(outcome(
                json!({
                    "study_now": study_now,
                    "schedule_planning": !planning_off(&state),
                }),
                "start_session",
                Vec::new(),
                limits,
            ))
        }
        _ => unreachable!(),
    }
}
