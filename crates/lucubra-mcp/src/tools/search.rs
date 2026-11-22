// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_search`: FTS5 search with safe snippets wrapped in `MaterialPayload` (`S29`, `S37`, `S38`).

use lucubra_ingest::pipeline::open_index;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::budget::{BudgetLimits, estimate_tokens, max_payload_chars, truncate_chars};
use crate::envelope::{ProgressEntry, ToolError, ToolErrorCode};
use crate::material::MaterialPayload;
use crate::tools::{
    ToolContext, ToolOutcome, check_keys, check_slug, hash_to_path_map, opt_bool, opt_str, opt_u64,
    req_str,
};

/// JSON Schema arguments for `lucubra_search`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchArgs {
    /// Non-empty search query string.
    pub query: String,
    /// Optional subject slug filter.
    #[serde(default)]
    pub subject: Option<String>,
    /// Optional unit slug filter (requires subject).
    #[serde(default)]
    pub unit: Option<String>,
    /// FTS5 match mode: word (default), prefix, or substring.
    #[serde(default)]
    pub mode: Option<String>,
    /// Maximum hits to return (1-50, default 10).
    #[serde(default)]
    pub limit: Option<u64>,
    /// Hit offset for pagination (default 0).
    #[serde(default)]
    pub offset: Option<u64>,
    /// Hybrid semantic ranking flag (deferred to M8; uses FTS5 fallback in M3).
    #[serde(default)]
    pub vector: Option<bool>,
    /// Optional completed-step checkpoints to log alongside this call.
    #[serde(default)]
    pub progress: Option<Vec<ProgressEntry>>,
}

pub fn execute(
    args: &Map<String, Value>,
    ctx: &ToolContext<'_>,
    limits: BudgetLimits,
) -> std::result::Result<ToolOutcome, ToolError> {
    check_keys(
        args,
        &[
            "query", "subject", "unit", "mode", "limit", "offset", "vector", "progress",
        ],
        "lucubra_search",
    )?;

    let query = req_str(
        args,
        "query",
        "provide a non-empty 'query' string to search extracted text",
    )?;
    let subject = opt_str(args, "subject")?;
    if let Some(s) = subject {
        check_slug("subject", s)?;
    }
    let unit = opt_str(args, "unit")?;
    if let Some(u) = unit {
        if subject.is_none() {
            return Err(ToolError::new(
                ToolErrorCode::InvalidArguments,
                "provide 'subject' alongside 'unit' filter",
            ));
        }
        check_slug("unit", u)?;
    }

    let mode = opt_str(args, "mode")?.unwrap_or("word");
    if !matches!(mode, "word" | "prefix" | "substring") {
        return Err(ToolError::new(
            ToolErrorCode::InvalidArguments,
            "mode must be one of: word, prefix, substring",
        ));
    }

    let limit = opt_u64(
        args,
        "limit",
        1,
        50,
        "limit must be an integer between 1 and 50",
    )?
    .unwrap_or(10) as usize;
    let offset = opt_u64(
        args,
        "offset",
        0,
        10_000,
        "offset must be a non-negative integer",
    )?
    .unwrap_or(0) as usize;
    let vector = opt_bool(args, "vector", "vector must be true or false")?.unwrap_or(false);

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
    let path_map = hash_to_path_map(&state);

    let index = open_index(ctx.ws).map_err(|e| {
        ToolError::new(
            ToolErrorCode::WorkspaceError,
            format!("failed opening index: {e}"),
        )
    })?;

    // Pull a wider pool when filtering by subject/unit so paging still sees hits.
    let fetch_limit = if subject.is_some() {
        (offset + limit + 50).min(250)
    } else {
        offset + limit + 1
    };

    let raw_hits = match mode {
        "word" => index.search(query, fetch_limit),
        "prefix" => index.search_prefix(query, fetch_limit),
        "substring" => index.search_substring(query, fetch_limit),
        _ => unreachable!(),
    }
    .map_err(|e| ToolError::new(ToolErrorCode::WorkspaceError, format!("search failed: {e}")))?;

    let prefix_filter = match (subject, unit) {
        (Some(s), Some(u)) => Some(format!("subjects/{s}/{u}/")),
        (Some(s), None) => Some(format!("subjects/{s}/")),
        _ => None,
    };

    let filtered: Vec<_> = raw_hits
        .into_iter()
        .filter(|hit| {
            let Some(ref prefix) = prefix_filter else {
                return true;
            };
            path_map
                .get(&hit.hash)
                .is_some_and(|rel| rel.starts_with(prefix))
        })
        .collect();

    let total_after_offset = filtered.len().saturating_sub(offset);
    let page_hits = filtered
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    let has_more_filtered = total_after_offset > page_hits.len();

    let max_data_chars = max_payload_chars(limits.response_limit);
    let mut warnings = Vec::new();
    if vector {
        warnings.push(
            "Vector search (lucubra[vector]) is not enabled yet; returning FTS5 BM25 results."
                .to_owned(),
        );
    }

    // Fit hit count and snippet length to response budget cleanly (`S37`).
    let mut snippet_cap = 160usize;
    let mut keep_count = page_hits.len();
    let mut truncated_by_budget = false;

    let items = loop {
        let built: Vec<Value> = page_hits
            .iter()
            .take(keep_count)
            .map(|hit| {
                let (snippet, _) = truncate_chars(&hit.text, snippet_cap);
                let short_ref = format!("F:{}/{}", &hit.hash[..8.min(hit.hash.len())], hit.locator);
                let path = path_map.get(&hit.hash).cloned();
                let material = MaterialPayload::new(&snippet, Some(short_ref.clone()), path, None);
                json!({
                    "ref": short_ref,
                    "file_hash": hit.hash,
                    "locator": hit.locator,
                    "score": hit.score,
                    "material": material,
                })
            })
            .collect();
        let probe = json!({ "hits": built });
        let serialized = serde_json::to_string(&probe).unwrap_or_default();
        if serialized.chars().count() <= max_data_chars || (keep_count <= 1 && snippet_cap <= 80) {
            break built;
        }
        truncated_by_budget = true;
        if snippet_cap > 80 {
            snippet_cap = 80;
        } else if keep_count > 1 {
            keep_count -= 1;
        } else {
            break built;
        }
    };

    let truncated = has_more_filtered || truncated_by_budget;
    let next_offset = if truncated && !items.is_empty() {
        Some(offset + items.len())
    } else {
        None
    };

    let _ = estimate_tokens("");

    Ok(ToolOutcome {
        data: json!({
            "query": query,
            "mode": mode,
            "count": items.len(),
            "hits": items,
            "truncated": truncated,
            "next_offset": next_offset,
        }),
        next: Some(if items.is_empty() { "ready" } else { "read" }.into()),
        warnings,
        per_call_limit: limits.response_limit,
        suppress_wrap_up: false,
        active_session_override: None,
    })
}
