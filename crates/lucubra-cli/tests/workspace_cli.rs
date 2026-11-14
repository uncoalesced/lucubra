// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Runs the workspace subcommands of the built `lucubra` binary.

use std::fs;
use std::path::Path;
use std::process::Command;

fn lucubra(data: &Path, args: &[&str]) -> (Option<i32>, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_lucubra"))
        .args(args)
        .env("LUCUBRA_DATA_DIR", data)
        .output()
        .unwrap();
    (
        out.status.code(),
        String::from_utf8(out.stdout).unwrap().replace("\r\n", "\n"),
        String::from_utf8(out.stderr).unwrap(),
    )
}

fn state_hash(stdout: &str) -> String {
    stdout
        .lines()
        .find_map(|l| l.strip_prefix("state-hash: "))
        .unwrap_or_else(|| panic!("no state-hash line in {stdout}"))
        .to_owned()
}

#[test]
fn rebuild_after_deleting_the_cache_prints_the_same_hash() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    let data = tmp.path().join("data");
    let ws_arg = ws.to_str().unwrap();

    let (code, out, err) = lucubra(&data, &["init", "--workspace", ws_arg]);
    assert_eq!(code, Some(0), "{err}");
    assert!(ws.join("AGENTS.md").is_file());
    assert!(ws.join(".lucubra/VERSION").is_file());
    let init_hash = state_hash(&out);

    let (code, out, _) = lucubra(&data, &["rebuild", "-w", ws_arg]);
    assert_eq!(code, Some(0));
    let first = state_hash(&out);
    assert_eq!(first, init_hash);

    fs::remove_dir_all(data.join("workspaces")).unwrap();
    let (code, out, _) = lucubra(&data, &["rebuild", "-w", ws_arg]);
    assert_eq!(code, Some(0));
    assert_eq!(state_hash(&out), first);

    let (code, out, _) = lucubra(&data, &["status", "-w", ws_arg]);
    assert_eq!(code, Some(0));
    assert!(out.contains("events: 1\n"), "{out}");
    assert_eq!(state_hash(&out), first);
}

#[test]
fn reset_keeps_sources_notes_and_events() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    let data = tmp.path().join("data");
    let ws_arg = ws.to_str().unwrap();
    assert_eq!(lucubra(&data, &["init", "-w", ws_arg]).0, Some(0));

    let sources = ws.join("subjects/maths/algebra/sources");
    fs::create_dir_all(&sources).unwrap();
    fs::write(sources.join("book.pdf"), b"%PDF fixture").unwrap();
    let events = fs::read_dir(ws.join(".lucubra/events"))
        .unwrap()
        .map(|e| fs::read(e.unwrap().path()).unwrap())
        .collect::<Vec<_>>();

    let (code, out, err) = lucubra(&data, &["reset", "--yes", "-w", ws_arg]);
    assert_eq!(code, Some(0), "{err}");
    assert!(out.contains("state-hash: "));
    assert_eq!(fs::read(sources.join("book.pdf")).unwrap(), b"%PDF fixture");
    let after = fs::read_dir(ws.join(".lucubra/events"))
        .unwrap()
        .map(|e| fs::read(e.unwrap().path()).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(events, after);
    assert!(ws.join("AGENTS.md").is_file());
}

#[test]
fn commands_refuse_a_folder_that_is_not_a_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    let (code, _, err) = lucubra(
        &tmp.path().join("data"),
        &["rebuild", "-w", tmp.path().to_str().unwrap()],
    );
    assert_eq!(code, Some(1));
    assert!(err.contains("not a Lucubra workspace"), "{err}");
}
