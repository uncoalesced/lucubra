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
- Onboarding and organisation: profile questions with form elicitation where the host supports it, file classification, a first-run layout for approval, file moves and undo.
- Planner and sessions: effort and priority model, scheduler with an overcommit warning, Pomodoro-Flow blocks, session scripts and handoff files, iCal busy times (address stored outside the workspace) and a `schedule/lucubra-plan.ics` export.
- Practice: 13 item types, a grounding check against your own notes, an engine grader for closed formats and rubric grading for open ones, FSRS review, exam simulation, tutor mode for assessment files and Anki export.
- Pack mode and companion page: a study pack of at most four files for hosts without MCP, handoff blocks ingested from `inbox/` or pasted, and `lucubra serve` on 127.0.0.1 with a per-launch token.
- Dashboard: weekly consistency, time per subject, mastery, retention curve, readiness per exam, plan adherence, learned capacity and focus span, in `lucubra_status`, the companion page and a static `dashboard/index.html`.
- Daily check-in (`lucubra_session` `checkin` and the companion page), with a sleep and retention view that stays hidden until 30 days have both a check-in and review data.
- Nine domain packs with item templates, exam method overrides, optional onboarding questions and companion MCP server recommendations. Lucubra never installs a recommended server.
- Hermes Agent profile states the 4096-character message limit of messaging apps such as Telegram.
- `lucubra doctor --report` writes a redacted diagnostic file (versions, tools, host profile, error codes) to attach to an issue. File names are added only with `--include-names`.
- Clean install check on Windows and macOS in the release workflow, through the install scripts.
- Teaching styles as data: eight styles (tutor, tester, explainer, socratic, drill, viva, exam simulation, revision), each with its own step sequence. `lucubra_session` returns the active style's steps, and a change of style takes effect from the next step.
- Study scripts: accepting a plan writes `sessions/SCRIPT_<date>_<slug>.md` for each plan day, with timed blocks and breaks, links to the unit cards, 3 to 5 recall questions per block and the answers at the end. The session handoff compares planned minutes with the minutes done.
- Preferences: `lucubra_profile action=preference` records learning and output preferences, projected to `PREFERENCES.md` (at most 600 tokens, user edits preserved). There is no field for a condition or diagnosis.
- Reminders, off by default: opt in per type (study blocks, review days, exam countdown) to add calendar alarms to `schedule/lucubra-plan.ics`. Review days come from the review schedule. Claude Desktop and Claude Code also get a scheduled-task recipe in `lucubra_orient`.
- Companion page: an easier-to-read font option, using fonts already on the device.
- Grades: a grading scheme (a 10-point credit-weighted preset or custom bands), results per term with credits, SGPA and CGPA in `lucubra_status`, and subject credits and prerequisites in the profile.
- Target CGPA: the grade and minimum marks each pending subject needs, or the best reachable value when the target is out of reach. While a target is set, the planner gives more weight to subjects with more credits (at most 1.5 times).
- `lucubra term close --term <label> [--keep <subject>]` copies the generated files into `archive/<label>/`, carries failed subjects over with their mastery and items, keeps chosen subjects' review cards on a slow schedule and retires the rest. Your own files are not moved.
- Past papers: questions submitted with `past_paper: {year, marks}` give topic frequency in `lucubra_status` ("asked in 4 of the last 5 years", shown as history, not a prediction). Frequency raises a unit's planner weight by at most 1.5 times, and exam simulation follows the past papers' unit and question-type mix.
- Mock tests: `lucubra_export action=mock_test` writes a printable paper to `exports/` as Markdown and as an HTML page you can print or save as PDF, with marks per question and no answer key. `lucubra_practice action=mock_grade` grades the photographed answers: closed questions by the engine, open ones by rubric scores. The read-back stays unverified until you confirm it.
- Downloads: `lucubra_ingest action=fetch` saves a file into `inbox/` from a site listed in `SOURCES.md`, over HTTPS, checking every redirect against the list. It is off when `SOURCES.md` does not exist.
- Community and policy documents: README, contributing guide, code of conduct, security policy, privacy statement, terms of use, accessibility statement and data handling policy.
- Issue templates (bug report, feature request, host compatibility report) and a pull request template.

### Changed

- minimum Python version is now 3.11

### Security

- Pillow 13.0.0 (43 advisories), url 2.5.9 (RUSTSEC-2026-0592)

### Known limitations

- The `mock_test` action of `lucubra_export` returns a not-implemented error in this build.
- The `fetch` action of `lucubra_ingest` is not implemented.
- Revit (`.rvt`) and SketchUp (`.skp`) files are detected but not extracted.
