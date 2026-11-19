// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_ingest`: scan, extraction, vision batch listing, and deferred stubs.

use std::collections::BTreeMap;

use lucubra_core::state::{State, col};
use lucubra_ingest::doctor::check_tools;
use lucubra_ingest::pipeline::{Outcome, extract_pending};
use lucubra_ingest::scan;
use lucubra_ingest::vision::{pending_vision_pages, render_dir};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::budget::{BudgetLimits, resolve_tier};
use crate::envelope::{ProgressEntry, ToolError, ToolErrorCode};
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
    /// Move specifications for approve_layout (M4).
    #[serde(default)]
    pub moves: Option<Vec<Value>>,
    /// Session id filter for undo_moves (M4).
    #[serde(default)]
    pub session_id: Option<String>,
    /// Scope for undo_moves: last or all (M4).
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
                    "status": status_summary,
                }),
                next: Some(if extracted_now > 0 { "read" } else { "ready" }.into()),
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

            let summary = collect_status_details(ctx.ws, &pre_state, batch_size, cursor);
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
                &["action", "moves", "progress"],
                "lucubra_ingest(approve_layout)",
            )?;
            let Some(moves) = args.get("moves").and_then(Value::as_array) else {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "moves must be an array of {from, to, mode} objects",
                ));
            };
            for (idx, item) in moves.iter().enumerate() {
                let Some(obj) = item.as_object() else {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        format!("moves[{idx}] must be an object with {{from, to, mode?}}"),
                    ));
                };
                if obj.get("from").and_then(Value::as_str).is_none()
                    || obj.get("to").and_then(Value::as_str).is_none()
                {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        format!("moves[{idx}] requires string fields 'from' and 'to'"),
                    ));
                }
                if let Some(mode) = obj.get("mode").and_then(Value::as_str)
                    && !matches!(mode, "move" | "index")
                {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        format!("moves[{idx}].mode must be 'move' or 'index'"),
                    ));
                }
            }
            Err(ToolError::new(
                ToolErrorCode::NotImplemented,
                "Layout approval and file moves are implemented in milestone M4; keep files in inbox/ or subjects/ for now.",
            ))
        }
        "undo_moves" => {
            check_keys(
                args,
                &["action", "session_id", "scope", "progress"],
                "lucubra_ingest(undo_moves)",
            )?;
            let _sid = opt_str(args, "session_id")?;
            if let Some(scope) = opt_str(args, "scope")?
                && !matches!(scope, "last" | "all")
            {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "scope must be 'last' or 'all'",
                ));
            }
            Err(ToolError::new(
                ToolErrorCode::NotImplemented,
                "Layout approval and file moves are implemented in milestone M4; keep files in inbox/ or subjects/ for now.",
            ))
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
