// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_ingest`: scan, extraction, classification, vision batches, first-run
//! layout approval and move undo (SPEC sections 10.1, 10.3 and 10.4).

use std::collections::BTreeMap;

use lucubra_core::organise::{self, FolderMode, Move, UndoScope};
use lucubra_core::state::{State, col};
use lucubra_ingest::classify::{BatchItem, classify_pending};
use lucubra_ingest::doctor::check_tools;
use lucubra_ingest::pipeline::{Outcome, extract_pending};
use lucubra_ingest::scan;
use lucubra_ingest::vision::{pending_vision_pages, render_dir};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::budget::{BudgetLimits, resolve_tier};
use crate::envelope::{ProgressEntry, ToolError, ToolErrorCode};
use crate::material::MaterialPayload;
use crate::tools::{
    ToolContext, ToolOutcome, check_keys, opt_bool, opt_str, opt_u64, req_str, require_action,
};

/// JSON Schema arguments for `lucubra_ingest`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IngestArgs {
    /// Action: scan, status, approve_layout, undo_moves, or fetch.
    pub action: String,
    /// Run extraction after scanning (default: true).
    #[serde(default)]
    pub extract: Option<bool>,
    /// Reserved opt-in for online-only cloud placeholders (S20).
    #[serde(default)]
    pub approve_online_only: Option<bool>,
    /// Page batch limit for pending vision items (1-10).
    #[serde(default)]
    pub vision_batch_size: Option<u64>,
    /// Pagination cursor into pending vision files.
    #[serde(default)]
    pub vision_cursor: Option<u64>,
    /// Explicit moves for approve_layout: `{from, to}` file moves, or
    /// `{from: <top-level folder>, mode: "index"}` to keep a folder in place.
    #[serde(default)]
    pub moves: Option<Vec<Value>>,
    /// approve_layout: apply the proposed layout as shown (default false).
    #[serde(default)]
    pub accept_proposal: Option<bool>,
    /// approve_layout: per top-level folder mode overrides, `{"Folder": "move"|"index"}`.
    #[serde(default)]
    pub folders: Option<Map<String, Value>>,
    /// Session id filter for undo_moves.
    #[serde(default)]
    pub session_id: Option<String>,
    /// Scope for undo_moves: last (default) or all.
    #[serde(default)]
    pub scope: Option<String>,
    /// HTTPS URL from SOURCES.md for fetch (M6).
    #[serde(default)]
    pub url: Option<String>,
    /// Optional completed-step checkpoints to log alongside this call.
    #[serde(default)]
    pub progress: Option<Vec<ProgressEntry>>,
}

