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
    /// Check external tools (Python worker, ODA/LibreDWG, Blender) and retry
    /// pending files in the workspace when a tool is found.
    Doctor(Target),
    /// Save an unverified (or verified) transcript for a vision page or image.
    Transcript {
        #[command(flatten)]
        target: Target,
        /// Full BLAKE3 hash or unambiguous prefix of the source file.
        #[arg(long)]
        hash: String,
        /// 1-based page number (omit for standalone images).
        #[arg(long)]
        page: Option<u32>,
        /// Transcript Markdown text.
        #[arg(long, required_unless_present = "file")]
        text: Option<String>,
        /// Read transcript Markdown text from a file.
        #[arg(long)]
        file: Option<PathBuf>,
        /// Mark the transcript as verified instead of unverified.
        #[arg(long)]
        verified: bool,
    },
    /// Watch the workspace and scan + extract changed files.
    Watch(Target),
    /// Start the MCP server over stdio.
    Mcp(Target),
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

fn run_extract(
    ws: &Workspace,
    tools: lucubra_ingest::doctor::ToolStatus,
) -> lucubra_core::Result<()> {
    use lucubra_ingest::pipeline::{
        ExtractOptions, Outcome, Progress, extract_pending_with_options,
    };
    let mut worker =
        match lucubra_ingest::worker::python_supervisor(lucubra_ingest::worker::DEFAULT_TIMEOUT) {
            Ok(sup) => Some(sup),
            Err(reason) => {
                println!("worker unavailable: {reason}");
                None
            }
        };
    let mut progress = |p: &Progress<'_>| {
        if p.total <= 50 || p.done == p.total || p.done.is_multiple_of(50) {
            match p.outcome {
                Outcome::Extracted { chunks, .. } => {
                    println!(
                        "  [{}/{}] extracted {} [{}]: {chunks} chunks",
                        p.done,
                        p.total,
                        p.path,
                        p.format.as_str()
                    );
                }
                Outcome::Failed { code, outcome } => {
                    println!(
                        "  [{}/{}] {outcome} {} [{}]: {code}",
                        p.done,
                        p.total,
                        p.path,
                        p.format.as_str()
                    );
                }
                Outcome::NoWorker => {}
            }
        }
    };
    let opts = ExtractOptions {
        tools,
        on_progress: Some(&mut progress),
        ..ExtractOptions::default()
    };
    let report = extract_pending_with_options(ws, worker.as_mut(), opts)?;
    let waiting = report
        .processed
        .iter()
        .filter(|p| matches!(p.outcome, Outcome::NoWorker))
        .count();
    println!(
        "extraction: {} done now, {} already indexed, {} failed or pending earlier",
        report.processed.len() - waiting,
        report.up_to_date,
        report.held
    );
    if report.vision_rendered > 0 || report.vision_skipped > 0 {
        println!(
            "vision: {} pages rendered, {} already cached",
            report.vision_rendered, report.vision_skipped
        );
    }
    if waiting > 0 {
        println!(
            "waiting for a Python worker: {waiting} files; they are extracted on the next ingest with a worker"
        );
    }
    for path in &report.missing {
        println!("warning: no copy on disk: {path}");
    }
    Ok(())
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
            let ws = Workspace::open(&workspace_path(&target), &data_dir)?;
            print_scan(&lucubra_ingest::scan(&ws)?);
            run_extract(&ws, lucubra_ingest::doctor::check_tools())?;
        }
        Command::Doctor(target) => {
            let status = lucubra_ingest::doctor::check_tools();
            for tool in status.all() {
                if let Some(path) = &tool.path {
                    let kind = tool.variant.map(|v| format!(" [{v}]")).unwrap_or_default();
                    println!("ok       {}{} -> {}", tool.id, kind, path.display());
                } else {
                    println!("missing  {} ({})", tool.id, tool.name);
                    println!("         install: {}", tool.install_steps);
                }
            }
            let ws_root = workspace_path(&target);
            if target.workspace.is_some() || ws_root.join(".lucubra").join("VERSION").is_file() {
                let ws = Workspace::open(&ws_root, &data_dir)?;
                run_extract(&ws, status)?;
            }
        }
        Command::Transcript {
            target,
            hash,
            page,
            text,
            file,
            verified,
        } => {
            let ws = Workspace::open(&workspace_path(&target), &data_dir)?;
            let body = match (text, file) {
                (Some(t), _) => t,
                (None, Some(path)) => std::fs::read_to_string(&path).map_err(|e| {
                    lucubra_core::Error::io(format!("reading {}", path.display()), e)
                })?,
                (None, None) => {
                    return Err(lucubra_core::Error::Invalid(
                        "provide --text or --file".into(),
                    ));
                }
            };
            let saved = lucubra_ingest::vision::save_transcript(&ws, &hash, page, &body, verified)?;
            println!("transcript saved: {}", saved.display());
        }
        Command::Watch(target) => {
            let ws = Workspace::open(&workspace_path(&target), &data_dir)?;
            let watcher = lucubra_ingest::watch::WorkspaceWatcher::start(
                ws.root(),
                std::time::Duration::from_millis(300),
            )?;
            println!("watching {} (Ctrl-C to stop)", ws.root().display());
            loop {
                let batch = watcher.next_batch(std::time::Duration::from_secs(60));
                if !batch.is_empty() {
                    println!("changed: {} file(s)", batch.len());
                    print_scan(&lucubra_ingest::scan(&ws)?);
                    run_extract(&ws, lucubra_ingest::doctor::check_tools())?;
                }
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
        Command::Mcp(target) => {
            init_mcp_stderr_tracing();
            let ws_root = workspace_path(&target);
            let options = lucubra_mcp::ServerOptions {
                workspace_root: ws_root,
                data_dir,
                read_only_tools: false,
            };
            let server = lucubra_mcp::LucubraServer::open(&options)?;
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|e| lucubra_core::Error::io("building async runtime", e))?;
            let ct = tokio_util::sync::CancellationToken::new();
            let signal_ct = ct.clone();
            rt.block_on(async move {
                tokio::spawn(async move {
                    if tokio::signal::ctrl_c().await.is_ok() {
                        signal_ct.cancel();
                    }
                });
                lucubra_mcp::serve_stdio(server, ct).await
            })
            .map_err(|e| lucubra_core::Error::Invalid(format!("MCP server stopped: {e}")))?;
        }
    }
    Ok(())
}

fn init_mcp_stderr_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_env("LUCUBRA_LOG")
        .or_else(|_| EnvFilter::try_from_default_env())
        .unwrap_or_else(|_| EnvFilter::new("warn"));
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(filter)
        .try_init();
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
