// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_submit`: validation and persistence for unit summaries,
//! transcripts, session summaries, classifications, grounded practice items,
//! rubric grades and assessment tags (`SPEC 8.2`, `10.3`, `13`, `S23`).

use lucubra_core::state::{State, col};
use lucubra_ingest::pipeline::open_index;
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
    /// Action: unit_summary, transcript, session_summary, classification, items, grades, or tag.
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
    /// Practice item mode: strict (default) or extended.
    #[serde(default)]
    pub mode: Option<String>,
    /// Generated practice items: {type, prompt, answer, options?, accepted?, tolerance?, unit?, rubric?, difficulty?, sources: [{file_hash, location, anchor}], retry_of?}.
    #[serde(default)]
    pub items: Option<Vec<Value>>,
    /// Assessment tag for `tag`: true turns tutor mode on, false off.
    #[serde(default)]
    pub assessment: Option<bool>,
    /// Rubric-checked grades.
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
    resolve_file_hash(state, hash_or_prefix)
        .is_ok_and(|full| crate::tools::is_assessment(state, &full))
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

/// Item types the engine stores (SPEC section 13.1).
const ITEM_TYPES: &[&str] = &[
    "mcq_single",
    "mcq_multi",
    "cloze",
    "true_false",
    "matching",
    "numeric",
    "ordering",
    "short_answer",
    "long_answer",
    "viva_prompt",
    "case_stem",
    "diagram_label",
    "formula_recall",
];

/// Open types the host grades against a stored rubric.
const RUBRIC_TYPES: &[&str] = &["short_answer", "long_answer", "viva_prompt", "case_stem"];

fn grounding_failed(msg: impl Into<String>) -> ToolError {
    ToolError::new(ToolErrorCode::GroundingFailed, msg)
}

/// Structural checks for one submitted item; grounding is checked later.
fn check_item_shape<'a>(
    idx: usize,
    item: &'a Value,
    mode: &str,
) -> std::result::Result<&'a Map<String, Value>, ToolError> {
    let invalid = |msg: String| ToolError::new(ToolErrorCode::InvalidArguments, msg);
    let Some(obj) = item.as_object() else {
        return Err(invalid(format!("items[{idx}] must be an object")));
    };
    if obj
        .get("prompt")
        .and_then(Value::as_str)
        .is_none_or(|s| s.trim().is_empty())
    {
        return Err(invalid(format!("items[{idx}] requires non-empty 'prompt'")));
    }
    if obj.get("answer").is_none() {
        return Err(invalid(format!("items[{idx}] requires 'answer'")));
    }
    let kind = obj.get("type").and_then(Value::as_str).unwrap_or_default();
    let solution = kind == "solution" || obj.get("solution").and_then(Value::as_bool) == Some(true);
    if !solution && !ITEM_TYPES.contains(&kind) {
        return Err(invalid(format!(
            "items[{idx}].type must be one of: {}",
            ITEM_TYPES.join(", ")
        )));
    }
    if RUBRIC_TYPES.contains(&kind)
        && obj
            .get("rubric")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty)
    {
        return Err(invalid(format!(
            "items[{idx}] of type {kind} requires 'rubric' as [{{criterion, max_points}}]"
        )));
    }
    let shape = format!(
        "strict-mode item[{idx}] requires at least one source with {{file_hash, location, anchor}} and anchor <= 25 words"
    );
    let sources = obj.get("sources").and_then(Value::as_array);
    if mode == "strict" && sources.is_none_or(Vec::is_empty) {
        return Err(grounding_failed(shape));
    }
    for (sidx, src) in sources.into_iter().flatten().enumerate() {
        let Some(sobj) = src.as_object() else {
            return Err(invalid(format!(
                "items[{idx}].sources[{sidx}] must be an object"
            )));
        };
        let field = |k: &str| {
            sobj.get(k)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
        };
        if field("file_hash").is_none()
            || (mode == "strict" && (field("location").is_none() || field("anchor").is_none()))
        {
            return Err(grounding_failed(shape));
        }
        if field("anchor").is_some_and(|a| count_words(a) > 25) {
            return Err(grounding_failed(format!(
                "strict-mode item[{idx}] anchor exceeds 25 words (shorten the exact quote)"
            )));
        }
    }
    Ok(obj)
}

