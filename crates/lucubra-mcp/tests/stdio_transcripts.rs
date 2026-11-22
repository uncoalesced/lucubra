// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Golden JSON-RPC 2.0 stdio transcript tests driving the real `lucubra mcp`
//! child binary over piped stdin and stdout (`M3-7`, `S23`, `S37`).

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

const GOLDEN_JSON: &str = include_str!("fixtures/golden_transcripts.json");

fn lucubra_binary() -> PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_lucubra") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return pb;
        }
    }
    let test_exe = std::env::current_exe().expect("locate current test executable");
    let profile_dir = test_exe
        .parent()
        .and_then(|p| p.parent())
        .expect("deps/../ is cargo profile directory");
    let candidate = profile_dir.join(format!("lucubra{}", std::env::consts::EXE_SUFFIX));
    assert!(
        candidate.is_file(),
        "missing built CLI binary at {}; run `cargo build -p lucubra-cli` first",
        candidate.display()
    );
    candidate
}

struct McpChild {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl McpChild {
    fn spawn(ws: &Path, data: &Path) -> Self {
        let bin = lucubra_binary();
        let mut child = Command::new(&bin)
            .args(["mcp", "-w"])
            .arg(ws)
            .env("LUCUBRA_DATA_DIR", data)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("failed spawning {}: {e}", bin.display()));
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = BufReader::new(child.stdout.take().expect("piped stdout"));
        Self {
            child,
            stdin: Some(stdin),
            stdout,
            next_id: 1,
        }
    }

    fn send_notification(&mut self, method: &str, params: Value) {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        let line = serde_json::to_string(&msg).unwrap();
        let stdin = self.stdin.as_mut().expect("stdin open");
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let line = serde_json::to_string(&msg).unwrap();
        let stdin = self.stdin.as_mut().expect("stdin open");
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();

        loop {
            let mut raw = String::new();
            let n = self.stdout.read_line(&mut raw).expect("read stdout line");
            assert!(n > 0, "EOF before receiving response for id={id}");
            let trimmed = raw.trim();
            assert!(!trimmed.is_empty(), "empty line on stdout");
            let val: Value = serde_json::from_str(trimmed)
                .unwrap_or_else(|e| panic!("non-JSON-RPC line on stdout: {e}; raw={trimmed}"));
            assert_eq!(val["jsonrpc"], "2.0", "bad jsonrpc frame: {val}");
            if val.get("id") == Some(&json!(id)) {
                return val;
            }
        }
    }

    fn call_tool(&mut self, name: &str, args: Value) -> (bool, Value) {
        let resp = self.request(
            "tools/call",
            json!({
                "name": name,
                "arguments": args,
            }),
        );
        assert!(
            resp.get("error").is_none(),
            "unexpected JSON-RPC error for {name}: {resp}"
        );
        let result = &resp["result"];
        let is_error = result
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let content = result["content"].as_array().expect("content array");
        assert_eq!(content.len(), 1);
        let text = content[0]["text"].as_str().expect("text content");
        let env: Value = serde_json::from_str(text)
            .unwrap_or_else(|e| panic!("non-JSON envelope text: {e}; text={text}"));
        assert_eq!(env["ok"].as_bool(), Some(!is_error), "env={env}");
        (is_error, env)
    }

    fn close_and_wait(mut self) -> (Option<i32>, String) {
        drop(self.stdin.take());
        let output = self.child.wait_with_output().expect("wait child");
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        (output.status.code(), stderr)
    }
}

fn init_cli_workspace(ws: &Path, data: &Path) {
    let bin = lucubra_binary();
    let out = Command::new(&bin)
        .args(["init", "-w"])
        .arg(ws)
        .env("LUCUBRA_DATA_DIR", data)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "init failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn uninitialized_workspace_exits_nonzero_with_clean_stdout() {
    let tmp = tempfile::tempdir().unwrap();
    let missing_ws = tmp.path().join("not-initialized");
    fs::create_dir_all(&missing_ws).unwrap();
    let data = tmp.path().join("data");

    let bin = lucubra_binary();
    let out = Command::new(&bin)
        .args(["mcp", "-w"])
        .arg(&missing_ws)
        .env("LUCUBRA_DATA_DIR", &data)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty(), "stdout must be empty on error");
    assert!(!out.stderr.is_empty(), "stderr should explain failure");
}

