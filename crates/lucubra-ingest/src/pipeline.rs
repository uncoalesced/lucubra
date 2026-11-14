// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Extraction queue (SPEC section 10.1 steps 3 to 6, ADR 0011).
//!
//! For each file in replayed state, in path order, the queue extracts what is
//! not yet in this device's index. A file another device already extracted is
//! re-extracted locally without a new event. Failures are logged once and not
//! retried automatically; `pending` files wait for `lucubra doctor` or for the
//! user's approval.

use std::collections::BTreeMap;
use std::fs;

use lucubra_core::state::col;
use lucubra_core::{Error, Result, Workspace};
use serde_json::{Value, json};

use crate::index::{INDEX_FILE, Index};
use crate::native::{Chunk, Extracted, extract_native};
use crate::route::{Extractor, Format, route_file};
use crate::worker::{Reply, Supervisor};

/// Message for formats with no reader in v1 (D49).
pub const UNSUPPORTED_STEPS: &str = "Revit and SketchUp files cannot be read. Export the model \
to PDF, IFC, DWG or images and drop the export in inbox/.";

/// What happened to one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Extracted now.
    Extracted {
        /// Chunks stored.
        chunks: usize,
        /// False when another device had already logged the extraction.
        logged: bool,
    },
    /// Failed or pending, with its code.
    Failed {
        /// ADR 0011 code.
        code: String,
        /// `failed` or `pending`.
        outcome: &'static str,
    },
    /// Needs a worker and none was available; nothing was logged.
    NoWorker,
}

/// One processed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Processed {
    /// A path of the file.
    pub path: String,
    /// Content hash.
    pub hash: String,
    /// Routed format.
    pub format: Format,
    /// Result.
    pub outcome: Outcome,
}

