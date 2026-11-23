// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Workspace-relative path confinement shared by MCP tools and file moves.

use std::fs;
use std::path::{Component, Path, PathBuf};

pub(crate) fn is_reserved_windows_component(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).to_ascii_lowercase();
    matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || ((stem.starts_with("com") || stem.starts_with("lpt"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit())
}

/// Validates and resolves a workspace-relative path, rejecting traversal,
/// drive prefixes, NUL bytes, `.lucubra/`, reserved Windows device names,
/// and symlink escapes outside `root`.
///
/// Returns `(normalized_rel_forward_slashes, resolved_absolute_path)`.
pub fn resolve_workspace_path(
    root: &Path,
    raw: &str,
) -> std::result::Result<(String, PathBuf), String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("path must not be empty".to_owned());
    }
    if trimmed.contains('\0') {
        return Err("path contains NUL byte".to_owned());
    }
    let normalized = trimmed.replace('\\', "/");
    if normalized.starts_with('/') {
        return Err("path must be relative to the workspace root, not absolute".to_owned());
    }
    let bytes = normalized.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err("path must be relative to the workspace root, without drive prefix".to_owned());
    }

    let candidate_rel = Path::new(&normalized);
    let mut clean_parts: Vec<String> = Vec::new();
    for comp in candidate_rel.components() {
        match comp {
            Component::Normal(seg) => {
                let s = seg
                    .to_str()
                    .ok_or_else(|| "path contains invalid UTF-8".to_owned())?;
                if is_reserved_windows_component(s) {
                    return Err(format!(
                        "path component '{s}' is a reserved Windows device name"
                    ));
                }
                clean_parts.push(s.to_owned());
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err("path traversal outside the workspace is forbidden".to_owned());
            }
        }
    }

    if clean_parts.is_empty() {
        return Err("path must name a file inside the workspace".to_owned());
    }
    if clean_parts[0].eq_ignore_ascii_case(".lucubra") {
        return Err("access to internal .lucubra/ directory is forbidden".to_owned());
    }

    let canon_root = root
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize workspace root: {e}"))?;
    let mut current = canon_root.clone();
    for (idx, part) in clean_parts.iter().enumerate() {
        current.push(part);
        let is_last = idx + 1 == clean_parts.len();
        match fs::symlink_metadata(&current) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err("symlinks cannot be followed in workspace paths".to_owned());
                }
                if !is_last {
                    let canon = current
                        .canonicalize()
                        .map_err(|e| format!("cannot resolve path component: {e}"))?;
                    if !canon.starts_with(&canon_root) {
                        return Err("path escapes workspace root".to_owned());
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Non-existent suffix is allowed when callers check for optional files
                // (e.g., unit/subject checks will later return not_found cleanly).
                for rest in &clean_parts[idx + 1..] {
                    current.push(rest);
                }
                break;
            }
            Err(e) => return Err(format!("cannot inspect path '{}': {e}", current.display())),
        }
    }

    let rel_str = clean_parts.join("/");
    let joined = clean_parts.iter().fold(canon_root, |acc, p| acc.join(p));
    Ok((rel_str, joined))
}
