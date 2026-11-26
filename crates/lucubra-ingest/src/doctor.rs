// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! External Tier 2 tool detection for `lucubra doctor` (SPEC 10.2, ADR 0011, S07-S08).

use std::env;
use std::path::{Path, PathBuf};

use crate::worker::find_python;

/// Status of one external dependency or tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCheck {
    /// Short identifier (`python`, `oda`, `blender`).
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// Whether an executable was found.
    pub available: bool,
    /// Path to the executable if found.
    pub path: Option<PathBuf>,
    /// Specific variant detected (for example `oda` or `libredwg`).
    pub variant: Option<&'static str>,
    /// User-facing installation guidance when missing.
    pub install_steps: &'static str,
}

/// Combined status of external tools needed by ingestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolStatus {
    /// Python worker interpreter.
    pub python: ToolCheck,
    /// DWG converter (ODA File Converter or LibreDWG `dwg2dxf`).
    pub oda: ToolCheck,
    /// Headless Blender for `.blend` files.
    pub blender: ToolCheck,
}

impl ToolStatus {
    /// Whether the named ADR 0011 tool key (`oda` or `blender`) is available.
    pub fn has(&self, tool: &str) -> bool {
        match tool {
            "oda" => self.oda.available,
            "blender" => self.blender.available,
            "python" => self.python.available,
            _ => false,
        }
    }

    /// Every check in display order.
    pub fn all(&self) -> [&ToolCheck; 3] {
        [&self.python, &self.oda, &self.blender]
    }
}

fn candidate_names(base: &str) -> Vec<String> {
    if cfg!(windows) {
        let lower = base.to_ascii_lowercase();
        if lower.ends_with(".exe") || lower.ends_with(".cmd") || lower.ends_with(".bat") {
            vec![base.to_owned()]
        } else {
            vec![
                format!("{base}.exe"),
                format!("{base}.cmd"),
                format!("{base}.bat"),
                base.to_owned(),
            ]
        }
    } else {
        vec![base.to_owned()]
    }
}

fn find_on_path(env_override: &[&str], names: &[&str]) -> Option<PathBuf> {
    for var in env_override {
        if let Some(raw) = env::var_os(var) {
            let p = PathBuf::from(raw);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        for base in names {
            for candidate in candidate_names(base) {
                let full = dir.join(&candidate);
                if full.is_file() {
                    return Some(full);
                }
            }
        }
    }
    None
}

fn oda_variant(path: &Path) -> &'static str {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name.contains("dwg2dxf") {
        "libredwg"
    } else {
        "oda"
    }
}

/// Checks Python, ODA/LibreDWG, and Blender availability on this machine.
pub fn check_tools() -> ToolStatus {
    let python_path = find_python();
    let oda_path = find_on_path(
        &["LUCUBRA_ODA_PATH", "LUCUBRA_LIBREDWG_PATH"],
        &["ODAFileConverter", "dwg2dxf"],
    );
    let blender_path = find_on_path(&["LUCUBRA_BLENDER_PATH"], &["blender"]);

    ToolStatus {
        python: ToolCheck {
            id: "python",
            name: "Python worker interpreter",
            available: python_path.is_some(),
            path: python_path,
            variant: None,
            install_steps: "Install via `uv tool install lucubra` or set LUCUBRA_PYTHON to the venv Python.",
        },
        oda: ToolCheck {
            id: "oda",
            name: "DWG converter (ODA File Converter or LibreDWG dwg2dxf)",
            available: oda_path.is_some(),
            variant: oda_path.as_deref().map(oda_variant),
            path: oda_path,
            install_steps: "Install ODA File Converter (https://www.opendesign.com/guestfiles/oda_file_converter) or LibreDWG (`dwg2dxf`) and add it to PATH (or set LUCUBRA_ODA_PATH).",
        },
        blender: ToolCheck {
            id: "blender",
            name: "Blender (headless .blend reader)",
            available: blender_path.is_some(),
            variant: blender_path.as_ref().map(|_| "blender"),
            path: blender_path,
            install_steps: "Install Blender (https://www.blender.org/download/) and add `blender` to PATH (or set LUCUBRA_BLENDER_PATH).",
        },
    }
}

