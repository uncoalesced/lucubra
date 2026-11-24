// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Registry of the 11 MCP tools, raw JSON pre-validation (`S23`), and shared helpers.

pub mod export;
pub mod ingest;
pub mod orient;
pub mod plan;
pub mod practice;
pub mod profile;
pub mod read;
pub mod search;
pub mod session;
pub mod status;
pub mod submit;

use std::collections::BTreeMap;

use lucubra_core::Workspace;
use lucubra_core::projection::is_slug;
use lucubra_core::state::{State, col};
use lucubra_ingest::worker::Supervisor;
use rmcp::handler::server::tool::schema_for_input;
use rmcp::model::{Tool, ToolAnnotations};
use serde_json::{Map, Value};

use crate::budget::{BudgetLimits, resolve_tier};
use crate::envelope::{ToolError, ToolErrorCode, append_progress_entries, parse_progress_array};
use crate::host::DetectedHost;

/// Ordered list of every tool name exposed by Lucubra.
pub const ALL_TOOL_NAMES: &[&str] = &[
    "lucubra_orient",
    "lucubra_profile",
    "lucubra_ingest",
    "lucubra_read",
    "lucubra_search",
    "lucubra_submit",
    "lucubra_plan",
    "lucubra_session",
    "lucubra_practice",
    "lucubra_export",
    "lucubra_status",
];

/// Runtime context supplied to synchronous tool handlers inside `spawn_blocking`.
pub struct ToolContext<'a> {
    /// Open workspace handle.
    pub ws: &'a Workspace,
    /// Host detected during `initialize`.
    pub host: &'a DetectedHost,
    /// Active session id tracked by `SessionRuntime`.
    pub active_session_id: Option<&'a str>,
    /// Lazy Python worker supervisor (provided for `lucubra_ingest` calls).
    pub supervisor: Option<&'a mut Supervisor>,
}

/// Result produced by an individual tool handler prior to envelope formatting.
#[derive(Debug, Clone)]
pub struct ToolOutcome {
    /// Structured JSON payload serialized under `envelope.data`.
    pub data: Value,
    /// Suggested next action string.
    pub next: Option<String>,
    /// Non-fatal warnings.
    pub warnings: Vec<String>,
    /// Tier per-call token cap applied to this response.
    pub per_call_limit: usize,
    /// True when this call is closing a session (`lucubra_session(action="end")`).
    pub suppress_wrap_up: bool,
    /// Optional update to `SessionRuntime::active_session_id`.
    pub active_session_override: Option<Option<String>>,
}

/// Complete synchronous dispatch bundle returned to `LucubraServer::call_tool`.
pub struct DispatchResult {
    /// Success or structured `S23` error.
    pub outcome: std::result::Result<ToolOutcome, ToolError>,
    /// Resolved tier limits for this call.
    pub limits: BudgetLimits,
    /// Whether `wrap_up` nudge should be suppressed (`lucubra_session(action="end")`).
    pub suppress_wrap_up: bool,
    /// Optional update to `SessionRuntime::active_session_id`.
    pub session_update: Option<Option<String>>,
}

fn tool_def<T: schemars::JsonSchema + std::any::Any>(
    name: &'static str,
    description: &'static str,
    read_only: bool,
    idempotent: bool,
) -> Tool {
    let schema = schema_for_input::<T>().unwrap_or_else(|e| panic!("schema for {name}: {e}"));
    let annotations = ToolAnnotations::new()
        .read_only(read_only)
        .destructive(false)
        .idempotent(idempotent)
        .open_world(false);
    Tool::new(name, description, schema).with_annotations(annotations)
}

