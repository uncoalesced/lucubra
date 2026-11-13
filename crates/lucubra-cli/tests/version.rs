// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Runs the built `lucubra` binary.

use std::process::Command;

fn lucubra(args: &[&str]) -> (Option<i32>, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_lucubra"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

#[test]
fn version_flag_prints_the_workspace_version() {
    let expected = format!("lucubra {}\n", env!("CARGO_PKG_VERSION"));
    for flag in ["--version", "-V"] {
        let (code, stdout, _) = lucubra(&[flag]);
        assert_eq!(code, Some(0));
        assert_eq!(stdout.replace("\r\n", "\n"), expected);
    }
}

#[test]
fn unknown_argument_exits_with_status_two() {
    let (code, stdout, stderr) = lucubra(&["no-such-command"]);
    assert_eq!(code, Some(2));
    assert!(stdout.is_empty());
    assert!(stderr.contains("unknown argument 'no-such-command'"));
}
