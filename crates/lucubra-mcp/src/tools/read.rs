// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_read`: progressive index, subject, unit, chunk, and transcript reads (`S29`, `S37`).

use std::fs;

use lucubra_core::state::{State, col};
use lucubra_ingest::pipeline::open_index;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::budget::{BudgetLimits, max_payload_chars, truncate_chars};
use crate::envelope::{ProgressEntry, ToolError, ToolErrorCode};
use crate::material::{MaterialPayload, resolve_workspace_path};
use crate::tools::{
    ToolContext, ToolOutcome, check_keys, check_slug, hash_to_path_map, opt_str, opt_u64,
    require_action, resolve_file_hash,
};

/// JSON Schema arguments for `lucubra_read`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadArgs {
    /// Action: index, subject, unit, chunk, or transcript.
    pub action: String,
    /// Subject slug (for subject, unit).
    #[serde(default)]
    pub subject: Option<String>,
    /// Unit slug (for unit).
    #[serde(default)]
    pub unit: Option<String>,
    /// Chunk reference e.g. "F:<hash>/p:12" or "S:<s>/U:<u>/F:<hash>/p:12".
    #[serde(default, rename = "ref")]
    pub ref_id: Option<String>,
    /// BLAKE3 hash or prefix of the source file (alternative to ref).
    #[serde(default)]
    pub file_hash: Option<String>,
    /// Chunk locator inside file_hash (e.g., p:12, h:1, x:Sheet1!summary).
    #[serde(default)]
    pub locator: Option<String>,
    /// Workspace-relative transcript path (for transcript).
    #[serde(default)]
    pub path: Option<String>,
    /// Optional 1-based page filter for transcript lookup.
    #[serde(default)]
    pub page: Option<u64>,
    /// Character offset for chunk/transcript pagination (default: 0).
    #[serde(default)]
    pub offset: Option<u64>,
    /// Maximum characters to return (capped by tier response budget).
    #[serde(default)]
    pub limit: Option<u64>,
    /// Optional completed-step checkpoints to log alongside this call.
    #[serde(default)]
    pub progress: Option<Vec<ProgressEntry>>,
}

fn parse_chunk_ref(ref_str: &str) -> std::result::Result<(String, String), ToolError> {
    let parts: Vec<&str> = ref_str.trim().split('/').collect();
    for (idx, seg) in parts.iter().enumerate() {
        if let Some(hash_prefix) = seg.strip_prefix("F:")
            && idx + 1 < parts.len()
        {
            let locator = parts[idx + 1..].join("/");
            if !hash_prefix.is_empty() && !locator.is_empty() {
                return Ok((hash_prefix.to_owned(), locator));
            }
        }
    }
    Err(ToolError::new(
        ToolErrorCode::InvalidArguments,
        "provide 'ref' (e.g. \"F:9a1c22e0/p:12\") or both 'file_hash' and 'locator' (call lucubra_search first to find refs)",
    ))
}

fn slice_chars(text: &str, offset: usize, max_chars: usize) -> (String, bool, Option<usize>) {
    let total = text.chars().count();
    if offset >= total {
        return (String::new(), false, None);
    }
    let tail: String = text.chars().skip(offset).collect();
    let (piece, cut) = truncate_chars(&tail, max_chars);
    let next_offset = cut.then(|| offset + piece.chars().count());
    (piece, cut, next_offset)
}

fn read_confined_text(root: &std::path::Path, rel: &str) -> std::result::Result<String, ToolError> {
    let (_, abs) = resolve_workspace_path(root, rel)
        .map_err(|msg| ToolError::new(ToolErrorCode::InvalidPath, msg))?;
    let meta = fs::symlink_metadata(&abs).map_err(|_| {
        ToolError::new(
            ToolErrorCode::NotFound,
            format!("file '{rel}' does not exist in workspace"),
        )
    })?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(ToolError::new(
            ToolErrorCode::InvalidPath,
            format!("'{rel}' is not a regular file inside the workspace"),
        ));
    }
    fs::read_to_string(&abs).map_err(|e| {
        ToolError::new(
            ToolErrorCode::WorkspaceError,
            format!("failed reading '{rel}': {e}"),
        )
    })
}

fn transcript_lookup_from_state(
    state: &State,
    full_hash: &str,
    page: Option<u64>,
) -> Option<(String, Option<bool>)> {
    let key = match page {
        Some(p) => format!("{full_hash}/p:{p}"),
        None => {
            if state
                .get(col::TRANSCRIPTS, &format!("{full_hash}/img"))
                .is_some()
            {
                format!("{full_hash}/img")
            } else {
                let prefix = format!("{full_hash}/");
                state
                    .collection(col::TRANSCRIPTS)
                    .find(|(k, _)| k.starts_with(&prefix))
                    .map(|(k, _)| k.clone())?
            }
        }
    };
    let entry = state.get(col::TRANSCRIPTS, &key)?;
    let path = entry.get("path").and_then(Value::as_str)?.to_owned();
    let verified = entry.get("verified").and_then(Value::as_bool);
    Some((path, verified))
}