fn collect_status_details(
    ws: &lucubra_core::Workspace,
    state: &State,
    batch_size: usize,
    cursor: usize,
) -> Value {
    let mut path_for_hash: BTreeMap<&str, &str> = BTreeMap::new();
    for (rel, v) in state.collection(col::PATHS) {
        if let Some(h) = v.get("hash").and_then(Value::as_str) {
            path_for_hash.entry(h).or_insert(rel.as_str());
        }
    }
    let tools = check_tools();

    let mut extracted = 0usize;
    let mut pending = 0usize;
    let mut failed = 0usize;
    let mut online_only = 0usize;
    let mut issues = Vec::new();
    let mut all_vision = Vec::new();

    for (hash, file) in state.collection(col::FILES) {
        let status = file.get("status").and_then(Value::as_str).unwrap_or("");
        let path = path_for_hash.get(hash.as_str()).copied().unwrap_or("");
        match status {
            "extracted" => {
                extracted += 1;
                let pages = pending_vision_pages(state, hash);
                if !pages.is_empty() {
                    let rendered_pngs: Vec<String> = match render_dir(ws, hash) {
                        Ok(dir) => pages
                            .iter()
                            .filter_map(|p| {
                                let png = dir.join(format!("p{p}.png"));
                                png.is_file().then(|| png.to_string_lossy().into_owned())
                            })
                            .collect(),
                        Err(_) => Vec::new(),
                    };
                    all_vision.push(json!({
                        "file_hash": hash,
                        "path": path,
                        "pages": pages,
                        "rendered_pngs": rendered_pngs,
                    }));
                }
            }
            "pending" => {
                pending += 1;
                let code = file.get("code").and_then(Value::as_str).unwrap_or("");
                if code == "online_only" {
                    online_only += 1;
                }
                let tool = file.get("tool").and_then(Value::as_str);
                let install = tool.and_then(|t| match t {
                    "oda" => Some(tools.oda.install_steps),
                    "blender" => Some(tools.blender.install_steps),
                    _ => None,
                });
                if issues.len() < 20 {
                    issues.push(json!({
                        "file_hash": hash,
                        "path": path,
                        "status": status,
                        "code": code,
                        "tool": tool,
                        "install": install,
                    }));
                }
            }
            "failed" => {
                failed += 1;
                if issues.len() < 20 {
                    issues.push(json!({
                        "file_hash": hash,
                        "path": path,
                        "status": status,
                        "code": file.get("code").and_then(Value::as_str),
                        "detail": file.get("detail").and_then(Value::as_str),
                    }));
                }
            }
            _ => {}
        }
    }

    let total_vision_files = all_vision.len();
    let end = (cursor + batch_size).min(total_vision_files);
    let batch: Vec<Value> = if cursor < total_vision_files {
        all_vision[cursor..end].to_vec()
    } else {
        Vec::new()
    };
    let next_cursor = (end < total_vision_files).then_some(end);

    json!({
        "counts": {
            "extracted": extracted,
            "pending": pending,
            "failed": failed,
            "online_only": online_only,
            "needs_vision_files": total_vision_files,
        },
        "issues": issues,
        "needs_vision_batch": batch,
        "vision_next_cursor": next_cursor,
        "vision_remaining_files": total_vision_files.saturating_sub(end),
    })
}