/// What one run of the queue did.
#[derive(Debug, Default)]
pub struct ExtractReport {
    /// Files processed in this run.
    pub processed: Vec<Processed>,
    /// Files already in the local index.
    pub up_to_date: usize,
    /// Files whose state is `failed` or `pending`, left alone.
    pub held: usize,
    /// Files with no readable copy at any known path.
    pub missing: Vec<String>,
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// Builds an `Extracted` from a worker's `ok` response.
pub fn from_worker(response: &Value) -> Extracted {
    let chunks = response
        .get("chunks")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|c| {
                    Some(Chunk {
                        locator: str_of(c, "ref")
                            .or_else(|| str_of(c, "locator"))?
                            .to_owned(),
                        text: str_of(c, "text")?.to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let needs_vision = response
        .get("needs_vision")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|n| n.as_u64().and_then(|n| u32::try_from(n).ok()))
                .collect()
        })
        .unwrap_or_default();
    let units_hint = response
        .get("units_hint")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Extracted {
        extractor: str_of(response, "extractor")
            .unwrap_or("lucubra-workers")
            .to_owned(),
        chunks,
        needs_vision,
        units_hint,
        truncated: response
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

/// A failure before logging: code, reason, and the missing tool if any.
struct Fail {
    code: String,
    detail: String,
    tool: Option<&'static str>,
}

fn log_failure(
    ws: &Workspace,
    hash: &str,
    path: &str,
    format: Format,
    fail: &Fail,
) -> Result<&'static str> {
    let outcome = if matches!(fail.code.as_str(), "tool_missing" | "online_only") {
        "pending"
    } else {
        "failed"
    };
    let mut data = json!({
        "hash": hash, "path": path, "format": format.as_str(),
        "outcome": outcome, "code": fail.code, "detail": fail.detail,
    });
    if let Some(tool) = fail.tool {
        data["tool"] = Value::String(tool.to_owned());
    }
    ws.append("file.failed", data)?;
    Ok(outcome)
}

/// Opens this device's index for a workspace.
pub fn open_index(ws: &Workspace) -> Result<Index> {
    let cache_dir = ws.cache_dir()?;
    fs::create_dir_all(&cache_dir)
        .map_err(|e| Error::io(format!("creating {}", cache_dir.display()), e))?;
    Index::open(&cache_dir.join(INDEX_FILE))
}

/// Runs the queue once. `worker` is used for worker formats; without one,
/// those files are reported as `NoWorker` and left for a later run.
pub fn extract_pending(
    ws: &Workspace,
    mut worker: Option<&mut Supervisor>,
) -> Result<ExtractReport> {
    let state = ws.replay()?.state;
    let mut index = open_index(ws)?;

    // Every known path for each hash, in path order.
    let mut by_hash: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (path, entry) in state.collection(col::PATHS) {
        if let Some(hash) = str_of(entry, "hash") {
            by_hash
                .entry(hash.to_owned())
                .or_default()
                .push(path.clone());
        }
    }
    let mut order: Vec<(String, String)> = by_hash
        .iter()
        .map(|(hash, paths)| (paths[0].clone(), hash.clone()))
        .collect();
    order.sort();

    let mut report = ExtractReport::default();
    for (_, hash) in order {
        let status = state
            .get(col::FILES, &hash)
            .and_then(|f| str_of(f, "status"))
            .unwrap_or("seen");
        if matches!(status, "failed" | "pending") {
            report.held += 1;
            continue;
        }
        if index.extractor_for(&hash)?.is_some() {
            report.up_to_date += 1;
            continue;
        }
        let paths = &by_hash[&hash];
        let Some(path) = paths.iter().find(|p| ws.root().join(p).is_file()).cloned() else {
            report.missing.push(paths[0].clone());
            continue;
        };
        let abs = ws.root().join(&path);
        let format = route_file(&abs).unwrap_or(Format::Unknown);
        let result: std::result::Result<Extracted, Fail> = match format.extractor() {
            Extractor::Native => extract_native(&abs, format).map_err(|f| Fail {
                code: f.code.to_owned(),
                detail: f.detail,
                tool: None,
            }),
            Extractor::Worker => {
                let Some(sup) = worker.as_deref_mut() else {
                    report.processed.push(Processed {
                        path,
                        hash,
                        format,
                        outcome: Outcome::NoWorker,
                    });
                    continue;
                };
                match sup.request("extract", &abs, json!({"format": format.as_str()})) {
                    Reply::Ok(response) => Ok(from_worker(&response)),
                    Reply::Failed { code, detail } => Err(Fail {
                        code,
                        detail,
                        tool: None,
                    }),
                }
            }
            Extractor::Tool(tool) => Err(Fail {
                code: "tool_missing".into(),
                detail: format!(
                    "{} needs {tool}; run `lucubra doctor` for install steps",
                    format.as_str()
                ),
                tool: Some(tool),
            }),
            Extractor::Unsupported => Err(Fail {
                code: "unsupported".into(),
                detail: UNSUPPORTED_STEPS.into(),
                tool: None,
            }),
            Extractor::None => continue,
        };
        let outcome = match result {
            Ok(extracted) => {
                index.store(&hash, &extracted)?;
                let logged = status != "extracted";
                if logged {
                    ws.append(
                        "file.extracted",
                        json!({
                            "hash": hash,
                            "format": format.as_str(),
                            "extractor": extracted.extractor,
                            "chunks": extracted.chunks.len(),
                            "units_hint": extracted.units_hint,
                            "needs_vision": extracted.needs_vision,
                            "truncated": extracted.truncated,
                        }),
                    )?;
                }
                Outcome::Extracted {
                    chunks: extracted.chunks.len(),
                    logged,
                }
            }
            Err(fail) => {
                let outcome = log_failure(ws, &hash, &path, format, &fail)?;
                Outcome::Failed {
                    code: fail.code,
                    outcome,
                }
            }
        };
        report.processed.push(Processed {
            path,
            hash,
            format,
            outcome,
        });
    }
    Ok(report)
}
