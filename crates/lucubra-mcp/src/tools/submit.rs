// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_submit`: validation across all 6 submit actions and persistence for
//! `unit_summary`, `transcript`, `session_summary` and `classification`
//! (`SPEC 8.2`, `10.3`, `S23`).

use std::collections::BTreeSet;

use lucubra_core::state::{State, col};
use lucubra_ingest::vision::save_transcript;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::budget::BudgetLimits;
use crate::envelope::{ProgressEntry, ToolError, ToolErrorCode, validate_unit_ref};
use crate::material::resolve_workspace_path;
use crate::tools::{
    ToolContext, ToolOutcome, check_keys, check_slug, count_words, opt_bool, opt_str, opt_u64,
    req_str, require_action, resolve_file_hash,
};

/// JSON Schema arguments for `lucubra_submit`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SubmitArgs {
    /// Action: unit_summary, transcript, session_summary, classification, items, or grades.
    pub action: String,
    /// Subject slug (for unit_summary).
    #[serde(default)]
    pub subject: Option<String>,
    /// Unit slug (for unit_summary) or "<subject>/<unit>" (for items).
    #[serde(default)]
    pub unit: Option<String>,
    /// Unit display title (for unit_summary).
    #[serde(default)]
    pub title: Option<String>,
    /// Subject display title (for unit_summary).
    #[serde(default)]
    pub subject_title: Option<String>,
    /// Text summary (required for unit_summary and session_summary <= 150 words).
    #[serde(default)]
    pub summary: Option<String>,
    /// Concept list (for unit_summary).
    #[serde(default)]
    pub concepts: Option<Vec<String>>,
    /// Objectives list (for unit_summary).
    #[serde(default)]
    pub objectives: Option<Vec<String>>,
    /// Pitfalls list (for unit_summary).
    #[serde(default)]
    pub pitfalls: Option<Vec<String>>,
    /// Source references for unit_summary.
    #[serde(default)]
    pub sources: Option<Vec<Value>>,
    /// Source BLAKE3 hash or prefix (for transcript).
    #[serde(default)]
    pub file_hash: Option<String>,
    /// Workspace-relative source path (for transcript).
    #[serde(default)]
    pub path: Option<String>,
    /// 1-based page number for multi-page transcripts (omit for standalone images).
    #[serde(default)]
    pub page: Option<u64>,
    /// Extracted transcript markdown text (for transcript).
    #[serde(default)]
    pub text: Option<String>,
    /// Explicit verification override (defaults to false).
    #[serde(default)]
    pub verified: Option<bool>,
    /// Session id (for session_summary and grades).
    #[serde(default)]
    pub session_id: Option<String>,
    /// Classifications: `{path|file_hash, subject, unit?, confirmed_by_user?}`.
    #[serde(default)]
    pub classifications: Option<Vec<Value>>,
    /// Practice item mode: strict (default) or extended (M6).
    #[serde(default)]
    pub mode: Option<String>,
    /// Generated recall/practice items (M6).
    #[serde(default)]
    pub items: Option<Vec<Value>>,
    /// Rubric-checked grades (M6).
    #[serde(default)]
    pub grades: Option<Vec<Value>>,
    /// Optional completed-step checkpoints to log alongside this call.
    #[serde(default)]
    pub progress: Option<Vec<ProgressEntry>>,
}

fn ws_err(e: lucubra_core::Error) -> ToolError {
    ToolError::new(
        ToolErrorCode::WorkspaceError,
        format!("workspace submit failed: {e}"),
    )
}