pub fn execute(
    args: &Map<String, Value>,
    mut ctx: ToolContext<'_>,
    limits: BudgetLimits,
) -> std::result::Result<ToolOutcome, ToolError> {
    let action = require_action(
        args,
        &["scan", "status", "approve_layout", "undo_moves", "fetch"],
    )?;

    let pre_state = ctx
        .ws
        .replay()
        .map_err(|e| {
            ToolError::new(
                ToolErrorCode::WorkspaceError,
                format!("failed to replay workspace state: {e}"),
            )
        })?
        .state;
    let tier = resolve_tier(
        pre_state.collections.get(col::PROFILE),
        ctx.host.profile.id,
        ctx.host.profile.default_tier,
    );
    let default_batch = if tier == crate::budget::BudgetTier::Local {
        1
    } else {
        3
    };

    match action {
        "scan" => {
            check_keys(
                args,
                &[
                    "action",
                    "extract",
                    "approve_online_only",
                    "vision_batch_size",
                    "vision_cursor",
                    "progress",
                ],
                "lucubra_ingest(scan)",
            )?;
            let run_extract =
                opt_bool(args, "extract", "extract must be true or false")?.unwrap_or(true);
            let _approve_online = opt_bool(
                args,
                "approve_online_only",
                "approve_online_only must be true or false",
            )?;
            let batch_size = opt_u64(
                args,
                "vision_batch_size",
                1,
                10,
                "vision_batch_size must be an integer between 1 and 10",
            )?
            .unwrap_or(default_batch) as usize;
            let cursor = opt_u64(
                args,
                "vision_cursor",
                0,
                100_000,
                "vision_cursor must be a non-negative integer",
            )?
            .unwrap_or(0) as usize;

            let scanned = scan(ctx.ws).map_err(|e| {
                ToolError::new(ToolErrorCode::WorkspaceError, format!("scan failed: {e}"))
            })?;

            let mut warnings = scanned.problems.clone();
            let mut extracted_now = 0usize;
            let mut failed_now = 0usize;
            let mut waiting_worker = 0usize;
            let worker_available = ctx.supervisor.is_some();

            if run_extract {
                let sup = ctx.supervisor.take();
                let report = extract_pending(ctx.ws, sup).map_err(|e| {
                    ToolError::new(
                        ToolErrorCode::WorkspaceError,
                        format!("extract_pending failed: {e}"),
                    )
                })?;
                for item in &report.processed {
                    match item.outcome {
                        Outcome::Extracted { .. } => extracted_now += 1,
                        Outcome::Failed { .. } => failed_now += 1,
                        Outcome::NoWorker => waiting_worker += 1,
                    }
                }
                if waiting_worker > 0 && !worker_available {
                    warnings.push(format!(
                        "{waiting_worker} queued file(s) require Python workers (check `lucubra doctor`)."
                    ));
                }
            }

            let classified = classify_pending(ctx.ws, true).map_err(ws_error)?;
            let filed = file_inbox(&ctx)?;
            let state = ctx
                .ws
                .replay()
                .map_err(|e| {
                    ToolError::new(
                        ToolErrorCode::WorkspaceError,
                        format!("failed to reload workspace state: {e}"),
                    )
                })?
                .state;
            let status_summary = collect_status_details(ctx.ws, &state, batch_size, cursor);
            let next = if !classified.batch.is_empty() {
                "classify"
            } else if !layout_approved(&state) && !classified.engine.is_empty() {
                "approve_layout"
            } else if extracted_now > 0 {
                "read"
            } else {
                "ready"
            };

            Ok(ToolOutcome {
                data: json!({
                    "scan": {
                        "new_or_changed": scanned.logged(),
                        "unchanged": scanned.unchanged,
                        "online_only_count": scanned.online_only.len(),
                        "online_only_bytes": scanned.online_only_bytes(),
                    },
                    "extract": {
                        "ran": run_extract,
                        "worker_available": worker_available,
                        "extracted_now": extracted_now,
                        "failed_or_pending_now": failed_now,
                        "waiting_for_worker": waiting_worker,
                    },
                    "classify": {
                        "by_engine": classified.engine.len(),
                        "needs_host": classified.batch.len(),
                    },
                    "inbox_filed": filed,
                    "status": status_summary,
                }),
                next: Some(next.into()),
                warnings,
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: None,
            })
        }
        "status" => {
            check_keys(
                args,
                &["action", "vision_batch_size", "vision_cursor", "progress"],
                "lucubra_ingest(status)",
            )?;
            let batch_size = opt_u64(
                args,
                "vision_batch_size",
                1,
                10,
                "vision_batch_size must be an integer between 1 and 10",
            )?
            .unwrap_or(default_batch) as usize;
            let cursor = opt_u64(
                args,
                "vision_cursor",
                0,
                100_000,
                "vision_cursor must be a non-negative integer",
            )?
            .unwrap_or(0) as usize;

            let mut summary = collect_status_details(ctx.ws, &pre_state, batch_size, cursor);
            let batch = classify_pending(ctx.ws, false).map_err(ws_error)?.batch;
            summary["classification_batch"] = classification_batch(&batch);
            Ok(ToolOutcome {
                data: summary,
                next: None,
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: None,
            })
        }
        "approve_layout" => {
            check_keys(
                args,
                &["action", "moves", "accept_proposal", "folders", "progress"],
                "lucubra_ingest(approve_layout)",
            )?;
            let mut overrides = BTreeMap::new();
            if let Some(folders) = args.get("folders") {
                let Some(map) = folders.as_object() else {
                    return Err(invalid(
                        "folders must be an object of folder name to 'move' or 'index'",
                    ));
                };
                for (name, mode) in map {
                    let mode = mode.as_str().and_then(FolderMode::parse).ok_or_else(|| {
                        invalid(format!("folders.{name} must be 'move' or 'index'"))
                    })?;
                    overrides.insert(name.clone(), mode);
                }
            }
            let mut explicit = Vec::new();
            if let Some(moves) = args.get("moves") {
                let Some(moves) = moves.as_array() else {
                    return Err(invalid(
                        "moves must be an array of {from, to} or {from, mode: \"index\"} objects",
                    ));
                };
                for (idx, item) in moves.iter().enumerate() {
                    let obj = item.as_object().ok_or_else(|| {
                        invalid(format!("moves[{idx}] must be an object with {{from, to}}"))
                    })?;
                    let from = obj.get("from").and_then(Value::as_str).ok_or_else(|| {
                        invalid(format!("moves[{idx}] requires string field 'from'"))
                    })?;
                    match obj.get("mode").and_then(Value::as_str) {
                        Some("index") => {
                            overrides.insert(from.to_owned(), FolderMode::Index);
                        }
                        None | Some("move") => {
                            let to = obj.get("to").and_then(Value::as_str).ok_or_else(|| {
                                invalid(format!("moves[{idx}] requires string field 'to'"))
                            })?;
                            explicit.push(Move {
                                from: from.to_owned(),
                                to: to.to_owned(),
                            });
                        }
                        Some(_) => {
                            return Err(invalid(format!(
                                "moves[{idx}].mode must be 'move' or 'index'"
                            )));
                        }
                    }
                }
            }
            let accept = opt_bool(
                args,
                "accept_proposal",
                "accept_proposal must be true or false",
            )?
            .unwrap_or(false);
            let proposal = organise::propose(&pre_state, &overrides);
            if explicit.is_empty() && !accept {
                return Ok(ToolOutcome {
                    data: proposal_summary(&proposal),
                    next: Some("approve_layout".into()),
                    warnings: vec![
                        "Nothing has moved. Show this tree to the user and call approve_layout with accept_proposal=true (and any folder overrides) only after they approve.".into(),
                    ],
                    per_call_limit: limits.response_limit,
                    suppress_wrap_up: false,
                    active_session_override: None,
                });
            }
            let moves = if explicit.is_empty() {
                proposal.moves.clone()
            } else {
                explicit
            };
            let report =
                organise::apply(ctx.ws, &moves, ctx.active_session_id).map_err(ws_error)?;
            let folders: Map<String, Value> = proposal
                .folders
                .iter()
                .map(|(k, v)| (k.clone(), Value::String(v.as_str().into())))
                .collect();
            ctx.ws
                .append(
                    "setting.changed",
                    json!({"key": "layout", "value": {"approved": true, "folders": folders}}),
                )
                .map_err(ws_error)?;
            ctx.ws.project().map_err(ws_error)?;
            Ok(ToolOutcome {
                data: move_summary(&report),
                next: Some("ready".into()),
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: None,
            })
        }
        "undo_moves" => {
            check_keys(
                args,
                &["action", "session_id", "scope", "progress"],
                "lucubra_ingest(undo_moves)",
            )?;
            let sid = opt_str(args, "session_id")?;
            let scope = match opt_str(args, "scope")? {
                None | Some("last") => UndoScope::Last,
                Some("all") => UndoScope::All,
                Some(_) => return Err(invalid("scope must be 'last' or 'all'")),
            };
            let report = organise::undo(ctx.ws, scope, sid).map_err(ws_error)?;
            ctx.ws.project().map_err(ws_error)?;
            Ok(ToolOutcome {
                data: move_summary(&report),
                next: Some("ready".into()),
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: None,
            })
        }
        "fetch" => {
            check_keys(
                args,
                &["action", "url", "progress"],
                "lucubra_ingest(fetch)",
            )?;
            let url = req_str(
                args,
                "url",
                "provide 'url' (https://...) from a domain listed in SOURCES.md",
            )?;
            if !url.starts_with("https://") && !url.starts_with("http://") {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "url must start with https:// or http://",
                ));
            }
            Err(ToolError::new(
                ToolErrorCode::NotImplemented,
                "Domain-restricted download (lucubra_ingest fetch) is implemented in milestone M6; save the downloaded file into inbox/ manually.",
            ))
        }
        _ => unreachable!(),
    }
}

