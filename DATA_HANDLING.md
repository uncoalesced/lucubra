# Data handling

This document traces the data that Lucubra reads, writes and sends. It covers the paths on disk, the content returned to an AI host, what never leaves the machine, how long each item is kept, how to inspect it and how to delete it.

The claims were checked against the source tree at commit `eb106e0` (branch `main`, 2026-11-23). Each claim names the file that implements it, so you can check it yourself. Where a behaviour depends on a feature that is not yet in the build, the text says so in the section "Planned features". This document is re-verified before each release.

## Summary

- Lucubra does not open network connections. The crates and the Python workers contain no HTTP client, no telemetry, and no update check. The default build opens no listening socket.
- Lucubra runs as a child process of an AI host. The host starts `lucubra mcp` and talks to it over standard input and output. Lucubra never starts a connection of its own.
- The only data that leaves Lucubra is the content of tool responses sent to the host that started it. The host then sends that content to its model provider, unless the model runs on the same machine. That step is governed by the host's terms and is outside Lucubra's control.
- Lucubra keeps its state in two places: the `.lucubra/` folder inside your workspace (plain text event logs and hash records) and one application-data directory on the machine (a device identifier and a rebuildable cache).
- Scanning, extraction, indexing and search do not modify your source files. Lucubra writes new files only in the places listed in section 4.

## 1. What Lucubra does not do

| Behaviour | Status in this build | Source |
|---|---|---|
| Opens outgoing network connections | None. No HTTP client crate in `Cargo.lock` or `uv.lock` | `Cargo.lock`, `uv.lock` |
| Sends telemetry or crash reports | None. No telemetry code path | `crates/`, `python/` |
| Checks for updates | None. No update request (decision D43) | `docs/adr/0002-mcpb-bundle-type.md` |
| Listens on a network port | None in the default build. Streamable HTTP is behind the optional `http-transport` feature, which is off by default and not used by the CLI | `crates/lucubra-mcp/Cargo.toml`, `crates/lucubra-mcp/src/transport.rs`, `docs/adr/0012-mcp-server-surface-and-budgets.md` |
| Downloads web pages or files | Not implemented. The `fetch` action of `lucubra_ingest` returns a NotImplemented error | `crates/lucubra-mcp/src/tools/ingest.rs` |
| Reads files outside the workspace | Tool paths are confined to the workspace root. Absolute paths, parent-directory components, symlink escapes and `.lucubra/` are rejected | `crates/lucubra-mcp/src/material.rs`, `crates/lucubra-mcp/src/envelope.rs` |
| Opens files that are only in the cloud | No. Online-only placeholders are counted and their size is reported; the file is never opened | `docs/adr/0011-ingestion-pipeline-and-limits.md` (section 1) |
| Asks for or stores passwords of protected files | No. Password-protected files fail with the code `encrypted` and are never prompted for | `docs/adr/0011-ingestion-pipeline-and-limits.md` (section 3) |
| Sends the device identifier to the host | No tool response schema includes it | `crates/lucubra-mcp/src/tools/` |

To check the first, second and third rows from a clone of the repository:

```sh
grep -cE '^name = "(reqwest|hyper|ureq|isahc|native-tls|curl|attohttpc|surf)"' Cargo.lock
grep -cE '^name = "(requests|httpx|urllib3|aiohttp|pycurl)"' uv.lock
grep -rn 'TcpListener\|UdpSocket' crates --include='*.rs'
```

Each command should print `0` or nothing. The lock file checks cover the direct and transitive dependencies of both languages.

## 2. Processes

