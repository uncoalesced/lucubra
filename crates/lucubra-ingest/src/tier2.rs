// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Tier 2 external-tool adapters for `.dwg` and `.blend` (SPEC 10.2, S07-S08).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static SEQ: AtomicU64 = AtomicU64::new(0);

use serde_json::{Value, json};

use crate::doctor::ToolStatus;
use crate::native::{Chunk, Extracted, split_text};
use crate::worker::{Reply, Supervisor};

const BLENDER_SCRIPT: &str = include_str!("../../../python/lucubra_workers/blender_extract.py");

/// Failure returned by a Tier 2 adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tier2Error {
    /// ADR 0011 failure code.
    pub code: &'static str,
    /// Human-readable reason.
    pub detail: String,
    /// Missing tool id if `tool_missing`.
    pub tool: Option<&'static str>,
}

fn err(code: &'static str, detail: impl Into<String>, tool: Option<&'static str>) -> Tier2Error {
    Tier2Error {
        code,
        detail: detail.into(),
        tool,
    }
}

struct TempWorkDir(PathBuf);

impl TempWorkDir {
    fn new(base: &Path) -> std::io::Result<Self> {
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = base
            .join("tier2")
            .join(format!("{}-{seq}", std::process::id()));
        fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }
}

impl Drop for TempWorkDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run_with_timeout(mut cmd: Command, timeout: Duration, label: &str) -> Result<(), Tier2Error> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| err("worker_crashed", format!("starting {label}: {e}"), None))?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    return Ok(());
                }
                let out = child.wait_with_output().ok();
                let stderr = out
                    .as_ref()
                    .map(|o| String::from_utf8_lossy(&o.stderr).trim().to_owned())
                    .unwrap_or_default();
                let msg = if stderr.is_empty() {
                    format!("{label} exited with {status}")
                } else {
                    format!("{label} exited with {status}: {stderr}")
                };
                return Err(err("corrupt", msg, None));
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(err(
                        "timeout",
                        format!("{label} exceeded {} s", timeout.as_secs()),
                        None,
                    ));
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(err(
                    "worker_crashed",
                    format!("waiting on {label}: {e}"),
                    None,
                ));
            }
        }
    }
}

/// Converts a DWG file to DXF using LibreDWG or ODA File Converter, then
/// extracts the DXF through the Python worker.
pub fn extract_dwg(
    tools: &ToolStatus,
    dwg_path: &Path,
    cache_dir: &Path,
    worker: &mut Supervisor,
    timeout: Duration,
) -> Result<Extracted, Tier2Error> {
    let Some(tool_path) = tools.oda.path.as_deref() else {
        return Err(err("tool_missing", tools.oda.install_steps, Some("oda")));
    };
    let tmp = TempWorkDir::new(cache_dir)
        .map_err(|e| err("corrupt", format!("creating tier2 scratch dir: {e}"), None))?;
    let out_dir = tmp.0.join("out");
    fs::create_dir_all(&out_dir)
        .map_err(|e| err("corrupt", format!("creating tier2 out dir: {e}"), None))?;

    let variant = tools.oda.variant.unwrap_or("oda");
    let mut cmd = Command::new(tool_path);
    if variant == "libredwg" {
        let dxf_path = out_dir.join("converted.dxf");
        cmd.arg("-o").arg(&dxf_path).arg(dwg_path);
        run_with_timeout(cmd, timeout, "dwg2dxf")?;
    } else {
        let in_dir = dwg_path.parent().unwrap_or_else(|| Path::new("."));
        let filter = dwg_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("*.dwg");
        cmd.arg(in_dir)
            .arg(&out_dir)
            .arg("ACAD2018")
            .arg("DXF")
            .arg("0")
            .arg("1")
            .arg(filter);
        run_with_timeout(cmd, timeout, "ODAFileConverter")?;
    }

    let dxf_path = fs::read_dir(&out_dir)
        .map_err(|e| err("corrupt", format!("listing converted DXF: {e}"), None))?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("dxf"))
        })
        .ok_or_else(|| err("corrupt", "DWG converter produced no .dxf file", None))?;

    match worker.request("extract", &dxf_path, json!({"format": "dxf"})) {
        Reply::Ok(resp) => {
            let mut extracted = crate::pipeline::from_worker(&resp);
            extracted.extractor = format!("{variant}+{}", extracted.extractor);
            Ok(extracted)
        }
        Reply::Failed { code, detail } => {
            let mapped = match code.as_str() {
                "timeout" => "timeout",
                "limit_exceeded" => "limit_exceeded",
                "encrypted" => "encrypted",
                "worker_crashed" => "worker_crashed",
                _ => "corrupt",
            };
            Err(err(mapped, detail, None))
        }
    }
}

