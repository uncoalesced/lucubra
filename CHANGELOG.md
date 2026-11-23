# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Python packages use the PEP 440 form of each version (for example `1.0.0b1` for `1.0.0-beta.1`).

## [Unreleased]

### Added

- Event core: append-only, per-device JSONL event logs in `.lucubra/events/`, with a hybrid logical clock, replay in a deterministic order and a rebuildable SQLite state cache.
- Workspace commands: `init`, `rebuild`, `project`, `reset`, `status`, `scan`, `ingest`, `search`, `doctor`, `transcript` and `watch`.
- Ingestion: discovery, BLAKE3 hashing, magic-byte and extension routing, native extraction for spreadsheets, text, notebooks, images and common CAD text formats, and Python workers for PDF, Office, DXF and DICOM files.
- Ingestion limits: input size, zip container checks, extracted text size, per-request timeout and worker memory limits.
- Vision detection for PDF pages and slides with little extractable text, and transcripts saved beside the original file as unverified until you confirm them.
- Projections: generated `AGENTS.md`, subject indexes and unit cards, with user edits preserved in `.lucubra/preserved/` before regeneration and sync-conflict copies moved to `.lucubra/conflicts/`.
- MCP server over stdio (`lucubra mcp`) with 11 tools: `lucubra_orient`, `lucubra_profile`, `lucubra_ingest`, `lucubra_read`, `lucubra_search`, `lucubra_submit`, `lucubra_plan`, `lucubra_session`, `lucubra_practice`, `lucubra_export` and `lucubra_status`.
- MCP server prompts: 5 prompts for the common study flows.
- Host detection from the MCP handshake for 11 host profiles, with default response budgets per plan tier (free, paid and local).
- Response budgets: per-response and per-session token limits, with a wrap-up signal when the session total passes its threshold.
- Strict validation of tool arguments, returning a compact error envelope with a single-line fix.
- Tier 2 helpers (ODA File Converter or LibreDWG for DWG, Blender for `.blend`), detected when installed and never bundled.
- Community and policy documents: README, contributing guide, code of conduct, security policy, privacy statement, terms of use, accessibility statement and data handling policy.
- Issue templates (bug report, feature request, host compatibility report) and a pull request template.

### Changed

- minimum Python version is now 3.11

### Security

- Pillow 13.0.0 (43 advisories), url 2.5.9 (RUSTSEC-2026-0592)

### Known limitations

- `lucubra_plan` and `lucubra_practice`, and some actions of `lucubra_export`, `lucubra_submit` and `lucubra_ingest`, return a not-implemented error in this build.
- The `fetch` action of `lucubra_ingest` is not implemented.
- Revit (`.rvt`) and SketchUp (`.skp`) files are detected but not extracted.
