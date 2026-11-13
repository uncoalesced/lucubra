// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! The `lucubra` binary.

use std::process::ExitCode;

const USAGE: &str = "\
Usage: lucubra [OPTION]

Options:
  -V, --version  Print the version
  -h, --help     Print this help";

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("--version" | "-V") => {
            println!("lucubra {}", lucubra_core::VERSION);
            ExitCode::SUCCESS
        }
        Some("--help" | "-h") | None => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("lucubra: unknown argument '{other}'\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}