/// Runs headless Blender on a `.blend` file to list objects, materials and modifiers.
pub fn extract_blend(
    tools: &ToolStatus,
    blend_path: &Path,
    cache_dir: &Path,
    timeout: Duration,
) -> Result<Extracted, Tier2Error> {
    let Some(tool_path) = tools.blender.path.as_deref() else {
        return Err(err(
            "tool_missing",
            tools.blender.install_steps,
            Some("blender"),
        ));
    };
    let tmp = TempWorkDir::new(cache_dir)
        .map_err(|e| err("corrupt", format!("creating tier2 scratch dir: {e}"), None))?;
    let script_path = tmp.0.join("blender_extract.py");
    let out_json = tmp.0.join("scene.json");
    fs::write(&script_path, BLENDER_SCRIPT)
        .map_err(|e| err("corrupt", format!("writing blender helper: {e}"), None))?;

    let mut cmd = Command::new(tool_path);
    cmd.args(blender_args(blend_path, &script_path, &out_json));
    run_with_timeout(cmd, timeout, "blender")?;

    let raw = fs::read_to_string(&out_json)
        .map_err(|e| err("corrupt", format!("reading blender output: {e}"), None))?;
    let doc: Value = serde_json::from_str(&raw)
        .map_err(|e| err("corrupt", format!("invalid blender JSON: {e}"), None))?;

    let scene = doc.get("scene").and_then(Value::as_str).unwrap_or("Scene");
    let objects = doc
        .get("objects")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let materials: Vec<&str> = doc
        .get("materials")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();

    let mut chunks = vec![Chunk {
        locator: "o:summary".into(),
        text: format!(
            "Format: Blender scene\nScene: {scene}\nObjects: {}\nMaterials: {}",
            objects.len(),
            if materials.is_empty() {
                "none".to_owned()
            } else {
                materials.join(", ")
            }
        ),
    }];
    let mut hints = Vec::new();
    for obj in &objects {
        let name = obj.get("name").and_then(Value::as_str).unwrap_or("Object");
        let kind = obj.get("type").and_then(Value::as_str).unwrap_or("UNKNOWN");
        hints.push(name.to_owned());
        let mats: Vec<&str> = obj
            .get("materials")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let mods: Vec<&str> = obj
            .get("modifiers")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let mut line = format!("Object: {name} ({kind})");
        if let Some(verts) = obj.get("vertices").and_then(Value::as_u64) {
            let polys = obj.get("polygons").and_then(Value::as_u64).unwrap_or(0);
            line.push_str(&format!("\nGeometry: {verts} vertices, {polys} polygons"));
        }
        if !mats.is_empty() {
            line.push_str(&format!("\nMaterials: {}", mats.join(", ")));
        }
        if !mods.is_empty() {
            line.push_str(&format!("\nModifiers: {}", mods.join(", ")));
        }
        for (part, piece) in split_text(&line).into_iter().enumerate() {
            let locator = if part == 0 {
                format!("o:{name}")
            } else {
                format!("o:{name}.{part}")
            };
            chunks.push(Chunk {
                locator,
                text: piece,
            });
        }
    }

    Ok(Extracted {
        extractor: "blender".into(),
        chunks,
        units_hint: hints,
        ..Extracted::default()
    })
}

/// Blender arguments for headless extraction; `--disable-autoexec` stops
/// scripts embedded in an untrusted `.blend` from running.
fn blender_args(blend: &Path, script: &Path, out_json: &Path) -> Vec<std::ffi::OsString> {
    vec![
        "--background".into(),
        "--disable-autoexec".into(),
        blend.into(),
        "--python".into(),
        script.into(),
        "--".into(),
        out_json.into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blender_never_autoexecutes_embedded_scripts() {
        let args = blender_args(Path::new("a.blend"), Path::new("s.py"), Path::new("o.json"));
        let disable = args.iter().position(|a| a == "--disable-autoexec").unwrap();
        let file = args.iter().position(|a| a == "a.blend").unwrap();
        assert!(disable < file, "autoexec must be off before the file loads");
    }
}
