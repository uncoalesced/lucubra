// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Grounding of practice item sources against the search index (SPEC
//! section 13.2, D53), shared by MCP item submission and Pack-mode handoff
//! ingest.

use lucubra_core::grounding::{verify_cell, verify_name, verify_text};
use lucubra_core::state::{State, col};
use serde_json::Value;

use crate::index::Index;

/// A full file hash from a full hash or a unique prefix of one.
pub fn resolve_hash(state: &State, hash_or_prefix: &str) -> Result<String, String> {
    let wanted = hash_or_prefix.trim();
    if wanted.is_empty() {
        return Err("file_hash is empty".into());
    }
    if state.get(col::FILES, wanted).is_some() {
        return Ok(wanted.to_owned());
    }
    let matches: Vec<&String> = state
        .collection(col::FILES)
        .map(|(k, _)| k)
        .filter(|k| k.starts_with(wanted))
        .collect();
    match matches.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(format!("no file matches '{wanted}'")),
        _ => Err(format!(
            "'{wanted}' matches more than one file; give more of the hash"
        )),
    }
}

/// Checks an anchor anywhere in a file's indexed text (Pack mode, where the
/// host may not know the exact chunk; SPEC 16.3).
pub fn ground_in_file(index: &Index, hash: &str, anchor: &str) -> Result<(), String> {
    let locators = index
        .locators_for_hash(hash, 100_000)
        .map_err(|e| format!("index read failed: {e}"))?;
    let bodies: Vec<String> = locators
        .iter()
        .filter_map(|l| index.chunk(hash, l).ok().flatten())
        .collect();
    if bodies.is_empty() {
        return Err("the cited file has no indexed text".into());
    }
    let refs: Vec<&str> = bodies.iter().map(String::as_str).collect();
    verify_text(anchor, &refs)
        .map(|_| ())
        .map_err(|r| r.to_string())
}

