// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Extraction queue (SPEC section 10.1 steps 3 to 6, ADR 0011).
//!
//! For each file in replayed state, in path order, the queue extracts what is
//! not yet in this device's index. A file another device already extracted is
//! re-extracted locally without a new event. Failures are logged once; a file
//! marked `pending` / `tool_missing` is automatically retried once `doctor`
//! finds the missing tool.

use std::collections::BTreeMap;
use std::fs;
use std::time::Duration;

use lucubra_core::hlc::Hlc;
use lucubra_core::state::col;
use lucubra_core::{Error, Result, Workspace};
use serde_json::{Value, json};

use crate::doctor::{ToolStatus, check_tools};
use crate::index::{INDEX_FILE, Index};
use crate::native::{Chunk, Extracted, extract_native};
use crate::route::{Extractor, Format, route_file};
use crate::tier2::{extract_blend, extract_dwg};
use crate::vision::render_pending_vision;
use crate::worker::{DEFAULT_TIMEOUT, Reply, Supervisor};

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

/// Progress update emitted after each queued file is processed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress<'a> {
    /// Files processed so far (1-based).
    pub done: usize,
    /// Total files queued in this run.
    pub total: usize,
    /// Workspace-relative path.
    pub path: &'a str,
    /// Detected format.
    pub format: Format,
    /// Outcome.
    pub outcome: &'a Outcome,
}

/// Options controlling extraction queue runs.
pub struct ExtractOptions<'a> {
    /// External tools available on this machine.
    pub tools: ToolStatus,
    /// Per-request timeout for Tier 2 tools.
    pub timeout: Duration,
    /// Optional progress callback.
    pub on_progress: Option<&'a mut dyn FnMut(&Progress<'_>)>,
}