/// Builds the 11 static [`Tool`] definitions advertised by `tools/list`.
///
/// Descriptions contain zero workspace file content (`SPEC section 22`, `S29`).
pub fn build_tools() -> Vec<Tool> {
    vec![
        tool_def::<orient::OrientArgs>(
            "lucubra_orient",
            "Call first in every conversation. Returns host profile, user preferences, subjects, exams, file counts, inbox count, active session state, and next suggested step.",
            true,
            true,
        ),
        tool_def::<profile::ProfileArgs>(
            "lucubra_profile",
            "Inspect or update onboarding profile answers, subjects, exams, and preferences (actions: next, answer, skip, get, set).",
            false,
            false,
        ),
        tool_def::<ingest::IngestArgs>(
            "lucubra_ingest",
            "Scan inbox/ files, check extraction/vision status, or page pending vision items (actions: scan, status, approve_layout, undo_moves, fetch).",
            false,
            false,
        ),
        tool_def::<read::ReadArgs>(
            "lucubra_read",
            "Read workspace indexes, unit cards, extracted chunks, or vision transcripts progressively (actions: index, subject, unit, chunk, transcript).",
            true,
            true,
        ),
        tool_def::<search::SearchArgs>(
            "lucubra_search",
            "Search extracted study text via FTS5 (word, prefix, or substring modes) with optional subject/unit filters and untrusted material wrappers.",
            true,
            true,
        ),
        tool_def::<submit::SubmitArgs>(
            "lucubra_submit",
            "Submit generated study artifacts: unit_summary, transcript, session_summary, classification, items (grounded by quoted anchors), grades (rubric scores), or tag (assessment on or off).",
            false,
            false,
        ),
        tool_def::<plan::PlanArgs>(
            "lucubra_plan",
            "Plan study time: propose a schedule from exams, material and busy time, edit it, accept it (writes schedule/lucubra-plan.ics), view a 30-day horizon, or get what to study now (actions: propose, edit, accept, horizon, next).",
            false,
            false,
        ),
        tool_def::<session::SessionArgs>(
            "lucubra_session",
            "Manage interactive study sessions and step logging (actions: start, step, end, resume).",
            false,
            false,
        ),
        tool_def::<practice::PracticeArgs>(
            "lucubra_practice",
            "Practise with the FSRS queue, answer items (closed formats graded by the engine, open formats by rubric), or run a timed adaptive exam simulation (actions: next, answer, exam_sim).",
            false,
            false,
        ),
        tool_def::<export::ExportArgs>(
            "lucubra_export",
            "Generate session briefs and host-appropriate exports (actions: brief, pack, anki, ics, dashboard, mock_test).",
            false,
            false,
        ),
        tool_def::<status::StatusArgs>(
            "lucubra_status",
            "Return workspace counts, indexed file totals, logged study minutes, and retention placeholders.",
            true,
            true,
        ),
    ]
}

/// Dispatches one raw MCP `tools/call` synchronously with `S23` pre-validation
/// and atomic `progress` checkpoint persistence.
pub fn dispatch_sync(
    tool_name: &str,
    raw_args: Option<Map<String, Value>>,
    ws: &Workspace,
    host: &DetectedHost,
    active_session_id: Option<&str>,
    supervisor: Option<&mut Supervisor>,
) -> DispatchResult {
    let state = ws.replay().map(|r| r.state).ok();
    let limits = resolve_tier(
        state.as_ref().and_then(|s| s.collections.get(col::PROFILE)),
        host.profile.id,
        host.profile.default_tier,
    )
    .limits();

    if !ALL_TOOL_NAMES.contains(&tool_name) {
        let hint = closest(tool_name, ALL_TOOL_NAMES, 4)
            .map(|m| format!(" (did you mean '{m}'?)"))
            .unwrap_or_default();
        return DispatchResult {
            outcome: Err(ToolError::new(
                ToolErrorCode::UnknownTool,
                format!(
                    "unknown tool '{tool_name}'{hint}; tool must be one of: {}",
                    ALL_TOOL_NAMES.join(", ")
                ),
            )),
            limits,
            suppress_wrap_up: false,
            session_update: None,
        };
    }

    let args = raw_args.unwrap_or_default();
    let progress = match parse_progress_array(&args) {
        Ok(entries) => entries,
        Err(err) => {
            return DispatchResult {
                outcome: Err(err),
                limits,
                suppress_wrap_up: false,
                session_update: None,
            };
        }
    };

    let ctx = ToolContext {
        ws,
        host,
        active_session_id,
        supervisor,
    };

    let mut result = match tool_name {
        "lucubra_orient" => orient::execute(&args, &ctx, limits),
        "lucubra_profile" => profile::execute(&args, &ctx, limits),
        "lucubra_ingest" => ingest::execute(&args, ctx, limits),
        "lucubra_read" => read::execute(&args, &ctx, limits),
        "lucubra_search" => search::execute(&args, &ctx, limits),
        "lucubra_submit" => submit::execute(&args, &ctx, limits),
        "lucubra_plan" => plan::execute(&args, &ctx, limits),
        "lucubra_session" => session::execute(&args, &ctx, limits),
        "lucubra_practice" => practice::execute(&args, &ctx, limits),
        "lucubra_export" => export::execute(&args, &ctx, limits),
        "lucubra_status" => status::execute(&args, &ctx, limits),
        _ => unreachable!("checked against ALL_TOOL_NAMES"),
    };

    let suppress_wrap_up = result.as_ref().map(|o| o.suppress_wrap_up).unwrap_or(false);
    let session_update = result
        .as_ref()
        .ok()
        .and_then(|o| o.active_session_override.clone());

    if let Ok(ref mut out) = result
        && !progress.is_empty()
    {
        let eff_session = out
            .active_session_override
            .as_ref()
            .and_then(|opt| opt.as_deref())
            .or(active_session_id);
        match append_progress_entries(ws, eff_session, &progress) {
            Ok(prog_warnings) => out.warnings.extend(prog_warnings),
            Err(err) => {
                return DispatchResult {
                    outcome: Err(err),
                    limits,
                    suppress_wrap_up: false,
                    session_update: None,
                };
            }
        }
    }

    DispatchResult {
        outcome: result,
        limits,
        suppress_wrap_up,
        session_update,
    }
}