fn hash_for_rel_path(state: &State, rel: &str) -> Option<String> {
    state
        .get(col::PATHS, rel)
        .and_then(|v| v.get("hash"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn is_assessment_file(state: &State, hash_or_prefix: &str) -> bool {
    let Ok(full) = resolve_file_hash(state, hash_or_prefix) else {
        return false;
    };
    let Some(file) = state.get(col::FILES, &full) else {
        return false;
    };
    if file.get("assessment").and_then(Value::as_bool) == Some(true) {
        return true;
    }
    if file
        .get("tags")
        .and_then(Value::as_array)
        .is_some_and(|tags| tags.iter().any(|t| t.as_str() == Some("assessment")))
    {
        return true;
    }
    if let Some(tag) = file.get("tag").and_then(Value::as_str)
        && tag.eq_ignore_ascii_case("assessment")
    {
        return true;
    }
    false
}

fn validate_string_array(
    args: &Map<String, Value>,
    key: &str,
) -> std::result::Result<Option<Vec<String>>, ToolError> {
    let Some(raw) = args.get(key) else {
        return Ok(None);
    };
    let Some(arr) = raw.as_array() else {
        return Err(ToolError::new(
            ToolErrorCode::InvalidArguments,
            format!("{key} must be an array of strings"),
        ));
    };
    let mut out = Vec::with_capacity(arr.len());
    for (idx, item) in arr.iter().enumerate() {
        let Some(s) = item.as_str().map(str::trim) else {
            return Err(ToolError::new(
                ToolErrorCode::InvalidArguments,
                format!("{key}[{idx}] must be a string"),
            ));
        };
        if !s.is_empty() {
            out.push(s.to_owned());
        }
    }
    Ok(Some(out))
}

pub fn execute(
    args: &Map<String, Value>,
    ctx: &ToolContext<'_>,
    limits: BudgetLimits,
) -> std::result::Result<ToolOutcome, ToolError> {
    let action = require_action(
        args,
        &[
            "unit_summary",
            "transcript",
            "session_summary",
            "classification",
            "items",
            "grades",
        ],
    )?;
    let state = ctx.ws.replay().map_err(ws_err)?.state;

    match action {
        "unit_summary" => {
            check_keys(
                args,
                &[
                    "action",
                    "subject",
                    "unit",
                    "title",
                    "subject_title",
                    "summary",
                    "concepts",
                    "objectives",
                    "pitfalls",
                    "sources",
                    "progress",
                ],
                "lucubra_submit(unit_summary)",
            )?;
            let Some(subject) = opt_str(args, "subject")? else {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "unit_summary requires 'subject', 'unit', and non-empty 'summary'",
                ));
            };
            let Some(unit) = opt_str(args, "unit")? else {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "unit_summary requires 'subject', 'unit', and non-empty 'summary'",
                ));
            };
            check_slug("subject", subject)?;
            check_slug("unit", unit)?;
            let summary = req_str(
                args,
                "summary",
                "unit_summary requires 'subject', 'unit', and non-empty 'summary'",
            )?;
            let title = opt_str(args, "title")?;
            let subject_title = opt_str(args, "subject_title")?;
            let concepts = validate_string_array(args, "concepts")?;
            let objectives = validate_string_array(args, "objectives")?;
            let pitfalls = validate_string_array(args, "pitfalls")?;
            let sources = match args.get("sources") {
                None | Some(Value::Null) => None,
                Some(Value::Array(arr)) => Some(Value::Array(arr.clone())),
                Some(_) => {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        "sources must be an array of {file_hash, pages?, locator?} objects",
                    ));
                }
            };

            let unit_key = format!("{subject}/{unit}");
            let event_kind = if state.get(col::UNITS, &unit_key).is_some() {
                "unit.updated"
            } else {
                "unit.created"
            };
            let mut payload = json!({
                "subject": subject,
                "unit": unit,
                "summary": summary,
            });
            if let Some(t) = title {
                payload["title"] = Value::String(t.to_owned());
            }
            if let Some(st) = subject_title {
                payload["subject_title"] = Value::String(st.to_owned());
            }
            if let Some(c) = concepts {
                payload["concepts"] = json!(c);
            }
            if let Some(o) = objectives {
                payload["objectives"] = json!(o);
            }
            if let Some(p) = pitfalls {
                payload["pitfalls"] = json!(p);
            }
            if let Some(s) = sources {
                payload["sources"] = s;
            }
            ctx.ws.append(event_kind, payload).map_err(ws_err)?;
            ctx.ws.project().map_err(ws_err)?;

            Ok(ToolOutcome {
                data: json!({
                    "event": event_kind,
                    "subject": subject,
                    "unit": unit,
                    "unit_card": format!("subjects/{subject}/{unit}/UNIT.md"),
                }),
                next: Some("read".into()),
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: None,
            })
        }
        "transcript" => {
            check_keys(
                args,
                &[
                    "action",
                    "file_hash",
                    "path",
                    "page",
                    "text",
                    "verified",
                    "progress",
                ],
                "lucubra_submit(transcript)",
            )?;
            let text = req_str(
                args,
                "text",
                "provide non-empty 'text' markdown alongside 'file_hash' or 'path'",
            )?;
            let page = opt_u64(args, "page", 1, 100_000, "page must be an integer >= 1")?
                .map(|p| p as u32);
            let verified =
                opt_bool(args, "verified", "verified must be true or false")?.unwrap_or(false);

            let full_hash = if let Some(h) = opt_str(args, "file_hash")? {
                resolve_file_hash(&state, h)?
            } else if let Some(p) = opt_str(args, "path")? {
                let (norm, _) = resolve_workspace_path(ctx.ws.root(), p)
                    .map_err(|msg| ToolError::new(ToolErrorCode::InvalidPath, msg))?;
                hash_for_rel_path(&state, &norm).ok_or_else(|| {
                    ToolError::new(
                        ToolErrorCode::NotFound,
                        format!("source path '{norm}' is not indexed; call lucubra_ingest(action=\"scan\") first"),
                    )
                })?
            } else {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "provide 'file_hash' or 'path' for lucubra_submit(action=\"transcript\")",
                ));
            };

            let saved_abs =
                save_transcript(ctx.ws, &full_hash, page, text, verified).map_err(|e| {
                    ToolError::new(
                        ToolErrorCode::WorkspaceError,
                        format!("failed saving transcript: {e}"),
                    )
                })?;
            let saved_rel = saved_abs
                .strip_prefix(ctx.ws.root())
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| saved_abs.to_string_lossy().into_owned());

            Ok(ToolOutcome {
                data: json!({
                    "file_hash": full_hash,
                    "page": page,
                    "verified": verified,
                    "transcript_path": saved_rel,
                }),
                next: None,
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: None,
            })
        }
        "session_summary" => {
            check_keys(
                args,
                &["action", "session_id", "summary", "progress"],
                "lucubra_submit(session_summary)",
            )?;
            let sid = req_str(
                args,
                "session_id",
                "provide 'session_id' and 'summary' (1-150 words)",
            )?;
            let summary = req_str(
                args,
                "summary",
                "provide 'session_id' and non-empty 'summary' (1-150 words)",
            )?;
            let words = count_words(summary);
            if words > 150 {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    format!("summary has {words} words; shorten it to at most 150 words"),
                ));
            }
            if sid == "_unattached" || state.get(col::SESSIONS, sid).is_none() {
                return Err(ToolError::new(
                    ToolErrorCode::NotFound,
                    format!("session '{sid}' not found; start or resume a session first"),
                ));
            }
            ctx.ws
                .append(
                    "handoff.written",
                    json!({ "session_id": sid, "summary": summary }),
                )
                .map_err(ws_err)?;
            Ok(ToolOutcome {
                data: json!({
                    "session_id": sid,
                    "words": words,
                }),
                next: None,
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: None,
            })
        }
        "classification" => {
            check_keys(
                args,
                &["action", "classifications", "progress"],
                "lucubra_submit(classification)",
            )?;
            let Some(arr) = args.get("classifications").and_then(Value::as_array) else {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "classifications must be a non-empty array of {path|file_hash, subject, unit?, confirmed_by_user?}",
                ));
            };
            if arr.is_empty() {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "classifications must not be empty",
                ));
            }
            let state = ctx.ws.replay().map_err(ws_err)?.state;
            // Validate every entry before writing any event.
            let mut planned = Vec::new();
            for (idx, item) in arr.iter().enumerate() {
                let Some(obj) = item.as_object() else {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        format!("classifications[{idx}] must be an object"),
                    ));
                };
                for key in obj.keys() {
                    if !matches!(
                        key.as_str(),
                        "path"
                            | "file_hash"
                            | "subject"
                            | "unit"
                            | "confidence"
                            | "confirmed_by_user"
                    ) {
                        return Err(ToolError::new(
                            ToolErrorCode::InvalidArguments,
                            format!("classifications[{idx}] has unknown key '{key}'"),
                        ));
                    }
                }
                let subj = obj
                    .get("subject")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .unwrap_or("");
                check_slug(&format!("classifications[{idx}].subject"), subj)?;
                if state.get(col::SUBJECTS, subj).is_none() {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        format!(
                            "classifications[{idx}].subject '{subj}' is not a known subject; add it with lucubra_profile(action=\"answer\", field=\"subjects\") first"
                        ),
                    ));
                }
                let unit = obj.get("unit").and_then(Value::as_str).map(str::trim);
                if let Some(u) = unit {
                    check_slug(&format!("classifications[{idx}].unit"), u)?;
                }
                let (path, hash) = if let Some(p) = obj.get("path").and_then(Value::as_str) {
                    let (norm, _) = resolve_workspace_path(ctx.ws.root(), p).map_err(|e| {
                        ToolError::new(
                            ToolErrorCode::InvalidArguments,
                            format!("classifications[{idx}].path: {e}"),
                        )
                    })?;
                    let hash = state
                        .get(col::PATHS, &norm)
                        .and_then(|l| l.get("hash"))
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            ToolError::new(
                                ToolErrorCode::NotFound,
                                format!(
                                    "classifications[{idx}].path '{norm}' has not been scanned"
                                ),
                            )
                        })?
                        .to_owned();
                    (norm, hash)
                } else if let Some(h) = obj.get("file_hash").and_then(Value::as_str) {
                    let hash = resolve_file_hash(&state, h)?;
                    let path = state
                        .collection(col::PATHS)
                        .find(|(_, l)| l.get("hash").and_then(Value::as_str) == Some(hash.as_str()))
                        .map(|(p, _)| p.clone())
                        .unwrap_or_default();
                    (path, hash)
                } else {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        format!("classifications[{idx}] requires 'path' or 'file_hash'"),
                    ));
                };
                let by_user = obj.get("confirmed_by_user").and_then(Value::as_bool) == Some(true);
                planned.push((
                    path,
                    hash,
                    subj.to_owned(),
                    unit.map(str::to_owned),
                    by_user,
                ));
            }
            let mut applied = 0usize;
            let mut needs_user = Vec::new();
            for (path, hash, subject, unit, by_user) in planned {
                if !by_user {
                    let engine_prior = state
                        .get(col::FILES, &hash)
                        .filter(|f| {
                            f.get("classified_by").and_then(Value::as_str) == Some("engine")
                        })
                        .and_then(|f| f.get("subject"))
                        .and_then(Value::as_str)
                        .map(|s| (s.to_owned(), lucubra_ingest::classify::STRONG));
                    let engine = engine_prior.or_else(|| {
                        lucubra_ingest::classify::candidates_for(ctx.ws, &state, &path, &hash)
                            .into_iter()
                            .next()
                            .filter(|c| c.score >= lucubra_ingest::classify::STRONG)
                            .map(|c| (c.subject, c.score))
                    });
                    if let Some((engine_subject, score)) = engine
                        && engine_subject != subject
                    {
                        needs_user.push(json!({
                            "path": path,
                            "host_subject": subject,
                            "engine_subject": engine_subject,
                            "engine_score": score,
                        }));
                        continue;
                    }
                }
                let mut payload = json!({
                    "hash": hash,
                    "subject": subject,
                    "by": if by_user { "user" } else { "host" },
                });
                if let Some(u) = unit {
                    payload["unit"] = Value::String(u);
                }
                ctx.ws.append("file.classified", payload).map_err(ws_err)?;
                applied += 1;
            }
            if applied > 0 {
                ctx.ws.project().map_err(ws_err)?;
            }
            let next = if needs_user.is_empty() {
                "approve_layout"
            } else {
                "ask_user"
            };
            Ok(ToolOutcome {
                data: json!({
                    "applied": applied,
                    "needs_user": needs_user,
                    "how": "For each needs_user entry, ask the user which subject is right, then resubmit it with confirmed_by_user=true.",
                }),
                next: Some(next.into()),
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: None,
            })
        }
        "items" => {
            check_keys(
                args,
                &["action", "unit", "mode", "items", "progress"],
                "lucubra_submit(items)",
            )?;
            let unit = req_str(
                args,
                "unit",
                "provide 'unit' as '<subject>/<unit>' (e.g., \"stats/u1\")",
            )?;
            if !validate_unit_ref(unit) {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidSlug,
                    "unit must be '<subject>/<unit>' using valid lowercase slugs",
                ));
            }
            let mode = opt_str(args, "mode")?.unwrap_or("strict");
            if !matches!(mode, "strict" | "extended") {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "mode must be 'strict' or 'extended'",
                ));
            }
            let Some(items) = args.get("items").and_then(Value::as_array) else {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "items must be a non-empty array of practice items",
                ));
            };
            if items.is_empty() {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "items must not be empty",
                ));
            }
            let paths = crate::tools::hash_to_path_map(&state);

            for (idx, item) in items.iter().enumerate() {
                let Some(obj) = item.as_object() else {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        format!("items[{idx}] must be an object"),
                    ));
                };
                if obj
                    .get("prompt")
                    .and_then(Value::as_str)
                    .is_none_or(|s| s.trim().is_empty())
                {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        format!("items[{idx}] requires non-empty 'prompt'"),
                    ));
                }
                if obj.get("answer").is_none() {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        format!("items[{idx}] requires 'answer'"),
                    ));
                }
                let is_solution = obj.get("solution").and_then(Value::as_bool) == Some(true)
                    || obj.get("type").and_then(Value::as_str) == Some("solution");

                let srcs = obj.get("sources").and_then(Value::as_array);
                if mode == "strict" && srcs.is_none_or(Vec::is_empty) {
                    return Err(ToolError::new(
                        ToolErrorCode::GroundingFailed,
                        format!(
                            "strict-mode item[{idx}] requires at least one source with {{file_hash, location, anchor}} and anchor <= 25 words"
                        ),
                    ));
                }
                if let Some(src_arr) = srcs {
                    let mut cited_hashes = BTreeSet::new();
                    for (sidx, src) in src_arr.iter().enumerate() {
                        let Some(sobj) = src.as_object() else {
                            return Err(ToolError::new(
                                ToolErrorCode::InvalidArguments,
                                format!("items[{idx}].sources[{sidx}] must be an object"),
                            ));
                        };
                        let fhash = sobj
                            .get("file_hash")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .ok_or_else(|| {
                                ToolError::new(
                                    ToolErrorCode::GroundingFailed,
                                    format!(
                                        "strict-mode item[{idx}] requires at least one source with {{file_hash, location, anchor}} and anchor <= 25 words"
                                    ),
                                )
                            })?;
                        let location = sobj
                            .get("location")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|s| !s.is_empty());
                        let anchor = sobj
                            .get("anchor")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|s| !s.is_empty());
                        if mode == "strict" && (location.is_none() || anchor.is_none()) {
                            return Err(ToolError::new(
                                ToolErrorCode::GroundingFailed,
                                format!(
                                    "strict-mode item[{idx}] requires at least one source with {{file_hash, location, anchor}} and anchor <= 25 words"
                                ),
                            ));
                        }
                        if let Some(anc) = anchor
                            && count_words(anc) > 25
                        {
                            return Err(ToolError::new(
                                ToolErrorCode::GroundingFailed,
                                format!(
                                    "strict-mode item[{idx}] anchor exceeds 25 words (shorten the exact quote)"
                                ),
                            ));
                        }
                        if is_solution && is_assessment_file(&state, fhash) {
                            let full = resolve_file_hash(&state, fhash).unwrap_or_default();
                            let path_label = paths.get(&full).map(String::as_str).unwrap_or(fhash);
                            return Err(ToolError::new(
                                ToolErrorCode::IntegrityRejected,
                                format!(
                                    "file '{path_label}' is tagged assessment (tutor mode); submit guiding questions or hints instead of a solution"
                                ),
                            ));
                        }
                        cited_hashes.insert(fhash.to_owned());
                    }
                    let _ = cited_hashes;
                }
            }
            Err(ToolError::new(
                ToolErrorCode::NotImplemented,
                "Anchor verification and practice item storage are implemented in milestone M6.",
            ))
        }
        "grades" => {
            check_keys(
                args,
                &["action", "session_id", "grades", "progress"],
                "lucubra_submit(grades)",
            )?;
            let _sid = opt_str(args, "session_id")?;
            let Some(grades) = args.get("grades").and_then(Value::as_array) else {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "grades must be a non-empty array of {item_id, criteria_scores, total, feedback?}",
                ));
            };
            if grades.is_empty() {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "grades must not be empty",
                ));
            }
            for (idx, g) in grades.iter().enumerate() {
                let Some(obj) = g.as_object() else {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        format!("grades[{idx}] must be an object"),
                    ));
                };
                if obj
                    .get("item_id")
                    .and_then(Value::as_str)
                    .is_none_or(|s| s.trim().is_empty())
                {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        format!("grades[{idx}] requires 'item_id'"),
                    ));
                }
                let Some(total) = obj.get("total").and_then(Value::as_f64) else {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        format!("grades[{idx}] requires numeric 'total'"),
                    ));
                };
                let Some(criteria) = obj.get("criteria_scores").and_then(Value::as_array) else {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        format!("grades[{idx}] requires 'criteria_scores' array"),
                    ));
                };
                let mut sum = 0.0f64;
                for (cidx, c) in criteria.iter().enumerate() {
                    let Some(cobj) = c.as_object() else {
                        return Err(ToolError::new(
                            ToolErrorCode::InvalidArguments,
                            format!("grades[{idx}].criteria_scores[{cidx}] must be an object"),
                        ));
                    };
                    let score = cobj.get("score").and_then(Value::as_f64).unwrap_or(-1.0);
                    let max_pts = cobj
                        .get("max_points")
                        .and_then(Value::as_f64)
                        .unwrap_or(-1.0);
                    if score < 0.0 || max_pts < 0.0 || score > max_pts {
                        return Err(ToolError::new(
                            ToolErrorCode::InvalidArguments,
                            format!("grades[{idx}]: each score must be in 0..=max_points"),
                        ));
                    }
                    sum += score;
                }
                if (sum - total).abs() > 1e-6 {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        format!(
                            "grades[{idx}]: total ({total}) must equal sum of criterion scores ({sum}) and each score must be in 0..=max_points"
                        ),
                    ));
                }
            }
            Err(ToolError::new(
                ToolErrorCode::NotImplemented,
                "Rubric grading and FSRS mastery updates are implemented in milestone M6.",
            ))
        }
        _ => unreachable!(),
    }
}
