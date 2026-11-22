// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Tier 2 missing/found tool flow and unsupported format errors (GATES M2-8, S07-S09).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use lucubra_core::Workspace;
use lucubra_core::state::col;
use lucubra_ingest::doctor::{ToolCheck, ToolStatus};
use lucubra_ingest::pipeline::{ExtractOptions, Outcome, extract_pending_with_options, open_index};
use lucubra_ingest::scan;
use lucubra_ingest::worker::{Supervisor, WorkerSpec};

fn venv_python() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    [
        root.join(".venv/Scripts/python.exe"),
        root.join(".venv/bin/python3"),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

fn empty_tools() -> ToolStatus {
    ToolStatus {
        python: ToolCheck {
            id: "python",
            name: "Python",
            available: false,
            path: None,
            variant: None,
            install_steps: "install python",
        },
        oda: ToolCheck {
            id: "oda",
            name: "ODA",
            available: false,
            path: None,
            variant: None,
            install_steps: "install oda or libredwg",
        },
        blender: ToolCheck {
            id: "blender",
            name: "Blender",
            available: false,
            path: None,
            variant: None,
            install_steps: "install blender",
        },
    }
}

fn write_fake_dwg2dxf(dir: &Path) -> PathBuf {
    let dxf_body = "  0\nSECTION\n  2\nHEADER\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n  0\nTEXT\n  8\nWalls\n  1\nBeam Span 6m\n  0\nENDSEC\n  0\nEOF\n";
    let sample = dir.join("sample.dxf");
    fs::write(&sample, dxf_body).unwrap();
    if cfg!(windows) {
        let script = dir.join("dwg2dxf.cmd");
        fs::write(
            &script,
            format!(
                "@echo off\r\ncopy /Y \"{}\" \"%~2\" >nul\r\n",
                sample.display()
            ),
        )
        .unwrap();
        script
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let script = dir.join("dwg2dxf");
            fs::write(
                &script,
                format!("#!/bin/sh\ncp \"{}\" \"$2\"\n", sample.display()),
            )
            .unwrap();
            let mut perms = fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script, perms).unwrap();
            script
        }
        #[cfg(not(unix))]
        unreachable!()
    }
}

fn write_fake_blender(dir: &Path) -> PathBuf {
    let scene_json = r#"{"scene":"Studio","objects":[{"name":"GearMesh","type":"MESH","materials":["BrassAlloy"],"modifiers":["BEVEL"],"vertices":24,"polygons":12}],"materials":["BrassAlloy"]}"#;
    let payload = dir.join("scene_payload.json");
    fs::write(&payload, scene_json).unwrap();
    if cfg!(windows) {
        let script = dir.join("blender.cmd");
        // Args: --background <blend> --python <script> -- <out_json>
        fs::write(
            &script,
            format!(
                "@echo off\r\ncopy /Y \"{}\" \"%~6\" >nul\r\n",
                payload.display()
            ),
        )
        .unwrap();
        script
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let script = dir.join("blender");
            fs::write(
                &script,
                format!("#!/bin/sh\ncp \"{}\" \"$6\"\n", payload.display()),
            )
            .unwrap();
            let mut perms = fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script, perms).unwrap();
            script
        }
        #[cfg(not(unix))]
        unreachable!()
    }
}

#[test]
fn m2_8_pending_tier2_requeues_when_tools_appear_and_unsupported_stays_failed() {
    let Some(python) = venv_python() else {
        eprintln!("skipped: no .venv; run `uv sync --group dev` first");
        return;
    };
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let ws = Workspace::init(root.path(), data.path()).unwrap();
    fs::create_dir_all(root.path().join("inbox")).unwrap();
    fs::write(root.path().join("inbox/plan.dwg"), b"AC1032\0\0\0\0").unwrap();
    fs::write(root.path().join("inbox/gear.blend"), b"BLENDER-v300\0").unwrap();
    fs::write(
        root.path().join("inbox/building.rvt"),
        b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1rvt",
    )
    .unwrap();
    fs::write(
        root.path().join("inbox/house.skp"),
        b"\xFF\xFE\xFF\x0ES\0k\0",
    )
    .unwrap();

    scan(&ws).unwrap();
    let mut sup = Supervisor::new(WorkerSpec::python(&python), Duration::from_secs(60));

    let first = extract_pending_with_options(
        &ws,
        Some(&mut sup),
        ExtractOptions {
            tools: empty_tools(),
            ..ExtractOptions::default()
        },
    )
    .unwrap();
    let outcome_of = |r: &lucubra_ingest::pipeline::ExtractReport, p: &str| {
        r.processed
            .iter()
            .find(|item| item.path == p)
            .unwrap()
            .outcome
            .clone()
    };
    assert_eq!(
        outcome_of(&first, "inbox/plan.dwg"),
        Outcome::Failed {
            code: "tool_missing".into(),
            outcome: "pending"
        }
    );
    assert_eq!(
        outcome_of(&first, "inbox/gear.blend"),
        Outcome::Failed {
            code: "tool_missing".into(),
            outcome: "pending"
        }
    );
    assert_eq!(
        outcome_of(&first, "inbox/building.rvt"),
        Outcome::Failed {
            code: "unsupported".into(),
            outcome: "failed"
        }
    );
    assert_eq!(
        outcome_of(&first, "inbox/house.skp"),
        Outcome::Failed {
            code: "unsupported".into(),
            outcome: "failed"
        }
    );

    // Install fake tools and re-run without clearing state: pending files re-queue automatically.
    let fake_dwg = write_fake_dwg2dxf(bin.path());
    let fake_blender = write_fake_blender(bin.path());
    let mut installed = empty_tools();
    installed.oda.available = true;
    installed.oda.variant = Some("libredwg");
    installed.oda.path = Some(fake_dwg);
    installed.blender.available = true;
    installed.blender.variant = Some("blender");
    installed.blender.path = Some(fake_blender);

    let second = extract_pending_with_options(
        &ws,
        Some(&mut sup),
        ExtractOptions {
            tools: installed,
            ..ExtractOptions::default()
        },
    )
    .unwrap();
    // The 2 unsupported files remain held; DWG and .blend extract now.
    assert_eq!(second.held, 2);
    assert!(matches!(
        outcome_of(&second, "inbox/plan.dwg"),
        Outcome::Extracted { logged: true, .. }
    ));
    assert!(matches!(
        outcome_of(&second, "inbox/gear.blend"),
        Outcome::Extracted { logged: true, .. }
    ));

    let index = open_index(&ws).unwrap();
    assert_eq!(index.search("Beam Span", 5).unwrap().len(), 1);
    assert_eq!(index.search("BrassAlloy", 5).unwrap().len(), 2);

    let state = ws.replay().unwrap().state;
    let blend_hash = second
        .processed
        .iter()
        .find(|p| p.path == "inbox/gear.blend")
        .unwrap()
        .hash
        .clone();
    assert_eq!(
        state.get(col::FILES, &blend_hash).unwrap()["status"],
        "extracted"
    );
}