/// Computes Levenshtein edit distance on lowercase ASCII/Unicode chars.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let ac: Vec<char> = a.chars().collect();
    let bc: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=bc.len()).collect();
    let mut curr = vec![0usize; bc.len() + 1];
    for (i, &ca) in ac.iter().enumerate() {
        curr[0] = i + 1;
        for (j, &cb) in bc.iter().enumerate() {
            let cost = usize::from(ca != cb);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[bc.len()]
}

/// Returns the closest candidate within `max_dist` (preferring prefix matches).
pub fn closest<'a>(needle: &str, candidates: &[&'a str], max_dist: usize) -> Option<&'a str> {
    let lower = needle.to_ascii_lowercase();
    let mut best: Option<(usize, &'a str)> = None;
    for &cand in candidates {
        let clower = cand.to_ascii_lowercase();
        let d = if clower.starts_with(&lower) && !lower.is_empty() {
            1
        } else {
            edit_distance(&lower, &clower)
        };
        if d <= max_dist && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, cand));
        }
    }
    best.map(|(_, cand)| cand)
}

/// Rejects keys not in `allowed`, emitting typo recovery hints when possible.
pub fn check_keys(
    args: &Map<String, Value>,
    allowed: &[&str],
    context_label: &str,
) -> std::result::Result<(), ToolError> {
    for key in args.keys() {
        if allowed.contains(&key.as_str()) {
            continue;
        }
        let hint = closest(key, allowed, 3)
            .map(|m| format!(" (did you mean '{m}'?)"))
            .unwrap_or_default();
        return Err(ToolError::new(
            ToolErrorCode::InvalidArguments,
            format!(
                "unknown field '{key}' for {context_label}{hint}; allowed keys: {}",
                allowed.join(", ")
            ),
        ));
    }
    Ok(())
}

/// Validates required `action` parameter against `allowed`.
pub fn require_action<'a>(
    args: &'a Map<String, Value>,
    allowed: &[&str],
) -> std::result::Result<&'a str, ToolError> {
    let Some(raw) = args.get("action") else {
        return Err(ToolError::new(
            ToolErrorCode::InvalidAction,
            format!(
                "missing required field 'action'; set action to one of: {}",
                allowed.join(", ")
            ),
        ));
    };
    let Some(act) = raw.as_str().map(str::trim) else {
        return Err(ToolError::new(
            ToolErrorCode::InvalidAction,
            format!(
                "field 'action' must be a string, one of: {}",
                allowed.join(", ")
            ),
        ));
    };
    if !allowed.contains(&act) {
        let hint = closest(act, allowed, 3)
            .map(|m| format!(" (did you mean '{m}'?)"))
            .unwrap_or_default();
        return Err(ToolError::new(
            ToolErrorCode::InvalidAction,
            format!(
                "unknown action '{act}'; action must be one of: {}{hint}",
                allowed.join(", ")
            ),
        ));
    }
    Ok(act)
}

/// Validates an optional string field (rejecting non-string JSON types).
pub fn opt_str<'a>(
    args: &'a Map<String, Value>,
    key: &str,
) -> std::result::Result<Option<&'a str>, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => {
            let t = s.trim();
            Ok((!t.is_empty()).then_some(t))
        }
        Some(_) => Err(ToolError::new(
            ToolErrorCode::InvalidArguments,
            format!("field '{key}' must be a string"),
        )),
    }
}

/// Requires a non-empty string field.
pub fn req_str<'a>(
    args: &'a Map<String, Value>,
    key: &str,
    fix: &str,
) -> std::result::Result<&'a str, ToolError> {
    opt_str(args, key)?.ok_or_else(|| ToolError::new(ToolErrorCode::InvalidArguments, fix))
}