#[test]
fn golden_transcript_1_lifecycle_study() {
    let golden: Value = serde_json::from_str(GOLDEN_JSON).unwrap();
    let spec = &golden["transcripts"]["lifecycle_study"];
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    let data = tmp.path().join("data");
    init_cli_workspace(&ws, &data);

    for f in spec["fixture_files"].as_array().unwrap() {
        let p = ws.join(f["path"].as_str().unwrap());
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, f["content"].as_str().unwrap()).unwrap();
    }

    let mut proc = McpChild::spawn(&ws, &data);
    let mut captured_ref: Option<String> = None;
    let mut captured_sid: Option<String> = None;

    for step in spec["steps"].as_array().unwrap() {
        match step["method"].as_str().unwrap() {
            "initialize" => {
                let resp = proc.request(
                    "initialize",
                    json!({
                        "protocolVersion": golden["protocol_version"],
                        "capabilities": {},
                        "clientInfo": spec["client_info"],
                    }),
                );
                assert_eq!(resp["result"]["serverInfo"]["name"], step["expect_server"]);
                assert_eq!(resp["result"]["protocolVersion"], step["expect_protocol"]);
            }
            "notifications/initialized" => {
                proc.send_notification("notifications/initialized", json!({}));
            }
            "tools/list" => {
                let resp = proc.request("tools/list", json!({}));
                let tools = resp["result"]["tools"].as_array().unwrap();
                assert_eq!(tools.len(), step["expect_count"].as_u64().unwrap() as usize);
                let prefix = step["expect_prefix"].as_str().unwrap();
                for t in tools {
                    assert!(t["name"].as_str().unwrap().starts_with(prefix));
                }
            }
            "prompts/list" => {
                let resp = proc.request("prompts/list", json!({}));
                let got: Vec<&str> = resp["result"]["prompts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| p["name"].as_str().unwrap())
                    .collect();
                let want: Vec<&str> = step["expect_names"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .collect();
                assert_eq!(got, want);
            }
            "tools/call" => {
                let tool = step["tool"].as_str().unwrap();
                let args = if step.get("args_from_captured_ref").is_some() {
                    let mut obj = step["args_from_captured_ref"].clone();
                    obj["ref"] = Value::String(captured_ref.clone().expect("captured ref"));
                    obj
                } else {
                    step["args"].clone()
                };
                let (err, env) = proc.call_tool(tool, args);
                assert_eq!(
                    !err,
                    step["expect_ok"].as_bool().unwrap(),
                    "tool={tool} env={env}"
                );

                if let Some(host_id) = step["expect_host_id"].as_str() {
                    assert_eq!(env["data"]["host"]["id"], host_id);
                }
                if step["capture_first_hit_ref"].as_bool() == Some(true) {
                    let first_ref = env["data"]["hits"][0]["ref"].as_str().unwrap().to_owned();
                    assert!(first_ref.starts_with("F:"));
                    captured_ref = Some(first_ref);
                }
                if let Some(trust) = step["expect_trust"].as_str() {
                    assert_eq!(env["data"]["material"]["trust"], trust);
                }
                if step["capture_session_id"].as_bool() == Some(true) {
                    let sid = env["data"]["session_id"].as_str().unwrap().to_owned();
                    assert!(!sid.is_empty());
                    captured_sid = Some(sid);
                }
                if let Some(mins) = step["expect_logged_minutes"].as_u64() {
                    assert_eq!(env["data"]["logged_minutes_total"], mins);
                }
            }
            other => panic!("unexpected step method {other}"),
        }
    }

    assert!(captured_sid.is_some());
    let (code, stderr) = proc.close_and_wait();
    assert_eq!(code, Some(0), "stderr={stderr}");
    assert!(!stderr.contains("panic"), "stderr={stderr}");
}

#[test]
fn golden_transcript_2_s23_error_and_recovery() {
    let golden: Value = serde_json::from_str(GOLDEN_JSON).unwrap();
    let spec = &golden["transcripts"]["s23_error_and_recovery"];
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    let data = tmp.path().join("data");
    init_cli_workspace(&ws, &data);

    let mut proc = McpChild::spawn(&ws, &data);
    for step in spec["steps"].as_array().unwrap() {
        match step["method"].as_str().unwrap() {
            "initialize" => {
                let resp = proc.request(
                    "initialize",
                    json!({
                        "protocolVersion": golden["protocol_version"],
                        "capabilities": {},
                        "clientInfo": spec["client_info"],
                    }),
                );
                assert_eq!(resp["result"]["serverInfo"]["name"], step["expect_server"]);
            }
            "notifications/initialized" => {
                proc.send_notification("notifications/initialized", json!({}));
            }
            "tools/call" => {
                let tool = step["tool"].as_str().unwrap();
                let (err, env) = proc.call_tool(tool, step["args"].clone());
                assert_eq!(!err, step["expect_ok"].as_bool().unwrap(), "env={env}");
                if !step["expect_ok"].as_bool().unwrap() {
                    assert_eq!(env["code"], step["expect_code"]);
                    let fix = env["fix"].as_str().unwrap();
                    assert!(!fix.contains('\n'), "multi-line fix: {fix}");
                    if let Some(sub) = step["expect_fix_contains"].as_str() {
                        assert!(fix.contains(sub), "fix={fix}");
                    }
                }
            }
            other => panic!("unexpected method {other}"),
        }
    }

    let (code, stderr) = proc.close_and_wait();
    assert_eq!(code, Some(0), "stderr={stderr}");
}

#[test]
fn golden_transcript_3_s37_local_budget_exhaustion() {
    let golden: Value = serde_json::from_str(GOLDEN_JSON).unwrap();
    let spec = &golden["transcripts"]["s37_local_budget_exhaustion"];
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    let data = tmp.path().join("data");
    init_cli_workspace(&ws, &data);

    let rel = spec["long_fixture_rel"].as_str().unwrap();
    let p = ws.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    let repeat = spec["long_fixture_repeat"].as_u64().unwrap() as usize;
    let body = format!(
        "# Thermodynamics\n\n{}",
        spec["long_fixture_unit_text"]
            .as_str()
            .unwrap()
            .repeat(repeat)
    );
    fs::write(&p, &body).unwrap();
    // Ingest caps chunks at 2,000 chars, so oversized reads go through AGENTS.md (`index`).
    fs::write(ws.join("AGENTS.md"), &body).unwrap();

    let mut proc = McpChild::spawn(&ws, &data);
    let resp = proc.request(
        "initialize",
        json!({
            "protocolVersion": golden["protocol_version"],
            "capabilities": {},
            "clientInfo": spec["client_info"],
        }),
    );
    assert_eq!(resp["result"]["serverInfo"]["name"], "lucubra");
    proc.send_notification("notifications/initialized", json!({}));

    let (err, scan_env) = proc.call_tool("lucubra_ingest", json!({ "action": "scan" }));
    assert!(!err, "{scan_env}");

    let (err, start_env) = proc.call_tool(
        "lucubra_session",
        json!({ "action": "start", "units": ["thermo/ch1"] }),
    );
    assert!(!err, "{start_env}");

    let expected_limit = spec["expect_local_limit"].as_u64().unwrap();
    let mut wrapped = false;
    for _ in 0..18 {
        let (err, env) = proc.call_tool("lucubra_read", json!({ "action": "index" }));
        assert!(!err, "{env}");
        assert_eq!(env["budget"]["limit"], expected_limit);
        if env["budget"]["served"].as_u64().unwrap() >= expected_limit {
            assert_eq!(env["next"], "wrap_up");
            assert!(!env["warnings"].as_array().unwrap().is_empty());
            wrapped = true;
            break;
        }
    }
    assert!(wrapped, "Local tier stdio session never hit wrap_up limit");

    let (err, end_env) = proc.call_tool(
        "lucubra_session",
        json!({
            "action": "end",
            "reason": "budget",
            "summary": "Wrapped up cleanly after reaching LM Studio local token budget."
        }),
    );
    assert!(!err, "{end_env}");
    assert_ne!(end_env["next"], "wrap_up");

    let (code, stderr) = proc.close_and_wait();
    assert_eq!(code, Some(0), "stderr={stderr}");
}