fn invalid(msg: impl Into<String>) -> ToolError {
    ToolError::new(ToolErrorCode::InvalidArguments, msg)
}

fn ws_error(e: lucubra_core::Error) -> ToolError {
    ToolError::new(
        ToolErrorCode::WorkspaceError,
        format!("workspace update failed: {e}"),
    )
}

fn layout_approved(state: &State) -> bool {
    state
        .get(col::SETTINGS, "layout")
        .and_then(|l| l.get("approved"))
        .and_then(Value::as_bool)
        == Some(true)
}

/// After first-run approval, confidently classified files in `inbox/` are
/// filed automatically (SPEC section 10.4).
fn file_inbox(ctx: &ToolContext<'_>) -> std::result::Result<usize, ToolError> {
    let state = ctx.ws.replay().map_err(ws_error)?.state;
    if !layout_approved(&state) {
        return Ok(0);
    }
    let moves: Vec<Move> = organise::propose(&state, &BTreeMap::new())
        .moves
        .into_iter()
        .filter(|m| m.from.starts_with("inbox/"))
        .collect();
    if moves.is_empty() {
        return Ok(0);
    }
    let report = organise::apply(ctx.ws, &moves, ctx.active_session_id).map_err(ws_error)?;
    ctx.ws.project().map_err(ws_error)?;
    Ok(report.moved.len())
}