| Process | Started by | Input and output | Notes |
|---|---|---|---|
| `lucubra` CLI | You, in a terminal | Arguments; output to the terminal | Commands: `init`, `rebuild`, `project`, `reset`, `status`, `scan`, `ingest`, `search`, `doctor`, `transcript`, `watch`, `mcp` (`crates/lucubra-cli/src/main.rs`) |
| `lucubra mcp` | The AI host, from its MCP configuration | JSON-RPC on standard input and output; diagnostics on standard error only | `crates/lucubra-mcp/src/server.rs`, `docs/adr/0012-mcp-server-surface-and-budgets.md` (decision 4) |
| Python worker (`python -m lucubra_workers`) | The engine | One JSON request per line in, one JSON reply per line out | `docs/adr/0001-wheel-layout.md`, `python/lucubra_workers/__init__.py`. The interpreter is taken from `LUCUBRA_PYTHON`, then from the folder of the `lucubra` binary, then from `PATH` |
| Tier 2 helpers (ODA File Converter or LibreDWG for DWG; Blender for `.blend`) | The engine, only when you have installed them | The file you asked to ingest in; a JSON file in a scratch folder out | Detected, never bundled (decision D20). `crates/lucubra-ingest/src/tier2.rs` |

Workers run under a time limit (120 seconds by default) and a memory limit (2 GiB by default, where the operating system allows one). A worker that exceeds either is stopped. See `docs/adr/0011-ingestion-pipeline-and-limits.md` section 4.

## 3. What Lucubra reads

| Input | Where | What Lucubra does with it | Source |
|---|---|---|---|
| Files in the workspace folder | Everywhere under the workspace root except the skipped folders `.lucubra`, `packs`, `sessions` and `dashboard`, and generated projection files | Hashes each file with BLAKE3. Extracts text only for supported formats. Records unknown formats as seen, without reading their content | `crates/lucubra-ingest/src/discover.rs`, `docs/adr/0011-ingestion-pipeline-and-limits.md` (sections 1 and 2) |
| Names starting with `.` or `~$` | Anywhere in the workspace | Skipped (Office lock files and hidden files) | `docs/adr/0011-ingestion-pipeline-and-limits.md` (section 1) |
| `PREFERENCES.md` | Workspace root | Returned to the host by `lucubra_orient`, capped at 2,400 characters | `crates/lucubra-mcp/src/material.rs` (`MAX_PREFERENCES_CHARS`) |
| `AGENTS.md` | Workspace root | Returned to the host by `lucubra_read` | `crates/lucubra-mcp/src/tools/read.rs` |
| Event logs | `.lucubra/events/*.jsonl`, from every device that synced into the workspace | Replayed to rebuild state | `docs/adr/0008-event-record-and-clock.md` |
| Device identifier | `device_id` in the application-data directory | Read at startup, created on first use | `crates/lucubra-core/src/appdata.rs` |
| State cache | `workspaces/<workspace-id>/cache.sqlite` in the application-data directory | Read to answer searches and reads | `docs/adr/0010-app-data-cache-and-reset.md` |
| Environment variables | At startup | `LUCUBRA_DATA_DIR` (data directory), `LUCUBRA_PYTHON` (worker interpreter) | `crates/lucubra-core/src/appdata.rs`, `docs/adr/0001-wheel-layout.md` |
| Tool arguments from the host | Standard input of `lucubra mcp` | Validated before use. Malformed or unknown arguments return an error envelope and change nothing | `docs/adr/0012-mcp-server-surface-and-budgets.md` (decision 3) |

## 4. What Lucubra writes, and where

Paths are relative to the workspace folder unless they start with `<data-dir>`. The data directory is defined in section 9.

