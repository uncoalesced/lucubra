// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! The `lucubra` binary.

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::error::{ContextKind, ErrorKind};
use clap::{Args, CommandFactory, Parser, Subcommand};
use lucubra_core::projection::ProjectReport;
use lucubra_core::{Workspace, appdata};

/// Local-first study engine.
#[derive(Parser)]
#[command(name = "lucubra", version = lucubra_core::VERSION, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Args)]
struct Target {
    /// Workspace folder (default: the current folder).
    #[arg(long, short = 'w', value_name = "PATH")]
    workspace: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// Create the workspace layout and its first event, then write the indexes.
    Init(Target),
    /// Delete this workspace's state cache, rebuild it from the event logs and
    /// print the state hash.
    Rebuild(Target),
    /// Regenerate AGENTS.md, subject indexes and unit cards.
    Project(Target),
    /// Clear derived state: the cache and unedited generated files. Event logs,
    /// sources, notes and preserved edits are kept.
    Reset {
        #[command(flatten)]
        target: Target,
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Show event counts and warnings.
    Status(Target),
    /// Find new and changed files, hash them and record them. Online-only
    /// cloud files are counted, never read.
    Scan(Target),
    /// Scan, then extract and index every file not yet in this device's index.
    Ingest(Target),
    /// Search extracted text.
    Search {
        #[command(flatten)]
        target: Target,
        /// Words to find.
        #[arg(required = true)]
        query: Vec<String>,
        /// Match inside words (three or more characters per term).
        #[arg(long)]
        substring: bool,
        /// Maximum results.
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
}

fn print_scan(report: &lucubra_ingest::ScanReport) {
    println!(
        "files: {} new or changed, {} unchanged",
        report.logged(),
        report.unchanged
    );
    for file in report.hashed.iter().filter(|f| f.logged) {
        println!(
            "  {} [{}] {}",
            file.rel,
            file.format.as_str(),
            &file.hash[..8]
        );
    }
    let (count, bytes) = (report.online_only.len(), report.online_only_bytes());
    if count > 0 {
        println!(
            "online-only: {count} files, {bytes} bytes, not downloaded (reading them would download them)"
        );
    }
    for problem in &report.problems {
        println!("warning: {problem}");
    }
}

fn workspace_path(target: &Target) -> PathBuf {
    target
        .workspace
        .clone()
        .unwrap_or_else(|| PathBuf::from("."))
}

fn print_report(report: &ProjectReport) {
    println!(
        "projections: {} written, {} unchanged",
        report.written.len(),
        report.unchanged.len()
    );
    for (path, to) in &report.preserved {
        println!("preserved edit: {path} -> {to}");
    }
    for (from, to) in &report.conflicts_moved {
        println!("conflict copy moved: {from} -> {to}");
    }
    for path in &report.user_conflicts {
        println!("conflict-named file left in place: {path}");
    }
    for warning in &report.warnings {
        println!("warning: {warning}");
    }
}

fn confirm_reset() -> bool {
    print!("Reset clears the state cache and unedited generated files. Type 'reset' to continue: ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).is_ok() && line.trim() == "reset"
}

fn run(command: Command) -> lucubra_core::Result<()> {
    let data_dir = appdata::default_data_dir()?;
    match command {
        Command::Init(target) => {
            let ws = Workspace::init(&workspace_path(&target), &data_dir)?;
            let report = ws.project()?;
            let (_, hash) = ws.refresh()?;
            println!("workspace: {}", ws.root().display());
            println!("device: {}", ws.device());
            print_report(&report);
            println!("state-hash: {hash}");
        }
        Command::Rebuild(target) => {
            let ws = Workspace::open(&workspace_path(&target), &data_dir)?;
            let (refresh, hash) = ws.rebuild()?;
            println!("events applied: {}", refresh.applied);
            println!("events skipped: {}", refresh.skipped.len());
            println!("state-hash: {hash}");
        }
        Command::Project(target) => {
            let ws = Workspace::open(&workspace_path(&target), &data_dir)?;
            print_report(&ws.project()?);
        }
        Command::Reset { target, yes } => {
            let ws = Workspace::open(&workspace_path(&target), &data_dir)?;
            if !yes && !confirm_reset() {
                println!("reset cancelled");
                return Ok(());
            }
            let (hash, report) = ws.reset()?;
            print_report(&report);
            println!("state-hash: {hash}");
        }
        Command::Status(target) => {
            let ws = Workspace::open(&workspace_path(&target), &data_dir)?;
            let status = ws.status()?;
            println!("workspace-id: {}", status.workspace_id);
            println!("device: {}", status.device);
            println!("events: {}", status.events);
            println!("events skipped: {}", status.skipped);
            println!("unreadable lines: {}", status.bad_lines.len());
            for bad in &status.bad_lines {
                let kind = if bad.torn { "torn" } else { "malformed" };
                println!("  {kind}: {} line {}", bad.file.display(), bad.line);
            }
            println!("state-hash: {}", status.state_hash);
        }
        Command::Scan(target) => {
            let ws = Workspace::open(&workspace_path(&target), &data_dir)?;
            print_scan(&lucubra_ingest::scan(&ws)?);
        }
        Command::Ingest(target) => {
            use lucubra_ingest::pipeline::{Outcome, extract_pending};
            let ws = Workspace::open(&workspace_path(&target), &data_dir)?;
            print_scan(&lucubra_ingest::scan(&ws)?);
            let report = extract_pending(&ws, None)?;
            let mut waiting = 0;
            for p in &report.processed {
                match &p.outcome {
                    Outcome::Extracted { chunks, .. } => {
                        println!(
                            "  extracted {} [{}]: {chunks} chunks",
                            p.path,
                            p.format.as_str()
                        );
                    }
                    Outcome::Failed { code, outcome } => {
                        println!("  {outcome} {} [{}]: {code}", p.path, p.format.as_str());
                    }
                    Outcome::NoWorker => waiting += 1,
                }
            }
            println!(
                "extraction: {} done now, {} already indexed, {} failed or pending earlier",
                report.processed.len() - waiting,
                report.up_to_date,
                report.held
            );
            if waiting > 0 {
                println!(
                    "waiting for the Python worker: {waiting} files (PDF, Office, DXF, DICOM)"
                );
            }
            for path in &report.missing {
                println!("warning: no copy on disk: {path}");
            }
        }
        Command::Search {
            target,
            query,
            substring,
            limit,
        } => {
            let ws = Workspace::open(&workspace_path(&target), &data_dir)?;
            let index = lucubra_ingest::pipeline::open_index(&ws)?;
            let query = query.join(" ");
            let hits = if substring {
                index.search_substring(&query, limit)?
            } else {
                index.search(&query, limit)?
            };
            let state = ws.replay()?.state;
            let path_of = |hash: &str| {
                state
                    .collection(lucubra_core::state::col::PATHS)
                    .find(|(_, v)| v.get("hash").and_then(|h| h.as_str()) == Some(hash))
                    .map(|(p, _)| p.clone())
                    .unwrap_or_default()
            };
            println!("results: {}", hits.len());
            for hit in &hits {
                let flat = hit.text.split_whitespace().collect::<Vec<_>>().join(" ");
                let snippet: String = flat.chars().take(120).collect();
                println!(
                    "F:{}/{}  {}",
                    &hit.hash[..8],
                    hit.locator,
                    path_of(&hit.hash)
                );
                println!("    {snippet}");
            }
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            return match err.kind() {
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                    let _ = err.print();
                    ExitCode::SUCCESS
                }
                ErrorKind::InvalidSubcommand | ErrorKind::UnknownArgument => {
                    let arg = err
                        .get(ContextKind::InvalidSubcommand)
                        .or_else(|| err.get(ContextKind::InvalidArg))
                        .map(ToString::to_string)
                        .unwrap_or_default();
                    eprintln!(
                        "lucubra: unknown argument '{arg}'\n\n{}",
                        Cli::command().render_help()
                    );
                    ExitCode::from(2)
                }
                _ => {
                    let _ = err.print();
                    ExitCode::from(2)
                }
            };
        }
    };
    let Some(command) = cli.command else {
        println!("{}", Cli::command().render_help());
        return ExitCode::SUCCESS;
    };
    match run(command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("lucubra: {err}");
            ExitCode::from(1)
        }
    }
}