/// Validates an optional bounded unsigned integer field.
pub fn opt_u64(
    args: &Map<String, Value>,
    key: &str,
    min: u64,
    max: u64,
    fix: &str,
) -> std::result::Result<Option<u64>, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => {
            let Some(val) = n.as_u64() else {
                return Err(ToolError::new(ToolErrorCode::InvalidArguments, fix));
            };
            if val < min || val > max {
                return Err(ToolError::new(ToolErrorCode::InvalidArguments, fix));
            }
            Ok(Some(val))
        }
        Some(_) => Err(ToolError::new(ToolErrorCode::InvalidArguments, fix)),
    }
}

/// Validates an optional boolean field.
pub fn opt_bool(
    args: &Map<String, Value>,
    key: &str,
    fix: &str,
) -> std::result::Result<Option<bool>, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(ToolError::new(ToolErrorCode::InvalidArguments, fix)),
    }
}

/// Validates an identifier slug (`subject`, `unit`).
pub fn check_slug(field: &str, val: &str) -> std::result::Result<(), ToolError> {
    if !is_slug(val) {
        return Err(ToolError::new(
            ToolErrorCode::InvalidSlug,
            format!(
                "{field} must be 1-64 lowercase ASCII letters, digits, '-' or '_' (no reserved names or paths)"
            ),
        ));
    }
    Ok(())
}

/// Counts whitespace-delimited words.
pub fn count_words(text: &str) -> usize {
    text.split_whitespace().count()
}

/// Maps content hashes to the first deterministic workspace-relative path in `col::PATHS`.
pub fn hash_to_path_map(state: &State) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for (rel, entry) in state.collection(col::PATHS) {
        if let Some(h) = entry.get("hash").and_then(Value::as_str) {
            map.entry(h.to_owned()).or_insert_with(|| rel.clone());
        }
    }
    map
}

/// Files that look like assessments and await the user's yes or no (S26),
/// as `needs_user` questions.
pub fn assessment_questions(ws: &Workspace, state: &State) -> Vec<Value> {
    lucubra_ingest::classify::assessment_suggestions(ws, state)
        .into_iter()
        .map(|(path, hash)| {
            serde_json::json!({
                "kind": "assessment_tag",
                "path": path,
                "file_hash": hash,
                "question": format!(
                    "'{path}' looks like graded coursework. Tag it as an assessment so help on it stays to hints and checks (tutor mode)? Answer with lucubra_submit(action=\"tag\", path, assessment=true or false)."
                ),
            })
        })
        .collect()
}

/// Tutor-mode guidance attached to reads and hits from an assessment-tagged
/// file (S26, S40).
pub const TUTOR_GUIDANCE: &str = "This file is tagged as an assessment. Give hints, guiding questions and checks of the student's own working; never write an answer they could submit.";

/// Whether the file with full hash `hash` is tagged as an assessment.
pub fn is_assessment(state: &State, hash: &str) -> bool {
    let Some(file) = state.get(col::FILES, hash) else {
        return false;
    };
    file.get("assessment").and_then(Value::as_bool) == Some(true)
        || file
            .get("tags")
            .and_then(Value::as_array)
            .is_some_and(|tags| tags.iter().any(|t| t.as_str() == Some("assessment")))
        || file
            .get("tag")
            .and_then(Value::as_str)
            .is_some_and(|t| t.eq_ignore_ascii_case("assessment"))
}

/// `tutor_mode` block for a response about `hash`, if it is an assessment.
pub fn tutor_mode(state: &State, hash: &str) -> Option<Value> {
    is_assessment(state, hash)
        .then(|| serde_json::json!({"active": true, "guidance": TUTOR_GUIDANCE}))
}

/// Resolves a full 64-char hash or unique prefix across `col::FILES`.
pub fn resolve_file_hash(
    state: &State,
    hash_or_prefix: &str,
) -> std::result::Result<String, ToolError> {
    let trimmed = hash_or_prefix.trim();
    if trimmed.is_empty() {
        return Err(ToolError::new(
            ToolErrorCode::InvalidArguments,
            "file_hash must not be empty",
        ));
    }
    if state.get(col::FILES, trimmed).is_some() {
        return Ok(trimmed.to_owned());
    }
    let matches: Vec<String> = state
        .collection(col::FILES)
        .map(|(k, _)| k.clone())
        .filter(|k| k.starts_with(trimmed))
        .collect();
    match matches.as_slice() {
        [only] => Ok(only.clone()),
        [] => Err(ToolError::new(
            ToolErrorCode::NotFound,
            format!(
                "no file matches file_hash '{trimmed}'; call lucubra_search or lucubra_ingest first"
            ),
        )),
        _ => Err(ToolError::new(
            ToolErrorCode::InvalidArguments,
            format!(
                "file_hash prefix '{trimmed}' matched multiple files; pass the full 64-char hash"
            ),
        )),
    }
}