| Path | Contents | Written by | Source |
|---|---|---|---|
| `.lucubra/VERSION` | Workspace format version | `lucubra init` | `crates/lucubra-core/src/workspace.rs` |
| `.lucubra/events/<device-id>.jsonl` | Event log for this device. One JSON object per line with the fields `v`, `id`, `dev`, `seq`, `hlc`, `ts`, `type` and `data`. Lines are only appended. Each append is flushed to disk with `sync_all` | `init`, `scan`, `ingest`, `transcript` and every MCP tool that records an event (for example `profile.set`, `file.seen`, `file.extracted`, `file.failed`, `step.completed`) | `docs/adr/0008-event-record-and-clock.md`, `crates/lucubra-core/src/log.rs` |
| `.lucubra/preserved/` | Copies of a generated file that you edited, saved before Lucubra regenerated the file. Names carry a UTC timestamp | Regeneration (`init`, `project`, `reset`) | `docs/adr/0009-projections-preservation-conflicts.md` (rule 4) |
| `.lucubra/conflicts/` | Sync-conflict copies of generated files, moved here and never deleted | Regeneration | `docs/adr/0009-projections-preservation-conflicts.md` (rule 6) |
| `.lucubra/written/<device-id>.json` | JSON map of workspace path to the BLAKE3 hash of each generated file this device wrote | Regeneration | `docs/adr/0009-projections-preservation-conflicts.md` (rule 3), `crates/lucubra-core/src/projection.rs` |
| `AGENTS.md` | Root index for coding agents and readers. Generated | Regeneration | `crates/lucubra-core/src/projection.rs` |
| `subjects/<subject>/agents_<subject>.md` and `subjects/<subject>/<unit>/UNIT.md` | Subject indexes and unit cards. Generated | Regeneration | `docs/adr/0009-projections-preservation-conflicts.md` (rule 2) |
| `<name>.transcript.md` beside the original file | Transcript of a page or image that the host read, marked verified or unverified | `lucubra transcript` | `crates/lucubra-cli/src/main.rs`, `docs/adr/0011-ingestion-pipeline-and-limits.md` (section 1), decision D21 |
| `<data-dir>/device_id` | Device identifier, `d-` followed by eight hexadecimal digits | First run of any command that writes events | `crates/lucubra-core/src/appdata.rs` |
| `<data-dir>/workspaces/<workspace-id>/cache.sqlite` | State cache: tables `meta`, `applied` and `state`, the full-text index, extracted text, chunks and rendered page images | Ingestion and rebuild | `docs/adr/0010-app-data-cache-and-reset.md`, `docs/adr/0011-ingestion-pipeline-and-limits.md` (section 6) |
| `<data-dir>/workspaces/<workspace-id>/tier2/<pid>-<n>/` | Scratch folder for one Tier 2 run (a helper script and its JSON output). Removed when the run ends | Tier 2 extraction | `crates/lucubra-ingest/src/tier2.rs` |
| `schedule/lucubra-plan.ics` | Reserved path for the study plan calendar file. This build does not write it | Planner, not yet built | `crates/lucubra-ingest/src/discover.rs` (listed as generated) |

Lucubra does not write diagnostics to a log file. Diagnostics go to standard error, and the host decides where they are kept. See `docs/adr/0012-mcp-server-surface-and-budgets.md` (decision 4).

Ingestion stores facts, not content, in the event log. A `file.extracted` event holds the hash, format, extractor name and version, chunk count, outline hints and a list of pages that need a vision read. Extracted text stays in the cache (`docs/adr/0011-ingestion-pipeline-and-limits.md`, section 6).

## 5. What leaves the machine

### 5.1 Through the MCP connection

The host that started `lucubra mcp` receives:

- the server instructions and tool descriptions, which contain no workspace content (`docs/adr/0012-mcp-server-surface-and-budgets.md`, decision 7);
- the output of each tool: orientation text, the `PREFERENCES.md` summary, `AGENTS.md`, search results, chunks of your study material, status counts and profile fields.

Study material is sanitized before it is returned. ANSI escape sequences, control characters and bidirectional override characters are removed. The text is wrapped in a payload marked `trust: "untrusted_study_material"` (`docs/adr/0012-mcp-server-surface-and-budgets.md`, decision 7; `crates/lucubra-mcp/src/material.rs`).

Each response is capped by the budget of the host's plan tier (`crates/lucubra-mcp/src/budget.rs`):

| Tier | Orientation response | Other responses | Session total before wrap-up |
|---|---|---|---|
| Free | 1,000 tokens | 2,000 tokens | 25,000 tokens |
| Paid | 1,500 tokens | 6,000 tokens | 80,000 tokens |
| Local | 700 tokens | 1,500 tokens | 12,000 tokens |

Tokens are estimated as one quarter of the character count, rounded up. The caps limit how much study material one response can carry. They do not limit what the host does with the response.

### 5.2 From the host to its model provider

