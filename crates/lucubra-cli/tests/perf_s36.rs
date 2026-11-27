// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Scenario S36: a large workspace. The initial ingest survives an interrupt
//! and resumes, and `lucubra_orient` answers in under 200 ms.
//!
//! Ignored by default: it needs a generated corpus outside the repository.
//! Generate one, then run with `--release` so the timing reflects a real build:
//!
//! ```text
//! uv run --no-project python scripts/fixtures/gen_scale_corpus.py \
//!     --out <ws>/inbox --count 3000 --target-bytes 300000000
//! LUCUBRA_PERF_WS=<ws> LUCUBRA_PERF_DATA=<data> \
//!     cargo test --release -p lucubra-cli --test perf_s36 -- --ignored --nocapture
//! ```

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn env_path(name: &str) -> PathBuf {
    PathBuf::from(std::env::var(name).unwrap_or_else(|_| panic!("set {name}")))
}

fn lucubra(ws: &str, data: &PathBuf, args: &[&str]) -> (Option<i32>, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_lucubra"))
        .args(args)
        .args(["-w", ws])
        .env("LUCUBRA_DATA_DIR", data)
        .output()
        .unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[test]
#[ignore = "needs a generated corpus; see the module docs"]
fn s36_large_workspace_resumes_after_an_interrupt_and_orient_is_fast() {
    let ws_path = env_path("LUCUBRA_PERF_WS");
    let data = env_path("LUCUBRA_PERF_DATA");
    let ws = ws_path.to_str().unwrap();

    // 1. Start the initial ingest and kill it part-way.
    let mut child = Command::new(env!("CARGO_BIN_EXE_lucubra"))
        .args(["ingest", "-w", ws])
        .env("LUCUBRA_DATA_DIR", &data)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_secs(4));
    let interrupted = child.try_wait().unwrap().is_none();
    child.kill().ok();
    child.wait().unwrap();
    println!("killed during the first ingest: {interrupted}");

    // 2. Run it again: it resumes and finishes.
    let resume = Instant::now();
    let (code, out) = lucubra(ws, &data, &["ingest"]);
    println!("resumed ingest finished in {:?}", resume.elapsed());
    assert_eq!(code, Some(0), "{out}");
    for line in out.lines().filter(|l| !l.starts_with("  ")) {
        println!("{line}");
    }

    // 3. A third run has nothing left to do.
    let (code, again) = lucubra(ws, &data, &["ingest"]);
    assert_eq!(code, Some(0));
    assert!(again.contains("0 new or changed"), "{again}");

    // 4. Orient in-process: one warm-up call, then the median of five.
    let workspace = lucubra_core::Workspace::open(&ws_path, &data).unwrap();
    let server = lucubra_mcp::LucubraServer::from_workspace(workspace, false);
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut times = Vec::new();
    for i in 0..6 {
        let t = Instant::now();
        let res =
            rt.block_on(server.handle_call_raw("lucubra_orient", Some(serde_json::Map::new())));
        let took = t.elapsed();
        assert!(res.is_ok(), "orient failed");
        if i > 0 {
            times.push(took);
        }
    }
    times.sort();
    let median = times[times.len() / 2];
    println!("orient median {median:?} over {times:?}");
    assert!(
        median < Duration::from_millis(200),
        "orient took {median:?}"
    );
}
