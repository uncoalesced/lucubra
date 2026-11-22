// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Worker supervisor tests (GATES M2-3). The worker is this test binary
//! re-run as `fake_worker_child`, so no Python is needed.

use std::io::{BufRead, Write};
use std::path::Path;
use std::time::Duration;

use lucubra_ingest::worker::{Reply, Supervisor, WorkerSpec};
use serde_json::{Value, json};

const CHILD_ENV: &str = "LUCUBRA_FAKE_WORKER";

/// The fake worker. Behaviour depends on the request path:
/// `sleep` never answers, `crash` exits, `fail` returns an error,
/// `noise` prints a stray line first, anything else succeeds.
#[test]
#[ignore = "run as a child process by the worker tests"]
fn fake_worker_child() {
    if std::env::var_os(CHILD_ENV).is_none() {
        return;
    }
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    // libtest has already printed "test fake_worker_child ... " with no
    // newline; end that line so the first response stands alone.
    writeln!(out).unwrap();
    out.flush().unwrap();
    for line in stdin.lock().lines() {
        let request: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let id = request["id"].clone();
        let path = request["path"].as_str().unwrap_or_default().to_owned();
        let response = match path.as_str() {
            "sleep" => {
                std::thread::sleep(Duration::from_secs(60));
                continue;
            }
            "crash" => std::process::exit(3),
            "fail" => {
                json!({"id": id, "ok": false, "error": {"code": "corrupt", "detail": "bad xref"}})
            }
            "noise" => {
                writeln!(out, "warning from some library").unwrap();
                json!({"id": id, "ok": true, "kind": "txt", "chunks": []})
            }
            _ => json!({"id": id, "ok": true, "kind": "txt", "chunks": [path]}),
        };
        writeln!(out, "{response}").unwrap();
        out.flush().unwrap();
    }
}

fn supervisor(timeout: Duration) -> Supervisor {
    let spec = WorkerSpec {
        program: std::env::current_exe().unwrap(),
        args: vec![
            "fake_worker_child".into(),
            "--exact".into(),
            "--ignored".into(),
            "--nocapture".into(),
            "--test-threads=1".into(),
        ],
        envs: vec![(CHILD_ENV.into(), "1".into())],
    };
    Supervisor::new(spec, timeout)
}

fn ok_chunks(reply: &Reply) -> Value {
    match reply {
        Reply::Ok(v) => v["chunks"].clone(),
        other => panic!("expected ok, got {other:?}"),
    }
}

fn code(reply: &Reply) -> &str {
    match reply {
        Reply::Failed { code, .. } => code,
        other => panic!("expected failure, got {other:?}"),
    }
}

#[test]
fn worker_answers_requests_on_one_process() {
    let mut sup = supervisor(Duration::from_secs(20));
    let a = sup.request("extract", Path::new("a.txt"), json!({}));
    let b = sup.request("extract", Path::new("b.txt"), json!({}));
    assert_eq!(ok_chunks(&a), json!(["a.txt"]));
    assert_eq!(ok_chunks(&b), json!(["b.txt"]));
    assert_eq!(sup.starts, 1);
}

#[test]
fn worker_timeout_is_killed_and_queue_continues() {
    let mut sup = supervisor(Duration::from_secs(2));
    // Warm up so the timeout measures the request, not process start.
    ok_chunks(&sup.request("extract", Path::new("warm"), json!({})));
    let slow = sup.request("extract", Path::new("sleep"), json!({}));
    assert_eq!(code(&slow), "timeout");
    let next = sup.request("extract", Path::new("next.txt"), json!({}));
    assert_eq!(ok_chunks(&next), json!(["next.txt"]));
    assert_eq!(sup.starts, 2);
}

#[test]
fn worker_crash_is_reported_and_restarted() {
    let mut sup = supervisor(Duration::from_secs(20));
    let crashed = sup.request("extract", Path::new("crash"), json!({}));
    assert_eq!(code(&crashed), "worker_crashed");
    let next = sup.request("extract", Path::new("next.txt"), json!({}));
    assert_eq!(ok_chunks(&next), json!(["next.txt"]));
    assert_eq!(sup.starts, 2);
}

#[test]
fn worker_error_replies_and_stray_output() {
    let mut sup = supervisor(Duration::from_secs(20));
    let failed = sup.request("extract", Path::new("fail"), json!({}));
    assert_eq!(
        failed,
        Reply::Failed {
            code: "corrupt".into(),
            detail: "bad xref".into()
        }
    );
    let noisy = sup.request("extract", Path::new("noise"), json!({}));
    assert_eq!(ok_chunks(&noisy), json!([]));
    assert_eq!(sup.starts, 1, "an error reply does not restart the worker");
}

#[test]
fn worker_missing_program_is_a_failure_not_a_panic() {
    let spec = WorkerSpec {
        program: "E:/definitely/not/here/python.exe".into(),
        args: vec![],
        envs: vec![],
    };
    let mut sup = Supervisor::new(spec, Duration::from_secs(1));
    let reply = sup.request("ping", Path::new(""), json!({}));
    assert_eq!(code(&reply), "worker_crashed");
}
