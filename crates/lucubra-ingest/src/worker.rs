// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Worker supervisor (SPEC section 4.2, ADR 0011 sections 3 and 4).
//!
//! A worker is a long-lived subprocess speaking JSON lines over stdio. The
//! supervisor sends one request per file and waits for the matching response
//! up to a deadline. A worker that passes the deadline is killed; a worker that
//! exits is noted. Either way the next request starts a fresh worker, so one
//! bad file never blocks the queue.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// Default per-request timeout (SPEC section 4.2).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// How to start a worker.
#[derive(Debug, Clone)]
pub struct WorkerSpec {
    /// Program to run.
    pub program: PathBuf,
    /// Arguments.
    pub args: Vec<OsString>,
    /// Extra environment variables.
    pub envs: Vec<(OsString, OsString)>,
}

impl WorkerSpec {
    /// The Python workers: `<python> -I -m lucubra_workers`.
    pub fn python(python: impl Into<PathBuf>) -> WorkerSpec {
        WorkerSpec {
            program: python.into(),
            args: vec!["-I".into(), "-m".into(), "lucubra_workers".into()],
            envs: vec![
                ("PYTHONIOENCODING".into(), "utf-8".into()),
                ("LUCUBRA_WORKER_MEM_MB".into(), "2048".into()),
            ],
        }
    }
}

/// Environment variable naming the Python interpreter for the workers.
pub const PYTHON_ENV: &str = "LUCUBRA_PYTHON";

/// Finds the Python that runs `lucubra_workers`: `LUCUBRA_PYTHON` if set,
/// otherwise the interpreter installed next to the `lucubra` binary (the
/// layout of a wheel install in a virtual environment or a `uv tool`).
pub fn find_python() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(PYTHON_ENV) {
        return Some(PathBuf::from(path));
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let names: &[&str] = if cfg!(windows) {
        &["python.exe"]
    } else {
        &["python3", "python"]
    };
    names
        .iter()
        .map(|name| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// Starts the Python workers and checks they answer a ping.
pub fn python_supervisor(timeout: Duration) -> std::result::Result<Supervisor, String> {
    let python = find_python().ok_or_else(|| {
        format!("no Python found next to lucubra; set {PYTHON_ENV} to the interpreter")
    })?;
    let mut sup = Supervisor::new(WorkerSpec::python(&python), timeout);
    match sup.request("ping", Path::new(""), json!({})) {
        Reply::Ok(_) => Ok(sup),
        Reply::Failed { code, detail } => Err(format!(
            "worker at {} did not answer a ping ({code}: {detail})",
            python.display()
        )),
    }
}

/// The result of one request.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    /// `ok: true`; the full response object.
    Ok(Value),
    /// A failure with an ADR 0011 code.
    Failed {
        /// Failure code, such as `timeout` or `worker_crashed`.
        code: String,
        /// Human-readable reason.
        detail: String,
    },
}

impl Reply {
    fn failed(code: &str, detail: impl Into<String>) -> Reply {
        Reply::Failed {
            code: code.to_owned(),
            detail: detail.into(),
        }
    }
}

struct Running {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
}

impl Running {
    fn stop(mut self) -> String {
        drop(self.stdin);
        let _ = self.child.kill();
        match self.child.wait() {
            Ok(status) => status.to_string(),
            Err(e) => e.to_string(),
        }
    }
}

/// Runs requests against one worker process at a time.
pub struct Supervisor {
    spec: WorkerSpec,
    timeout: Duration,
    running: Option<Running>,
    next_id: u64,
    /// Workers started so far, including restarts.
    pub starts: u32,
}

impl Supervisor {
    /// A supervisor that starts workers lazily.
    pub fn new(spec: WorkerSpec, timeout: Duration) -> Supervisor {
        Supervisor {
            spec,
            timeout,
            running: None,
            next_id: 0,
            starts: 0,
        }
    }

    fn start(&mut self) -> std::io::Result<&mut Running> {
        if self.running.is_none() {
            let mut child = Command::new(&self.spec.program)
                .args(&self.spec.args)
                .envs(self.spec.envs.iter().map(|(k, v)| (k, v)))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()?;
            let stdin = child.stdin.take().expect("stdin is piped");
            let stdout = child.stdout.take().expect("stdout is piped");
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            });
            self.starts += 1;
            self.running = Some(Running {
                child,
                stdin,
                lines: rx,
            });
        }
        Ok(self.running.as_mut().expect("just started"))
    }

    /// Sends one request and waits for its response.
    pub fn request(&mut self, op: &str, path: &Path, opts: Value) -> Reply {
        self.next_id += 1;
        let id = format!("r{}", self.next_id);
        let line = json!({"id": id, "op": op, "path": path.to_string_lossy(), "opts": opts});
        let timeout = self.timeout;
        let running = match self.start() {
            Ok(running) => running,
            Err(e) => return Reply::failed("worker_crashed", format!("cannot start worker: {e}")),
        };
        let sent = writeln!(running.stdin, "{line}").and_then(|()| running.stdin.flush());
        if let Err(e) = sent {
            let status = self.running.take().map(Running::stop).unwrap_or_default();
            return Reply::failed("worker_crashed", format!("write failed: {e} ({status})"));
        }
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let running = self.running.as_mut().expect("worker is running");
            match running.lines.recv_timeout(left) {
                Ok(text) => {
                    // Lines that are not this request's response (stray library
                    // output, a stale reply) are ignored.
                    let Ok(Value::Object(response)) = serde_json::from_str::<Value>(&text) else {
                        continue;
                    };
                    if response.get("id").and_then(Value::as_str) != Some(id.as_str()) {
                        continue;
                    }
                    if response.get("ok").and_then(Value::as_bool) == Some(true) {
                        return Reply::Ok(Value::Object(response));
                    }
                    let error = response.get("error");
                    let field = |name: &str| {
                        error
                            .and_then(|e| e.get(name))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned()
                    };
                    let code = field("code");
                    let code = if code.is_empty() {
                        "corrupt".into()
                    } else {
                        code
                    };
                    return Reply::Failed {
                        code,
                        detail: field("detail"),
                    };
                }
                Err(RecvTimeoutError::Timeout) => {
                    self.running.take().map(Running::stop);
                    return Reply::failed(
                        "timeout",
                        format!(
                            "no response within {} s; worker restarted",
                            timeout.as_secs_f32()
                        ),
                    );
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let status = self.running.take().map(Running::stop).unwrap_or_default();
                    return Reply::failed(
                        "worker_crashed",
                        format!("worker exited mid-request ({status}); restarted"),
                    );
                }
            }
        }
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        if let Some(running) = self.running.take() {
            running.stop();
        }
    }
}