fn parse_frontmatter_verified(raw: &str) -> Option<bool> {
    let rest = raw.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    for line in rest[..end].lines() {
        if let Some(val) = line.strip_prefix("verified:") {
            return val.trim().parse::<bool>().ok();
        }
    }
    None
}

pub fn execute(
    args: &Map<String, Value>,
    ctx: &ToolContext<'_>,
    limits: BudgetLimits,
) -> std::result::Result<ToolOutcome, ToolError> {
    let action = require_action(args, &["index", "subject", "unit", "chunk", "transcript"])?;
    let max_chars = max_payload_chars(limits.response_limit);

    let state = ctx
        .ws
        .replay()
        .map_err(|e| {
            ToolError::new(
                ToolErrorCode::WorkspaceError,
                format!("failed to replay workspace state: {e}"),
            )
        })?
        .state;

    match action {
        "index" => {
            check_keys(
                args,
                &["action", "offset", "limit", "progress"],
                "lucubra_read(index)",
            )?;
            let offset = opt_u64(args, "offset", 0, 10_000_000, "offset must be >= 0")?.unwrap_or(0)
                as usize;
            let limit = opt_u64(args, "limit", 1, max_chars as u64, "limit must be positive")?
                .map(|n| n as usize)
                .unwrap_or(max_chars)
                .min(max_chars);
            let raw = read_confined_text(ctx.ws.root(), "AGENTS.md")?;
            let (slice, truncated, next_offset) = slice_chars(&raw, offset, limit);
            Ok(ToolOutcome {
                data: json!({
                    "path": "AGENTS.md",
                    "content": slice,
                    "truncated": truncated,
                    "next_offset": next_offset,
                }),
                next: None,
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: None,
            })
        }
        "subject" => {
            check_keys(
                args,
                &["action", "subject", "offset", "limit", "progress"],
                "lucubra_read(subject)",
            )?;
            let Some(subject) = opt_str(args, "subject")? else {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "provide 'subject' slug (call lucubra_read(action=\"index\") to list valid subjects)",
                ));
            };
            check_slug("subject", subject)?;
            if state.get(col::SUBJECTS, subject).is_none() {
                return Err(ToolError::new(
                    ToolErrorCode::NotFound,
                    format!("subject '{subject}' not found; create or onboard it first"),
                ));
            }
            let rel = format!("subjects/{subject}/agents_{subject}.md");
            let offset = opt_u64(args, "offset", 0, 10_000_000, "offset must be >= 0")?.unwrap_or(0)
                as usize;
            let limit = opt_u64(args, "limit", 1, max_chars as u64, "limit must be positive")?
                .map(|n| n as usize)
                .unwrap_or(max_chars)
                .min(max_chars);
            let raw = match read_confined_text(ctx.ws.root(), &rel) {
                Ok(t) => t,
                Err(_) => serde_json::to_string_pretty(state.get(col::SUBJECTS, subject).unwrap())
                    .unwrap_or_default(),
            };
            let (slice, truncated, next_offset) = slice_chars(&raw, offset, limit);
            Ok(ToolOutcome {
                data: json!({
                    "subject": subject,
                    "path": rel,
                    "content": slice,
                    "truncated": truncated,
                    "next_offset": next_offset,
                }),
                next: None,
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: None,
            })
        }
        "unit" => {
            check_keys(
                args,
                &["action", "subject", "unit", "offset", "limit", "progress"],
                "lucubra_read(unit)",
            )?;
            let Some(subject) = opt_str(args, "subject")? else {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "provide both 'subject' and 'unit' slugs, e.g. subject=\"stats\", unit=\"u1\"",
                ));
            };
            let Some(unit) = opt_str(args, "unit")? else {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "provide both 'subject' and 'unit' slugs, e.g. subject=\"stats\", unit=\"u1\"",
                ));
            };
            check_slug("subject", subject)?;
            check_slug("unit", unit)?;
            let key = format!("{subject}/{unit}");
            let Some(unit_state) = state.get(col::UNITS, &key) else {
                return Err(ToolError::new(
                    ToolErrorCode::NotFound,
                    format!("unit '{key}' not found"),
                ));
            };
            let rel = format!("subjects/{subject}/{unit}/UNIT.md");
            let offset = opt_u64(args, "offset", 0, 10_000_000, "offset must be >= 0")?.unwrap_or(0)
                as usize;
            let limit = opt_u64(args, "limit", 1, max_chars as u64, "limit must be positive")?
                .map(|n| n as usize)
                .unwrap_or(max_chars)
                .min(max_chars);
            let raw = match read_confined_text(ctx.ws.root(), &rel) {
                Ok(t) => t,
                Err(_) => serde_json::to_string_pretty(unit_state).unwrap_or_default(),
            };
            let (slice, truncated, next_offset) = slice_chars(&raw, offset, limit);
            Ok(ToolOutcome {
                data: json!({
                    "subject": subject,
                    "unit": unit,
                    "path": rel,
                    "content": slice,
                    "truncated": truncated,
                    "next_offset": next_offset,
                }),
                next: None,
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: None,
            })
        }
        "chunk" => {
            check_keys(
                args,
                &[
                    "action",
                    "ref",
                    "file_hash",
                    "locator",
                    "offset",
                    "limit",
                    "progress",
                ],
                "lucubra_read(chunk)",
            )?;
            let offset = opt_u64(args, "offset", 0, 10_000_000, "offset must be >= 0")?.unwrap_or(0)
                as usize;
            let limit = opt_u64(args, "limit", 1, max_chars as u64, "limit must be positive")?
                .map(|n| n as usize)
                .unwrap_or(max_chars)
                .min(max_chars);

            let (hash_input, locator) = if let Some(ref_val) = opt_str(args, "ref")? {
                parse_chunk_ref(ref_val)?
            } else {
                let Some(h) = opt_str(args, "file_hash")? else {
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        "provide 'ref' (e.g. \"F:9a1c22e0/p:12\") or both 'file_hash' and 'locator' (call lucubra_search first to find refs)",
                    ));
                };
                let Some(loc) = opt_str(args, "locator")? else {
                    let full_hash = resolve_file_hash(&state, h)?;
                    let index = open_index(ctx.ws).map_err(|e| {
                        ToolError::new(ToolErrorCode::WorkspaceError, format!("open index: {e}"))
                    })?;
                    let avail = index.locators_for_hash(&full_hash, 12).unwrap_or_default();
                    return Err(ToolError::new(
                        ToolErrorCode::InvalidArguments,
                        format!(
                            "provide 'locator' alongside 'file_hash'; available locators start with: {}",
                            avail.join(", ")
                        ),
                    ));
                };
                (h.to_owned(), loc.to_owned())
            };

            let full_hash = resolve_file_hash(&state, &hash_input)?;
            let index = open_index(ctx.ws).map_err(|e| {
                ToolError::new(ToolErrorCode::WorkspaceError, format!("open index: {e}"))
            })?;
            let Some(raw_text) = index.chunk(&full_hash, &locator).map_err(|e| {
                ToolError::new(ToolErrorCode::WorkspaceError, format!("chunk lookup: {e}"))
            })?
            else {
                let avail = index.locators_for_hash(&full_hash, 10).unwrap_or_default();
                let hint = if avail.is_empty() {
                    "no chunks stored for this file".to_owned()
                } else {
                    format!("available locators start with: {}", avail.join(", "))
                };
                return Err(ToolError::new(
                    ToolErrorCode::NotFound,
                    format!("locator '{locator}' not found; {hint}"),
                ));
            };

            let (slice, truncated, next_offset) = slice_chars(&raw_text, offset, limit);
            let paths = hash_to_path_map(&state);
            let path = paths.get(&full_hash).cloned();
            let short_ref = format!("F:{}/{}", &full_hash[..8.min(full_hash.len())], locator);
            let material = MaterialPayload::new(&slice, Some(short_ref), path, None);
            Ok(ToolOutcome {
                data: json!({
                    "file_hash": full_hash,
                    "locator": locator,
                    "material": material,
                    "truncated": truncated,
                    "next_offset": next_offset,
                }),
                next: None,
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
                    "path",
                    "file_hash",
                    "page",
                    "offset",
                    "limit",
                    "progress",
                ],
                "lucubra_read(transcript)",
            )?;
            let offset = opt_u64(args, "offset", 0, 10_000_000, "offset must be >= 0")?.unwrap_or(0)
                as usize;
            let limit = opt_u64(args, "limit", 1, max_chars as u64, "limit must be positive")?
                .map(|n| n as usize)
                .unwrap_or(max_chars)
                .min(max_chars);
            let page = opt_u64(args, "page", 1, 100_000, "page must be an integer >= 1")?;

            let (rel, state_verified) = if let Some(p) = opt_str(args, "path")? {
                let (norm, _) = resolve_workspace_path(ctx.ws.root(), p)
                    .map_err(|msg| ToolError::new(ToolErrorCode::InvalidPath, msg))?;
                (norm, None)
            } else if let Some(h) = opt_str(args, "file_hash")? {
                let full_hash = resolve_file_hash(&state, h)?;
                transcript_lookup_from_state(&state, &full_hash, page).ok_or_else(|| {
                    ToolError::new(
                        ToolErrorCode::NotFound,
                        format!("no transcript recorded for file_hash '{full_hash}'"),
                    )
                })?
            } else {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "provide 'path' or 'file_hash' for lucubra_read(action=\"transcript\")",
                ));
            };

            let raw = read_confined_text(ctx.ws.root(), &rel)?;
            let verified = state_verified
                .or_else(|| parse_frontmatter_verified(&raw))
                .or(Some(false));
            let (slice, truncated, next_offset) = slice_chars(&raw, offset, limit);
            let material = MaterialPayload::new(&slice, None, Some(rel.clone()), verified);
            Ok(ToolOutcome {
                data: json!({
                    "path": rel,
                    "verified": verified.unwrap_or(false),
                    "material": material,
                    "truncated": truncated,
                    "next_offset": next_offset,
                }),
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
