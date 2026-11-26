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

#[test]
fn doctor_ingest_transcript_and_search_work_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    let data = tmp.path().join("data");
    let ws_arg = ws.to_str().unwrap();

    // Standalone `doctor` succeeds outside a workspace and reports tool names.
    let (code, doc_out, doc_err) = Command::new(env!("CARGO_BIN_EXE_lucubra"))
        .arg("doctor")
        .current_dir(tmp.path())
        .env("LUCUBRA_DATA_DIR", &data)
        .env("PATH", "")
        .output()
        .map(|o| {
            (
                o.status.code(),
                String::from_utf8(o.stdout).unwrap(),
                String::from_utf8(o.stderr).unwrap(),
            )
        })
        .unwrap();
    assert_eq!(code, Some(0), "{doc_err}");
    assert!(doc_out.contains("oda"), "{doc_out}");
    assert!(doc_out.contains("blender"), "{doc_out}");

    assert_eq!(lucubra(&data, &["init", "-w", ws_arg]).0, Some(0));
    fs::write(
        ws.join("inbox/notes.md"),
        "# Unit 1 Optics\nSnell law governs refraction.\n",
    )
    .unwrap();
    fs::write(
        ws.join("inbox/photo.png"),
        b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x02\x00\x00\x00\x90wS\xde",
    )
    .unwrap();

    let (code, ingest_out, err) = lucubra(&data, &["ingest", "-w", ws_arg]);
    assert_eq!(code, Some(0), "{err}");
    assert!(
        ingest_out.contains("extracted inbox/notes.md"),
        "{ingest_out}"
    );

    let (code, search_out, err) = lucubra(&data, &["search", "-w", ws_arg, "refraction"]);
    assert_eq!(code, Some(0), "{err}");
    assert!(search_out.contains("results: 1"), "{search_out}");

    let photo_hash = ingest_out
        .lines()
        .find_map(|l| l.trim().strip_prefix("inbox/photo.png [image] "))
        .unwrap_or_else(|| panic!("missing scan hash in {ingest_out}"))
        .trim()
        .to_owned();
    let (code, tr_out, err) = lucubra(
        &data,
        &[
            "transcript",
            "-w",
            ws_arg,
            "--hash",
            &photo_hash,
            "--text",
            "Handwritten lens equation notes.",
        ],
    );
    assert_eq!(code, Some(0), "{err}");
    assert!(tr_out.contains("photo.png.transcript.md"), "{tr_out}");

    let (code, tr_search, err) = lucubra(&data, &["search", "-w", ws_arg, "lens"]);
    assert_eq!(code, Some(0), "{err}");
    assert!(tr_search.contains("results: 1"), "{tr_search}");
}

fn doctor_report(data: &Path, ws_arg: &str, extra: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_lucubra"))
        .args(["doctor", "-w", ws_arg, "--report"])
        .args(extra)
        .env("LUCUBRA_DATA_DIR", data)
        .env("PATH", "")
        .env_remove("LUCUBRA_BLENDER_PATH")
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(0), "{stdout}");
    let path = stdout
        .lines()
        .find_map(|l| l.strip_prefix("report written: "))
        .unwrap_or_else(|| panic!("no report line in {stdout}"));
    fs::read_to_string(path.trim()).unwrap()
}

#[test]
fn s59_doctor_report_is_redacted_unless_names_are_opted_in() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    let data = tmp.path().join("data");
    let ws_arg = ws.to_str().unwrap();
    assert_eq!(lucubra(&data, &["init", "-w", ws_arg]).0, Some(0));
    fs::write(
        ws.join("inbox/zebra-quokka.md"),
        "Quokkazebra refraction notes.\n",
    )
    .unwrap();
    fs::write(
        ws.join("inbox/gearbox-secret.blend"),
        b"BLENDER-v300 not a real scene",
    )
    .unwrap();

    let ingest = Command::new(env!("CARGO_BIN_EXE_lucubra"))
        .args(["ingest", "-w", ws_arg])
        .env("LUCUBRA_DATA_DIR", &data)
        .env("PATH", "")
        .env_remove("LUCUBRA_BLENDER_PATH")
        .output()
        .unwrap();
    assert_eq!(ingest.status.code(), Some(0));

    let report = doctor_report(&data, ws_arg, &[]);
    assert!(
        report.contains(&format!("lucubra {}", env!("CARGO_PKG_VERSION"))),
        "{report}"
    );
    assert!(report.contains("tool blender: missing"), "{report}");
    assert!(report.contains("host profile: unknown"), "{report}");
    assert!(
        report.contains("open failures pending/tool_missing: 1"),
        "{report}"
    );
    assert!(report.contains("file names: omitted"), "{report}");
    for leak in ["gearbox-secret", "zebra-quokka", "Quokkazebra", ws_arg] {
        assert!(!report.contains(leak), "report leaks {leak}: {report}");
    }

    let named = doctor_report(&data, ws_arg, &["--include-names"]);
    assert!(named.contains("inbox/gearbox-secret.blend"), "{named}");
    assert!(!named.contains("Quokkazebra"), "{named}");
}
