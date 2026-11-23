// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Helpers shared by integration tests that drive the real `lucubra` binary.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::Value;

/// Path of the `lucubra` executable, building it on demand.
pub fn lucubra_binary() -> PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_lucubra") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return pb;
        }
    }
    // The CLI belongs to another crate, so `cargo test --workspace` may not have built it and
    // the target layout varies; ask cargo for the executable path instead of guessing.
    static BIN: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BIN.get_or_init(|| {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let exe = std::env::current_exe().expect("locate current test executable");
        let release = exe.components().any(|c| c.as_os_str() == "release");
        let mut cmd = Command::new(cargo);
        cmd.args(["build", "-p", "lucubra-cli", "--message-format=json"]);
        if release {
            cmd.arg("--release");
        }
        let out = cmd
            .stderr(Stdio::inherit())
            .output()
            .expect("spawn cargo build -p lucubra-cli");
        assert!(out.status.success(), "cargo build -p lucubra-cli failed");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter(|v| v["reason"] == "compiler-artifact" && v["target"]["name"] == "lucubra")
            .filter_map(|v| v["executable"].as_str().map(PathBuf::from))
            .next_back()
            .expect("cargo reported no lucubra executable")
    })
    .clone()
}