impl Default for ExtractOptions<'_> {
    fn default() -> Self {
        Self {
            tools: check_tools(),
            timeout: DEFAULT_TIMEOUT,
            on_progress: None,
        }
    }
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
    /// Vision PNG pages rendered in this run.
    pub vision_rendered: usize,
    /// Vision PNG pages already cached or transcribed.
    pub vision_skipped: usize,
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn non_empty_str(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
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
    let meta = response.get("meta");
    Extracted {
        extractor: str_of(response, "extractor")
            .unwrap_or("lucubra-workers")
            .to_owned(),
        title: non_empty_str(meta.and_then(|m| m.get("title"))),
        subject: non_empty_str(meta.and_then(|m| m.get("subject"))),
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
    observed: Option<&Hlc>,
    hash: &str,
    path: &str,
    format: Format,
    fail: &Fail,
) -> Result<(&'static str, Option<Hlc>)> {
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
    let ev = ws.append_observed("file.failed", data, observed)?;
    Ok((outcome, Hlc::parse(&ev.hlc)))
}

/// Opens this device's index for a workspace.
pub fn open_index(ws: &Workspace) -> Result<Index> {
    let cache_dir = ws.cache_dir()?;
    fs::create_dir_all(&cache_dir)
        .map_err(|e| Error::io(format!("creating {}", cache_dir.display()), e))?;
    Index::open(&cache_dir.join(INDEX_FILE))
}

/// Runs the queue once with default options.
pub fn extract_pending(ws: &Workspace, worker: Option<&mut Supervisor>) -> Result<ExtractReport> {
    extract_pending_with_options(ws, worker, ExtractOptions::default())
}

/// Runs the queue once using explicit tool status, timeout, and progress callback.
pub fn extract_pending_with_options(
    ws: &Workspace,
    mut worker: Option<&mut Supervisor>,
    mut opts: ExtractOptions<'_>,
) -> Result<ExtractReport> {
    let replay = ws.replay()?;
    let state = replay.state;
    let mut observed = replay.max_hlc;
    let cache_dir = ws.cache_dir()?;
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
    let mut work: Vec<(String, String, Format, bool)> = Vec::new();

    for (_, hash) in order {
        let file_entry = state.get(col::FILES, &hash);
        let status = file_entry
            .and_then(|f| str_of(f, "status"))
            .unwrap_or("seen");
        let code = file_entry.and_then(|f| str_of(f, "code"));
        let missing_tool = file_entry.and_then(|f| str_of(f, "tool"));

        let retry_tool = status == "pending"
            && code == Some("tool_missing")
            && missing_tool.is_some_and(|t| opts.tools.has(t));

        if matches!(status, "failed" | "pending") && !retry_tool {
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
        let format = route_file(&ws.root().join(&path)).unwrap_or(Format::Unknown);
        if format.extractor() == Extractor::None {
            continue;
        }
        work.push((path, hash, format, status == "extracted"));
    }

    let total = work.len();
    for (idx, (path, hash, format, already_extracted)) in work.into_iter().enumerate() {
        let abs = ws.root().join(&path);
        let result: std::result::Result<Extracted, Fail> = match format.extractor() {
            Extractor::Native => extract_native(&abs, format).map_err(|f| Fail {
                code: f.code.to_owned(),
                detail: f.detail,
                tool: None,
            }),
            Extractor::Worker => {
                let Some(sup) = worker.as_deref_mut() else {
                    let outcome = Outcome::NoWorker;
                    if let Some(cb) = opts.on_progress.as_deref_mut() {
                        cb(&Progress {
                            done: idx + 1,
                            total,
                            path: &path,
                            format,
                            outcome: &outcome,
                        });
                    }
                    report.processed.push(Processed {
                        path,
                        hash,
                        format,
                        outcome,
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
            Extractor::Tool("oda") => {
                if opts.tools.oda.available {
                    let Some(sup) = worker.as_deref_mut() else {
                        let outcome = Outcome::NoWorker;
                        if let Some(cb) = opts.on_progress.as_deref_mut() {
                            cb(&Progress {
                                done: idx + 1,
                                total,
                                path: &path,
                                format,
                                outcome: &outcome,
                            });
                        }
                        report.processed.push(Processed {
                            path,
                            hash,
                            format,
                            outcome,
                        });
                        continue;
                    };
                    extract_dwg(&opts.tools, &abs, &cache_dir, sup, opts.timeout).map_err(|e| {
                        Fail {
                            code: e.code.to_owned(),
                            detail: e.detail,
                            tool: e.tool,
                        }
                    })
                } else {
                    Err(Fail {
                        code: "tool_missing".into(),
                        detail: opts.tools.oda.install_steps.into(),
                        tool: Some("oda"),
                    })
                }
            }
            Extractor::Tool("blender") => {
                if opts.tools.blender.available {
                    extract_blend(&opts.tools, &abs, &cache_dir, opts.timeout).map_err(|e| Fail {
                        code: e.code.to_owned(),
                        detail: e.detail,
                        tool: e.tool,
                    })
                } else {
                    Err(Fail {
                        code: "tool_missing".into(),
                        detail: opts.tools.blender.install_steps.into(),
                        tool: Some("blender"),
                    })
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
                let logged = !already_extracted;
                if logged {
                    let mut data = json!({
                        "hash": hash,
                        "format": format.as_str(),
                        "extractor": extracted.extractor,
                        "chunks": extracted.chunks.len(),
                        "units_hint": extracted.units_hint,
                        "needs_vision": extracted.needs_vision,
                        "truncated": extracted.truncated,
                    });
                    if let Some(title) = extracted.title {
                        data["title"] = Value::String(title);
                    }
                    if let Some(subject) = extracted.subject {
                        data["subject"] = Value::String(subject);
                    }
                    let ev = ws.append_observed("file.extracted", data, observed.as_ref())?;
                    observed = Hlc::parse(&ev.hlc).or(observed);
                }
                Outcome::Extracted {
                    chunks: extracted.chunks.len(),
                    logged,
                }
            }
            Err(fail) => {
                let (outcome, next_hlc) =
                    log_failure(ws, observed.as_ref(), &hash, &path, format, &fail)?;
                observed = next_hlc.or(observed);
                Outcome::Failed {
                    code: fail.code,
                    outcome,
                }
            }
        };
        if let Some(cb) = opts.on_progress.as_deref_mut() {
            cb(&Progress {
                done: idx + 1,
                total,
                path: &path,
                format,
                outcome: &outcome,
            });
        }
        report.processed.push(Processed {
            path,
            hash,
            format,
            outcome,
        });
    }
    if let Some(sup) = worker {
        let vision = render_pending_vision(ws, sup)?;
        report.vision_rendered = vision.rendered.len();
        report.vision_skipped = vision.skipped;
    }
    Ok(report)
}