Once the host has a response, the host decides what to do with it. Most hosts send the conversation, including tool results, to a model provider in the cloud. A host that runs its model on the same machine (for example LM Studio, or Jan or Open WebUI with a local model) keeps that data on the machine. Check the host's own documentation for where its model runs. Lucubra has no control over this step.

### 5.3 Through sync clients

If you keep the workspace folder in a synced folder (Google Drive, OneDrive, iCloud Drive or Dropbox), the sync client uploads everything in it, including `.lucubra/events/`, `.lucubra/written/`, your source files and the generated files. The application-data directory, which holds the cache, is designed to stay outside synced folders (`docs/adr/0010-app-data-cache-and-reset.md`, Context; decision D13). Treat the workspace as data that the sync provider can read.

### 5.4 Nothing else

Lucubra makes no other outgoing connection. Section 1 lists the checks.

## 6. What stays on the machine

- The event logs, the cache, the device identifier and the preserved and conflict copies, except when a tool response contains part of them.
- Your source files. They are read locally and sent only as chunks that a tool response contains.
- The Tier 2 scratch folders, which are removed when each run ends.

## 7. Retention

| Item | Kept until | Source |
|---|---|---|
| Event logs | You delete them. Lucubra never removes events, including events from a newer or unknown version | `docs/adr/0008-event-record-and-clock.md` (rule 8) |
| Preserved and conflict copies | You delete them. Nothing is removed automatically | `docs/adr/0009-projections-preservation-conflicts.md` |
| Written-hash records | Overwritten each time this device writes a generated file | `docs/adr/0009-projections-preservation-conflicts.md` (rule 3) |
| State cache | Rebuilt from the event logs on demand. Deleted by `lucubra reset`, and cleared on open when the engine upgrade changes the fold fingerprint | `docs/adr/0010-app-data-cache-and-reset.md` (rules 5 and 6) |
| Tier 2 scratch folders | Removed when the run ends. A crash can leave one behind | `crates/lucubra-ingest/src/tier2.rs` |
| Device identifier | Until the application-data directory is deleted | `crates/lucubra-core/src/appdata.rs` |
| Anything the host keeps (conversations, its logs) | According to the host | Not Lucubra's to control |

## 8. Inspecting your data

1. Event logs are plain text. Each line is one JSON object:

   ```sh
   cat ".lucubra/events/"*.jsonl | less
   ```

2. Generated files that you edited are kept in `.lucubra/preserved/`:

   ```sh
   ls -la .lucubra/preserved/
   ```

3. Counts and warnings, read from the event logs:

   ```sh
   lucubra status --workspace <workspace-folder>
   ```

4. The cache is an SQLite database. Open it read-only:

   ```sh
   sqlite3 -readonly <data-dir>/workspaces/<workspace-id>/cache.sqlite ".tables"
   ```

   Do not edit the cache by hand. Rebuild it with `lucubra rebuild` instead.

5. The device identifier:

   ```sh
   cat <data-dir>/device_id
   ```

## 9. Locating the data directory

The data directory is `LUCUBRA_DATA_DIR` when that variable is set and not empty. Otherwise it is the platform's local data directory for the application `lucubra`, resolved by the `directories` crate (`crates/lucubra-core/src/appdata.rs`). On Windows this is normally `%LOCALAPPDATA%\lucubra\data`. The exact location on macOS and Linux is confirmed in the test report for each release.

The workspace identifier is a ULID that the `workspace.initialized` event records. The same workspace synced to another device maps to the same cache folder (`docs/adr/0010-app-data-cache-and-reset.md`, rule 3).

## 10. Deleting your data

Choose the smallest step that meets your need.

1. Clear derived state. This removes the cache and the generated files you have not edited. Events, sources, notes and preserved edits are kept:

   ```sh
   lucubra reset --workspace <workspace-folder>
   ```

   The command asks for confirmation. `--yes` skips the question.

2. Remove the application-data directory. This removes the device identifier and the cache of every workspace on the machine. It does not touch any workspace folder. Lucubra creates a new device identifier the next time it runs.