const LIST_CAP: usize = 15;

fn moves_json<'a>(moves: impl Iterator<Item = &'a Move>) -> Vec<Value> {
    moves
        .take(LIST_CAP)
        .map(|m| json!({"from": m.from, "to": m.to}))
        .collect()
}

fn proposal_summary(p: &organise::Proposal) -> Value {
    let mut tree: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for m in &p.moves {
        let mut parts = m.to.split('/').skip(1);
        let subject = parts.next().unwrap_or_default();
        let unit = parts.next().unwrap_or_default();
        *tree
            .entry(subject.into())
            .or_default()
            .entry(unit.into())
            .or_default() += 1;
    }
    let folders: Vec<Value> = p
        .folders
        .iter()
        .map(|(name, mode)| {
            let shown = if name.is_empty() { "(root)" } else { name };
            json!({"folder": shown, "mode": mode.as_str()})
        })
        .collect();
    json!({
        "approved": false,
        "folders": folders,
        "tree": tree,
        "moves_total": p.moves.len(),
        "sample_moves": moves_json(p.moves.iter()),
        "unclassified_total": p.unclassified.len(),
        "unclassified_sample": p.unclassified.iter().take(LIST_CAP).collect::<Vec<_>>(),
    })
}

fn move_summary(r: &organise::MoveReport) -> Value {
    let refused: Vec<Value> = r
        .refused
        .iter()
        .map(|(path, why)| json!({"path": path, "reason": why}))
        .collect();
    json!({
        "batch": r.batch,
        "moved_total": r.moved.len(),
        "moved_sample": moves_json(r.moved.iter()),
        "refused": refused,
    })
}

/// Host classification batch. The first characters of each file are study
/// material, so they travel inside an untrusted [`MaterialPayload`].
fn classification_batch(batch: &[BatchItem]) -> Value {
    let items: Vec<Value> = batch
        .iter()
        .take(25)
        .map(|item| {
            let candidates: Vec<Value> = item
                .candidates
                .iter()
                .map(|c| json!({"subject": c.subject, "score": c.score}))
                .collect();
            json!({
                "path": item.path,
                "file_hash": item.hash,
                "head": MaterialPayload::new(&item.head, None, Some(item.path.clone()), None),
                "candidates": candidates,
            })
        })
        .collect();
    json!({
        "total": batch.len(),
        "items": items,
        "how": "Classify these in one lucubra_submit(action=\"classification\") call; ask the user when unsure.",
    })
}
