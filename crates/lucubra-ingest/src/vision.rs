// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Vision page rendering and unverified transcript storage (SPEC 10.1, S04, S05).

use std::fs;
use std::path::{Path, PathBuf};

use lucubra_core::projection::{rel_to_path, write_atomic};
use lucubra_core::state::{State, col};
use lucubra_core::{Error, Result, Workspace};
use serde_json::{Value, json};

use crate::limits::RENDER_LONG_EDGE;
use crate::pipeline::open_index;
use crate::route::{Format, route_file};
use crate::worker::{Reply, Supervisor};

/// Summary of one vision rendering pass.
#[derive(Debug, Default)]
pub struct RenderReport {
    /// `(hash, page, png_path)` rendered in this pass.
    pub rendered: Vec<(String, u32, PathBuf)>,
    /// Pages skipped because a transcript or cached PNG already existed.
    pub skipped: usize,
    /// Non-fatal warnings from rendering.
    pub warnings: Vec<String>,
}

/// Directory inside app-data where rendered page PNGs live for `hash`.
pub fn render_dir(ws: &Workspace, hash: &str) -> Result<PathBuf> {
    Ok(ws.cache_dir()?.join("renders").join(hash))
}

/// Workspace-relative transcript path for a source file and optional page.
///
/// PDF/document pages save to `notes/<file>.p<n>.transcript.md` (or inside the
/// unit's `notes/` sibling of `sources/`). Image files without a page save next
/// to the photo as `<photo>.transcript.md`.
pub fn transcript_rel_path(source_rel: &str, page: Option<u32>) -> String {
    match page {
        Some(n) => {
            let file_name = Path::new(source_rel)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("source");
            if let Some((unit_prefix, tail)) = source_rel.split_once("/sources/") {
                format!("{unit_prefix}/notes/{tail}.p{n}.transcript.md")
            } else if let Some(tail) = source_rel.strip_prefix("sources/") {
                format!("notes/{tail}.p{n}.transcript.md")
            } else {
                format!("notes/{file_name}.p{n}.transcript.md")
            }
        }
        None => format!("{source_rel}.transcript.md"),
    }
}

/// Pages still requiring vision transcription for `hash` (excluding pages
/// already covered by `transcript.saved`).
pub fn pending_vision_pages(state: &State, hash: &str) -> Vec<u32> {
    let Some(file) = state.get(col::FILES, hash) else {
        return Vec::new();
    };
    if file.get("status").and_then(Value::as_str) != Some("extracted") {
        return Vec::new();
    }
    let format = file.get("format").and_then(Value::as_str).unwrap_or("");
    if format == "image"
        && state
            .get(col::TRANSCRIPTS, &format!("{hash}/img"))
            .is_some()
    {
        return Vec::new();
    }
    file.get("needs_vision")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_u64().and_then(|n| u32::try_from(n).ok()))
                .filter(|p| {
                    state
                        .get(col::TRANSCRIPTS, &format!("{hash}/p:{p}"))
                        .is_none()
                })
                .collect()
        })
        .unwrap_or_default()
}

fn resolve_hash(state: &State, hash_or_prefix: &str) -> Result<String> {
    if state.get(col::FILES, hash_or_prefix).is_some() {
        return Ok(hash_or_prefix.to_owned());
    }
    let matches: Vec<String> = state
        .collection(col::FILES)
        .map(|(k, _)| k.clone())
        .filter(|k| k.starts_with(hash_or_prefix))
        .collect();
    match matches.as_slice() {
        [only] => Ok(only.clone()),
        [] => Err(Error::Invalid(format!(
            "no file in workspace matches hash '{hash_or_prefix}'"
        ))),
        _ => Err(Error::Invalid(format!(
            "hash prefix '{hash_or_prefix}' matches multiple files"
        ))),
    }
}

fn source_path_for(state: &State, hash: &str) -> Option<String> {
    state
        .collection(col::PATHS)
        .find(|(_, entry)| entry.get("hash").and_then(Value::as_str) == Some(hash))
        .map(|(rel, _)| rel.clone())
}