3. Remove one workspace's Lucubra state:
   - delete `<workspace-folder>/.lucubra/`. This removes the event history of this device and every device that synced into the workspace, the preserved edits, the conflict copies and the written-hash records;
   - delete the generated `AGENTS.md`, and the `subjects/` folder if it holds only generated files (check first, because you may have added your own notes there). `lucubra project` regenerates them.

   Deleting `.lucubra/` cannot be undone. Lucubra does not keep a copy elsewhere.

4. Remove what the host keeps. Delete the Lucubra entry from the host's MCP configuration, and delete any conversation or memory in the host that holds study material. Lucubra cannot reach these.

After steps 2 and 3, the `lucubra` commands that need state will create it again on their next run. Run step 3 on every device that synced into the workspace, otherwise the other device's log will still be there when you sync again.

## 11. Summary table

| Data item | Stored where | Leaves the machine? | How to delete |
|---|---|---|---|
| Your source files (PDF, Office, spreadsheets, CAD and so on) | Your workspace folder (not changed by Lucubra) | Only as chunks in a tool response; a synced folder uploads the file itself | Delete the file. Then `lucubra reset` removes its index entries |
| File paths, sizes, hashes and modification times | `.lucubra/events/<device-id>.jsonl` (`file.seen`) | Only if a tool response includes them | Delete `.lucubra/`, or remove the matching lines from the log |
| Extracted text, chunks and rendered page images | `<data-dir>/workspaces/<id>/cache.sqlite` | Only as chunks in a tool response | `lucubra reset`, or delete the data directory |
| Profile fields you set | `.lucubra/events/` (`profile.set`) | Only in responses that include profile data (`lucubra_profile`) | Delete `.lucubra/` |
| Host name and version from the MCP handshake | `.lucubra/events/` (`profile.set`) | Only if the host asks through `lucubra_profile` | Delete `.lucubra/` |
| Preserved edits and conflict copies | `.lucubra/preserved/`, `.lucubra/conflicts/` | Only if a tool response includes them | Delete the folders |
| Generated index files (`AGENTS.md`, `subjects/`) | Workspace folder | Yes, `AGENTS.md` is returned by `lucubra_read` | `lucubra reset` removes unedited files; delete the others |
| Transcripts you saved | Beside the original file, `*.transcript.md` | Only if a tool response includes them | Delete the file |
| Device identifier | `<data-dir>/device_id` | No. Not part of any tool response | Delete the data directory |
| Tier 2 scratch output | `<data-dir>/workspaces/<id>/tier2/` | No | Removed by Lucubra; delete the data directory after a crash |
| Diagnostics | Standard error of the `lucubra mcp` process | Held by the host, if the host keeps logs | Managed by the host |
| Conversations and tool results held by the host | The host | Yes, to the host's model provider unless the model is local | Delete in the host |
| Copies in a synced folder | The sync provider | Yes | Delete in the sync client's trash too |

## 12. Planned features (not in this build)

These features are planned. None of them is in the current build, and each is opt-in:

- Calendar reading through a calendar secret address (iCal) that you supply. The address would be stored outside the workspace, in the operating system's credential store. Planned for milestone M5.
- Optional vector search. The optional extra is off by default (`docs/adr/0005-embedding-stack.md`). Any model download would happen only after you opt in. Planned.
- A companion page on the loopback address (`127.0.0.1`) for hosts that cannot reach a local server. It would require a token and check the Host and Origin headers. Planned for milestone M7.

When any of these is built, the sections 1, 5 and 11 of this document change, and the release checks in section 13 are run again before the release.

## 13. Verification

This document is verified before each release with these checks (run from a clone of the repository):

1. The network checks in section 1 print `0` or nothing.
2. Every path in section 4 matches a line in the source file named in its row.
3. The Blender helper is started with autoexec disabled (flag `--disable-autoexec` in `crates/lucubra-ingest/src/tier2.rs`). At the time of writing this flag is on `feat/m4-onboarding` only, not on `main`. This check stays open until the change reaches `main`.
4. The release build starts with `lucubra mcp` and makes no connection other than standard input and output. The test report for the release records the run.

Checks 1 and 2 were run against `eb106e0`. Check 3 is open. Check 4 is run on the release candidate.