/// Stable identity of an item across one regeneration: unit, type and
/// normalised prompt. The host passes `retry_of` when it rewrites the prompt.
fn item_key(unit: &str, obj: &Map<String, Value>) -> String {
    let prompt = obj
        .get("prompt")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let norm = lucubra_core::grounding::tokens(prompt).join(" ");
    let kind = obj.get("type").and_then(Value::as_str).unwrap_or_default();
    let digest = blake3::hash(format!("{unit}\n{kind}\n{norm}").as_bytes());
    format!("ik_{}", &digest.to_hex()[..16])
}

/// Verifies every source of a strict item against the index (SPEC section
/// 13.2, D53). Returns the grounding label or the first failure reason.
fn ground_sources(
    state: &State,
    index: Option<&lucubra_ingest::index::Index>,
    sources: &[Value],
) -> std::result::Result<&'static str, String> {
    use lucubra_core::grounding::{verify_cell, verify_name, verify_text};
    let index = index.ok_or("the search index is unavailable; run lucubra_ingest scan first")?;
    let mut transcript_only = true;
    for (sidx, src) in sources.iter().enumerate() {
        let fail = |reason: String| format!("sources[{sidx}]: {reason}");
        let text = |k: &str| {
            src.get(k)
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default()
        };
        let full = resolve_file_hash(state, text("file_hash")).map_err(|e| fail(e.fix))?;
        let (loc, anchor) = (text("location"), text("anchor"));
        let locators = index
            .locators_for_hash(&full, 100_000)
            .map_err(|e| fail(format!("index read failed: {e}")))?;
        let chunk = |l: &str| index.chunk(&full, l).ok().flatten();
        if let Some(page) = loc.strip_suffix(":transcript") {
            let body = chunk(loc).ok_or_else(|| fail(format!("no transcript at '{loc}'")))?;
            verify_text(anchor, &[&body]).map_err(|r| fail(r.to_string()))?;
            let key = match page.strip_prefix("p:") {
                Some(n) => format!("{full}/p:{n}"),
                None => format!("{full}/img"),
            };
            let verified = state
                .get(col::TRANSCRIPTS, &key)
                .and_then(|t| t.get("verified"))
                .and_then(Value::as_bool)
                == Some(true);
            transcript_only &= !verified;
            continue;
        }
        transcript_only = false;
        if let Some((sheet, cell)) = loc.strip_prefix("x:").and_then(|r| r.split_once('!'))
            && !cell.contains(':')
            && cell != "summary"
        {
            let prefix = format!("x:{sheet}!");
            let bodies: Vec<String> = locators
                .iter()
                .filter(|l| l.starts_with(&prefix))
                .filter_map(|l| chunk(l))
                .collect();
            let refs: Vec<&str> = bodies.iter().map(String::as_str).collect();
            verify_cell(cell, anchor, &refs).map_err(|r| fail(r.to_string()))?;
        } else if loc.starts_with("l:") || loc.starts_with("o:") {
            verify_name(loc, &locators).map_err(|r| fail(r.to_string()))?;
        } else {
            let pos = locators
                .iter()
                .position(|l| l == loc)
                .ok_or_else(|| fail(format!("location '{loc}' does not exist in that file")))?;
            let near = &locators[pos.saturating_sub(1)..(pos + 2).min(locators.len())];
            let bodies: Vec<String> = near.iter().filter_map(|l| chunk(l)).collect();
            let refs: Vec<&str> = bodies.iter().map(String::as_str).collect();
            verify_text(anchor, &refs).map_err(|r| fail(r.to_string()))?;
        }
    }
    Ok(if transcript_only {
        "unverified_transcript"
    } else {
        "verified"
    })
}