/// Verifies every source of a strict item against the index (SPEC section
/// 13.2, D53). Returns the grounding label or the first failure reason.
pub fn ground_sources(
    state: &State,
    index: Option<&Index>,
    sources: &[Value],
) -> std::result::Result<&'static str, String> {
    let index = index.ok_or("the search index is unavailable; run lucubra_ingest scan first")?;
    let mut transcript_only = true;
    for (sidx, src) in sources.iter().enumerate() {
        let fail = |reason: String| format!("sources[{sidx}]: {reason}");
        let text = |k: &str| {
            src.get(k)
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default()
        };
        let full = resolve_hash(state, text("file_hash")).map_err(fail)?;
        let (loc, anchor) = (text("location"), text("anchor"));
        let locators = index
            .locators_for_hash(&full, 100_000)
            .map_err(|e| fail(format!("index read failed: {e}")))?;
        let chunk = |l: &str| index.chunk(&full, l).ok().flatten();
        if let Some(page) = loc.strip_suffix(":transcript") {
            let body = chunk(loc).ok_or_else(|| fail(format!("no transcript at '{loc}'")))?;
            verify_text(anchor, &[&body]).map_err(|r| fail(r.to_string()))?;
            let key = match page.strip_prefix("p:") {
                Some(n) => format!("{full}/p:{n}"),
                None => format!("{full}/img"),
            };
            let verified = state
                .get(col::TRANSCRIPTS, &key)
                .and_then(|t| t.get("verified"))
                .and_then(Value::as_bool)
                == Some(true);
            transcript_only &= !verified;
            continue;
        }
        transcript_only = false;
        if let Some((sheet, cell)) = loc.strip_prefix("x:").and_then(|r| r.split_once('!'))
            && !cell.contains(':')
            && cell != "summary"
        {
            let prefix = format!("x:{sheet}!");
            let bodies: Vec<String> = locators
                .iter()
                .filter(|l| l.starts_with(&prefix))
                .filter_map(|l| chunk(l))
                .collect();
            let refs: Vec<&str> = bodies.iter().map(String::as_str).collect();
            verify_cell(cell, anchor, &refs).map_err(|r| fail(r.to_string()))?;
        } else if loc.starts_with("l:") || loc.starts_with("o:") {
            verify_name(loc, &locators).map_err(|r| fail(r.to_string()))?;
        } else {
            let pos = locators
                .iter()
                .position(|l| l == loc)
                .ok_or_else(|| fail(format!("location '{loc}' does not exist in that file")))?;
            let near = &locators[pos.saturating_sub(1)..(pos + 2).min(locators.len())];
            let bodies: Vec<String> = near.iter().filter_map(|l| chunk(l)).collect();
            let refs: Vec<&str> = bodies.iter().map(String::as_str).collect();
            verify_text(anchor, &refs).map_err(|r| fail(r.to_string()))?;
        }
    }
    Ok(if transcript_only {
        "unverified_transcript"
    } else {
        "verified"
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucubra_core::Workspace;
    use serde_json::json;

    const A: &str = "aaaa1111";
    const B: &str = "aaaa2222";

    struct Fx {
        _root: tempfile::TempDir,
        _data: tempfile::TempDir,
        state: State,
        index: Index,
    }

    fn fixture(transcript_verified: bool) -> Fx {
        let root = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let ws = Workspace::init(root.path(), data.path()).unwrap();
        for (path, hash) in [("inbox/a.md", A), ("inbox/b.png", B)] {
            ws.append(
                "file.seen",
                json!({"path": path, "hash": hash, "bytes": 10}),
            )
            .unwrap();
        }
        ws.append(
            "transcript.saved",
            json!({"hash": B, "verified": transcript_verified}),
        )
        .unwrap();
        let mut index = Index::open(&data.path().join("index.db")).unwrap();
        index
            .upsert_chunk(A, "h:1", "Snell law governs refraction at a boundary.")
            .unwrap();
        index.upsert_chunk(A, "h:2", "Lenses focus light.").unwrap();
        index
            .upsert_chunk(B, "img:transcript", "Handwritten lens equation.")
            .unwrap();
        let state = ws.replay().unwrap().state;
        Fx {
            _root: root,
            _data: data,
            state,
            index,
        }
    }

    #[test]
    fn resolve_hash_takes_full_or_unique_prefix() {
        let fx = fixture(false);
        assert_eq!(resolve_hash(&fx.state, A).unwrap(), A);
        assert_eq!(resolve_hash(&fx.state, " aaaa1 ").unwrap(), A);
        assert!(
            resolve_hash(&fx.state, "aaaa")
                .unwrap_err()
                .contains("more than one")
        );
        assert!(
            resolve_hash(&fx.state, "ffff")
                .unwrap_err()
                .contains("no file")
        );
        assert!(resolve_hash(&fx.state, "  ").unwrap_err().contains("empty"));
    }

    #[test]
    fn ground_in_file_searches_every_chunk() {
        let fx = fixture(false);
        ground_in_file(&fx.index, A, "lenses focus light").unwrap();
        assert!(ground_in_file(&fx.index, A, "magnets attract iron").is_err());
        let none = ground_in_file(&fx.index, "cccc", "anything").unwrap_err();
        assert!(none.contains("no indexed text"), "{none}");
    }

    #[test]
    fn ground_sources_labels_verified_and_transcript_only() {
        let fx = fixture(false);
        let chunk =
            json!({"file_hash": A, "location": "h:1", "anchor": "Snell law governs refraction"});
        assert_eq!(
            ground_sources(&fx.state, Some(&fx.index), std::slice::from_ref(&chunk)),
            Ok("verified")
        );
        let tr = json!({"file_hash": B, "location": "img:transcript", "anchor": "lens equation"});
        assert_eq!(
            ground_sources(&fx.state, Some(&fx.index), std::slice::from_ref(&tr)),
            Ok("unverified_transcript")
        );
        assert_eq!(
            ground_sources(&fx.state, Some(&fx.index), &[tr.clone(), chunk.clone()]),
            Ok("verified")
        );
        let verified = fixture(true);
        assert_eq!(
            ground_sources(&verified.state, Some(&verified.index), &[tr]),
            Ok("verified")
        );
    }

    #[test]
    fn ground_sources_reports_the_failing_source() {
        let fx = fixture(false);
        assert!(
            ground_sources(&fx.state, None, &[])
                .unwrap_err()
                .contains("unavailable")
        );
        let missing_loc = json!({"file_hash": A, "location": "h:9", "anchor": "Snell"});
        let err = ground_sources(&fx.state, Some(&fx.index), &[missing_loc]).unwrap_err();
        assert!(
            err.starts_with("sources[0]:") && err.contains("does not exist"),
            "{err}"
        );
        let wrong = json!({"file_hash": A, "location": "h:1", "anchor": "magnets attract iron"});
        let ok = json!({"file_hash": A, "location": "h:2", "anchor": "Lenses focus light"});
        let err = ground_sources(&fx.state, Some(&fx.index), &[ok, wrong]).unwrap_err();
        assert!(err.starts_with("sources[1]:"), "{err}");
    }
}