/// Saves a vision or handwriting transcript to the workspace, records
/// `transcript.saved` (and `transcript.verified` when `verified` is true), and
/// indexes the transcript text in FTS5.
pub fn save_transcript(
    ws: &Workspace,
    hash_or_prefix: &str,
    page: Option<u32>,
    text: &str,
    verified: bool,
) -> Result<PathBuf> {
    let state = ws.replay()?.state;
    let hash = resolve_hash(&state, hash_or_prefix)?;
    let source_rel = source_path_for(&state, &hash).unwrap_or_else(|| format!("F:{}", &hash[..8]));
    let rel = transcript_rel_path(&source_rel, page);
    let abs = rel_to_path(ws.root(), &rel);

    let mut content = String::from("---\n");
    content.push_str(&format!("source_hash: {hash}\n"));
    content.push_str(&format!("source_path: {source_rel}\n"));
    if let Some(p) = page {
        content.push_str(&format!("page: {p}\n"));
    }
    content.push_str(&format!("verified: {verified}\n---\n\n"));
    content.push_str(text.trim_end());
    content.push('\n');

    write_atomic(&abs, content.as_bytes())?;

    let mut payload = json!({
        "hash": hash,
        "path": rel,
        "verified": verified,
    });
    if let Some(p) = page {
        payload["page"] = json!(p);
    }
    ws.append("transcript.saved", payload.clone())?;
    if verified {
        ws.append("transcript.verified", payload)?;
    }

    let locator = match page {
        Some(p) => format!("p:{p}:transcript"),
        None => "img:transcript".to_owned(),
    };
    let mut index = open_index(ws)?;
    index.upsert_chunk(&hash, &locator, text)?;
    Ok(abs)
}

/// Renders all un-transcribed, not-yet-rendered `needs_vision` pages for PDF
/// and DICOM files once into the app-data cache.
pub fn render_pending_vision(ws: &Workspace, worker: &mut Supervisor) -> Result<RenderReport> {
    let state = ws.replay()?.state;
    let mut report = RenderReport::default();

    for (hash, _) in state.collection(col::FILES) {
        let pending = pending_vision_pages(&state, hash);
        if pending.is_empty() {
            continue;
        }
        let disk_path = state
            .collection(col::PATHS)
            .filter(|(_, entry)| entry.get("hash").and_then(Value::as_str) == Some(hash.as_str()))
            .map(|(rel, _)| ws.root().join(rel))
            .find(|p| p.is_file());
        let Some(abs) = disk_path else {
            continue;
        };
        let format = route_file(&abs).unwrap_or(Format::Unknown);
        if !matches!(format, Format::Pdf | Format::Dicom) {
            continue;
        }
        let out_dir = render_dir(ws, hash)?;
        let mut to_render = Vec::new();
        for p in pending {
            let png = out_dir.join(format!("p{p}.png"));
            if png.is_file() && fs::metadata(&png).map(|m| m.len()).unwrap_or(0) > 0 {
                report.skipped += 1;
            } else {
                to_render.push(p);
            }
        }
        if to_render.is_empty() {
            continue;
        }
        let opts = json!({
            "format": format.as_str(),
            "pages": to_render,
            "out_dir": out_dir.to_string_lossy(),
            "long_edge": RENDER_LONG_EDGE,
        });
        match worker.request("render", &abs, opts) {
            Reply::Ok(resp) => {
                if let Some(items) = resp.get("rendered").and_then(Value::as_array) {
                    for item in items {
                        if let (Some(p), Some(path_str)) = (
                            item.get("page").and_then(Value::as_u64),
                            item.get("path").and_then(Value::as_str),
                        ) {
                            report
                                .rendered
                                .push((hash.clone(), p as u32, PathBuf::from(path_str)));
                        }
                    }
                }
                if let Some(skipped) = resp.get("skipped").and_then(Value::as_array) {
                    report.skipped += skipped.len();
                }
            }
            Reply::Failed { code, detail } => {
                report
                    .warnings
                    .push(format!("render {hash} failed ({code}): {detail}"));
            }
        }
    }
    Ok(report)
}