/// Checks one host grade against the stored rubric: every criterion scored
/// within its range and a consistent total. Returns `(item_id, total, max)`.
fn check_grade(
    idx: usize,
    g: &Value,
    state: &State,
) -> std::result::Result<(String, f64, f64), ToolError> {
    let invalid = |msg: String| ToolError::new(ToolErrorCode::InvalidArguments, msg);
    let Some(obj) = g.as_object() else {
        return Err(invalid(format!("grades[{idx}] must be an object")));
    };
    let Some(item_id) = obj
        .get("item_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Err(invalid(format!("grades[{idx}] requires 'item_id'")));
    };
    let Some(item) = state.get(col::ITEMS, item_id) else {
        return Err(ToolError::new(
            ToolErrorCode::NotFound,
            format!("grades[{idx}]: no practice item '{item_id}'"),
        ));
    };
    let Some(total) = obj.get("total").and_then(Value::as_f64) else {
        return Err(invalid(format!("grades[{idx}] requires numeric 'total'")));
    };
    let Some(criteria) = obj.get("criteria_scores").and_then(Value::as_array) else {
        return Err(invalid(format!(
            "grades[{idx}] requires 'criteria_scores' array"
        )));
    };
    let rubric: Vec<(String, f64)> = item
        .get("rubric")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| {
            Some((
                c.get("criterion")?.as_str()?.to_owned(),
                c.get("max_points")?.as_f64()?,
            ))
        })
        .collect();
    let (mut sum, mut max_total) = (0.0f64, 0.0f64);
    for (cidx, c) in criteria.iter().enumerate() {
        let Some(cobj) = c.as_object() else {
            return Err(invalid(format!(
                "grades[{idx}].criteria_scores[{cidx}] must be an object"
            )));
        };
        let score = cobj.get("score").and_then(Value::as_f64).unwrap_or(-1.0);
        let named = cobj.get("criterion").and_then(Value::as_str);
        let max_pts = named
            .and_then(|n| rubric.iter().find(|(r, _)| r == n).map(|(_, m)| *m))
            .or_else(|| cobj.get("max_points").and_then(Value::as_f64))
            .unwrap_or(-1.0);
        if score < 0.0 || max_pts < 0.0 || score > max_pts {
            return Err(invalid(format!(
                "grades[{idx}]: each score must be in 0..=max_points"
            )));
        }
        sum += score;
        max_total += max_pts;
    }
    let scored: Vec<&str> = criteria
        .iter()
        .filter_map(|c| c.get("criterion").and_then(Value::as_str))
        .collect();
    if let Some((missing, _)) = rubric.iter().find(|(r, _)| !scored.contains(&r.as_str())) {
        return Err(invalid(format!(
            "grades[{idx}]: rubric criterion '{missing}' is not scored"
        )));
    }
    if (sum - total).abs() > 1e-6 {
        return Err(invalid(format!(
            "grades[{idx}]: total ({total}) must equal sum of criterion scores ({sum}) and each score must be in 0..=max_points"
        )));
    }
    Ok((item_id.to_owned(), total, max_total))
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
            "tag",
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
            let items = match args.get("items").and_then(Value::as_array) {
                Some(items) if !items.is_empty() => items,
                Some(_) => {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        "items must not be empty",
                    ));
                }
                None => {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        "items must be a non-empty array of practice items",
                    ));
                }
            };
            let paths = crate::tools::hash_to_path_map(&state);
            let index = open_index(ctx.ws).ok();
            let (mut created, mut retry, mut dropped) = (Vec::new(), Vec::new(), Vec::new());

            for (idx, item) in items.iter().enumerate() {
                let obj = check_item_shape(idx, item, mode)?;
                let is_solution = obj.get("solution").and_then(Value::as_bool) == Some(true)
                    || obj.get("type").and_then(Value::as_str) == Some("solution");
                let sources = obj
                    .get("sources")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for src in &sources {
                    let fhash = src
                        .get("file_hash")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or_default();
                    if is_solution && is_assessment_file(&state, fhash) {
                        let full = resolve_file_hash(&state, fhash).unwrap_or_default();
                        let label = paths.get(&full).map_or(fhash, String::as_str);
                        return Err(ToolError::new(
                            ToolErrorCode::IntegrityRejected,
                            format!(
                                "file '{label}' is tagged assessment (tutor mode); submit guiding questions or hints instead of a solution"
                            ),
                        ));
                    }
                }

                let key = obj
                    .get("retry_of")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| item_key(unit, obj));
                let grounding = if mode == "extended" {
                    Ok("beyond_notes")
                } else {
                    ground_sources(&state, index.as_ref(), &sources)
                };
                match grounding {
                    Ok(label) => {
                        let item_id =
                            format!("it_{}", ulid::Ulid::new().to_string().to_lowercase());
                        let difficulty = obj
                            .get("difficulty")
                            .and_then(Value::as_f64)
                            .unwrap_or(3.0)
                            .clamp(1.0, 5.0);
                        let sources: Vec<Value> = sources
                            .iter()
                            .map(|s| {
                                let mut s = s.clone();
                                if let Some(full) = s
                                    .get("file_hash")
                                    .and_then(Value::as_str)
                                    .and_then(|h| resolve_file_hash(&state, h).ok())
                                {
                                    s["file_hash"] = json!(full);
                                }
                                s
                            })
                            .collect();
                        let mut data = obj.clone();
                        data.remove("retry_of");
                        for (k, v) in [
                            ("item_id", json!(item_id)),
                            ("unit", json!(unit)),
                            ("mode", json!(mode)),
                            ("grounding", json!(label)),
                            ("item_key", json!(key)),
                            ("difficulty", json!(difficulty)),
                            ("sources", json!(sources)),
                            ("host", json!(ctx.host.client_name)),
                        ] {
                            data.insert(k.into(), v);
                        }
                        ctx.ws
                            .append("item.created", Value::Object(data))
                            .map_err(ws_err)?;
                        created.push(json!({"index": idx, "item_id": item_id, "grounding": label}));
                    }
                    Err(reason) => {
                        let prior = state
                            .get(col::REJECTIONS, &key)
                            .and_then(|r| r.get("count"))
                            .and_then(Value::as_u64)
                            .unwrap_or(0);
                        let last = prior >= 1;
                        ctx.ws
                            .append(
                                "item.rejected",
                                json!({"item_key": key, "unit": unit, "reason": reason, "final": last}),
                            )
                            .map_err(ws_err)?;
                        let entry = json!({"index": idx, "item_key": key, "reason": reason});
                        if last {
                            dropped.push(entry);
                        } else {
                            retry.push(entry);
                        }
                    }
                }
            }
            let mut warnings = Vec::new();
            if !retry.is_empty() {
                warnings.push(format!(
                    "{} item(s) failed the grounding check; regenerate each once with 'retry_of' set to its item_key",
                    retry.len()
                ));
            }
            if !dropped.is_empty() {
                warnings.push(format!(
                    "{} item(s) failed grounding twice and were dropped",
                    dropped.len()
                ));
            }
            let next = if retry.is_empty() {
                "practice"
            } else {
                "regenerate_items"
            };
            Ok(ToolOutcome {
                data: json!({
                    "unit": unit,
                    "mode": mode,
                    "created": created,
                    "retry_allowed": retry,
                    "dropped": dropped,
                }),
                next: Some(next.into()),
                warnings,
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: None,
            })
        }
        "grades" => {
            check_keys(
                args,
                &["action", "session_id", "grades", "progress"],
                "lucubra_submit(grades)",
            )?;
            let _sid = opt_str(args, "session_id")?;
            let grades = match args.get("grades").and_then(Value::as_array) {
                Some(g) if !g.is_empty() => g,
                Some(_) => {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        "grades must not be empty",
                    ));
                }
                None => {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        "grades must be a non-empty array of {item_id, criteria_scores, total, feedback?}",
                    ));
                }
            };
            let mut checked = Vec::new();
            for (idx, g) in grades.iter().enumerate() {
                checked.push(check_grade(idx, g, &state)?);
            }
            let today = crate::tools::practice::today_day();
            let mut stored = Vec::new();
            for (item_id, total, max_total) in checked {
                let ratio = if max_total > 0.0 {
                    total / max_total
                } else {
                    0.0
                };
                ctx.ws
                    .append(
                        "item.graded",
                        json!({"item_id": item_id, "score": ratio, "correct": ratio >= 0.5,
                               "total": total, "max_total": max_total, "graded_by": "host"}),
                    )
                    .map_err(ws_err)?;
                crate::tools::practice::review_card(
                    ctx,
                    &state,
                    &item_id,
                    ratio >= 0.5,
                    ratio,
                    None,
                    today,
                )?;
                stored.push(json!({"item_id": item_id, "score": ratio}));
            }
            Ok(ToolOutcome {
                data: json!({"stored": stored, "graded_by": "host"}),
                next: Some("practice".into()),
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: None,
            })
        }
        "tag" => {
            check_keys(
                args,
                &["action", "file_hash", "path", "assessment", "progress"],
                "lucubra_submit(tag)",
            )?;
            let full = match (opt_str(args, "file_hash")?, opt_str(args, "path")?) {
                (Some(h), _) => resolve_file_hash(&state, h)?,
                (None, Some(p)) => hash_for_rel_path(&state, p).ok_or_else(|| {
                    ToolError::new(ToolErrorCode::NotFound, format!("no scanned file at '{p}'"))
                })?,
                _ => {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        "provide 'file_hash' or 'path' and 'assessment' (true or false)",
                    ));
                }
            };
            let Some(on) = args.get("assessment").and_then(Value::as_bool) else {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "provide 'assessment' as true (tutor mode on) or false (off)",
                ));
            };
            ctx.ws
                .append(
                    "file.tagged",
                    json!({"hash": full, "assessment": on, "by": "user"}),
                )
                .map_err(ws_err)?;
            Ok(ToolOutcome {
                data: json!({"file_hash": full, "assessment": on, "tutor_mode": on}),
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