/// Builds the redacted diagnostic report for `lucubra doctor --report` (A8, S59).
///
/// Holds versions, tool availability, the last host profile and the counts of
/// open failure codes. It never holds extracted content or tool paths; file
/// names appear only when `include_names` is set.
pub fn report(
    tools: &ToolStatus,
    replay: Option<&lucubra_core::replay::Replay>,
    include_names: bool,
) -> String {
    use lucubra_core::state::col;
    use std::collections::BTreeMap;
    use std::fmt::Write as _;

    let mut out = String::new();
    let _ = writeln!(out, "lucubra {}", lucubra_core::VERSION);
    let _ = writeln!(out, "os: {} {}", env::consts::OS, env::consts::ARCH);
    for tool in tools.all() {
        let state = match (tool.available, tool.variant) {
            (true, Some(v)) => format!("ok [{v}]"),
            (true, None) => "ok".to_owned(),
            (false, _) => "missing".to_owned(),
        };
        let _ = writeln!(out, "tool {}: {state}", tool.id);
    }
    let Some(replay) = replay else {
        out.push_str("workspace: none\n");
        return out;
    };
    let host = replay
        .ordered
        .iter()
        .rev()
        .filter(|e| e.kind == "session.started")
        .find_map(|e| e.data.get("host").and_then(|h| h.as_str()));
    let _ = writeln!(out, "host profile: {}", host.unwrap_or("unknown"));
    let _ = writeln!(out, "events: {}", replay.ordered.len());
    let _ = writeln!(out, "events skipped: {}", replay.skipped.len());

    let state = &replay.state;
    let path_of = |hash: &str| {
        state
            .collection(col::PATHS)
            .find(|(_, v)| v.get("hash").and_then(|h| h.as_str()) == Some(hash))
            .map(|(p, _)| p.clone())
    };
    let mut codes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (hash, file) in state.collection(col::FILES) {
        let status = file.get("status").and_then(|s| s.as_str()).unwrap_or("");
        if !matches!(status, "failed" | "pending") {
            continue;
        }
        let code = file
            .get("code")
            .and_then(|c| c.as_str())
            .unwrap_or("unknown");
        let entry = codes.entry(format!("{status}/{code}")).or_default();
        entry.push(path_of(hash).unwrap_or_else(|| hash[..hash.len().min(8)].to_owned()));
    }
    if codes.is_empty() {
        out.push_str("open failures: none\n");
    }
    for (code, files) in &codes {
        let _ = writeln!(out, "open failures {code}: {}", files.len());
        if include_names {
            for name in files {
                let _ = writeln!(out, "  {name}");
            }
        }
    }
    if !include_names {
        out.push_str("file names: omitted (pass --include-names to add them)\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_names_last_host_and_omits_paths() {
        let dir = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let ws = lucubra_core::Workspace::init(dir.path(), data.path()).unwrap();
        for host in ["claude-desktop", "cursor"] {
            ws.append(
                "session.started",
                serde_json::json!({"session_id": host, "host": host}),
            )
            .unwrap();
        }
        let tools = check_tools();
        let text = report(&tools, Some(&ws.replay().unwrap()), false);
        assert!(text.contains("host profile: cursor"), "{text}");
        assert!(text.contains("open failures: none"), "{text}");
        for tool in tools.all() {
            if let Some(path) = &tool.path {
                assert!(!text.contains(&path.display().to_string()), "{text}");
            }
        }
        assert!(report(&tools, None, false).contains("workspace: none"));
    }

    #[test]
    fn tool_status_lookup_matches_known_ids() {
        let status = check_tools();
        assert_eq!(status.all().len(), 3);
        assert!(!status.has("unknown_tool"));
    }
}
