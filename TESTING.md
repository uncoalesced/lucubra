# Beta tester guide

This guide is for people testing Lucubra `1.0.0-beta.1` with an AI host. The goal is to find out whether Lucubra installs, connects and behaves correctly on your host and plan, and to capture enough detail to reproduce any failure. A full pass takes about 20 minutes once Lucubra is installed.

## Privacy

Everything an AI host reads through Lucubra is sent to that host's provider as part of the conversation. Search results, page text and page images all count. Test with material you are willing to share with that provider, and do not connect a workspace that holds exams, personal documents or anything confidential. Lucubra itself keeps your files on your computer and does not send anything over the network. Hosts that run a local model (LM Studio, Jan) keep the material on your computer.

Do not paste secrets, tokens or personal data into a bug report. The logs Lucubra writes name file paths, so check them before you attach them.

## What you need

- A computer with Python 3.11 or later available (`uv` installs one) and one of the hosts below.
- A folder of 10 to 30 study files: some PDFs, a DOCX or PPTX, ideally one scanned page or photo. A larger folder works, but the first ingest takes minutes (about 100 seconds for 305 files with 480 scanned pages in testing).
- Your AI host's plan name (Free, Pro, Plus, Max, Team and so on), because Lucubra behaves differently by plan.

## Setup checklist

Tick each line. Stop at the first failure and report it.

Install and workspace:

- [ ] `uv tool install --prerelease allow lucubra` (or another route from [getting started](docs/getting-started.md)) finishes without error.
- [ ] `lucubra --version` prints a version. Write it down.
- [ ] `lucubra doctor` prints `ok       python -> ...`. If it prints `missing  python`, set `LUCUBRA_PYTHON` as described in [troubleshooting](docs/troubleshooting.md#python-worker-not-found) and note that you had to.
- [ ] `lucubra init -w <workspace>` prints a `state-hash:` line.
- [ ] Your files are in the workspace folder (or in its `inbox/` folder).
- [ ] `lucubra ingest -w <workspace>` ends with `extraction: N done now, ... 0 failed or pending earlier`.
- [ ] `lucubra search -w <workspace> <a word from your notes>` prints `results:` with a number above 0.

Host connection (open the page for your host):

| Host | Page |
|---|---|
| Claude Desktop, Free or paid | [claude-desktop.md](docs/hosts/claude-desktop.md) |
| Claude Code | [claude-code.md](docs/hosts/claude-code.md) |
| Codex | [codex.md](docs/hosts/codex.md) |
| Antigravity | [antigravity.md](docs/hosts/antigravity.md) |
| Cursor | [cursor.md](docs/hosts/cursor.md) |
| Windsurf | [windsurf.md](docs/hosts/windsurf.md) |
| VS Code with Copilot | [vscode-copilot.md](docs/hosts/vscode-copilot.md) |
| LM Studio | [lm-studio.md](docs/hosts/lm-studio.md) |
| Hermes Agent | [hermes-agent.md](docs/hosts/hermes-agent.md) |
| Jan or Open WebUI | [jan-open-webui.md](docs/hosts/jan-open-webui.md) |
| Anything else | [generic.md](docs/hosts/generic.md) |

- [ ] The config entry is saved and the host was fully restarted.
- [ ] The host lists a `lucubra` server with 11 tools.
- [ ] If you pay for the host, you set `plan_tier` to `paid` (step 6 below checks it).

## The 20-minute script

Use one conversation for steps 2 to 8. Copy the prompts as written. Write down the time each step takes and anything unexpected.

1. Setup, 5 minutes. Complete the checklist above.
2. Orient, 2 minutes. Type: "Call lucubra_orient and tell me exactly what it returned." Expect `"ok": true`, the number of extracted files, and a `host` block. Check that `host.id` matches your host (for example `claude-desktop`) and that `tier` is what you expect. Record both. A `host.id` of `generic` on a host that has its own page is a detection bug worth reporting.
3. Onboarding prompt, 2 minutes. Use the `onboard` prompt if your host shows MCP prompts, otherwise type: "Start Lucubra onboarding for my study folder." Answer the questions it asks. Record whether it asked questions in chat or in a form, and whether it asked you to upload anything Lucubra already holds (it should not).
4. Search, 2 minutes. Type: "Use Lucubra to search my notes for <a topic from your files>." Expect a short list of hits with file names. Record the file names and whether they are right.
5. Read, 2 minutes. Type: "Open the first result and summarise it in five lines." The summary must match your file. Record whether the assistant called `lucubra_read` and whether the text was cut off (`truncated`).
6. Plan tier, 1 minute. If you pay for your host, type: "Use the Lucubra tool lucubra_profile to set my plan_tier to paid, then call lucubra_orient again." Expect `"updated": ["plan_tier"]` and then `"tier": "paid"`. Record if it did not change. If you are on a free plan, skip this step and note that.
7. Quiz prompt, 4 minutes. Use the `quiz` prompt or type: "Quiz me on <topic> from my notes with Lucubra. Ask five questions, one at a time." Answer two of them. Check that each question is based on your files and that the assistant can say where it came from. Record any question that is not in your notes.
8. Wrap-up, 2 minutes. Use the `wrap-up` prompt or type: "Wrap up this Lucubra session." Expect a short summary of at most 150 words and a tool result with `"status": "ended"`. The beta build records the session in the workspace event log. A handoff file in `<workspace>/sessions/` may not be written yet, so its absence is not a failure.
9. Check the workspace, 1 minute. Run `lucubra status -w <workspace>` and confirm `unreadable lines: 0`.

Optional if time allows: scanned material. Ask the assistant to read a photographed page that has no text. Lucubra renders such pages to images, and the assistant is expected to transcribe them and save the result. Record whether your host could read the image, how long it took, and whether your plan's usage limit stopped it.

Some features are still being built in this beta. A tool that returns `"code": "not_implemented"` is expected for features such as layout approval and moves. Note it, but it is not a host failure.

## What to record

For every run, whether it passed or failed:

| Item | Example |
|---|---|
| Host and version | Claude Desktop 1.x, Cursor 2.x |
| Plan tier | Free, Pro, Max, Team, free local model |
| Operating system | Windows 11, macOS 15, Ubuntu 24.04 |
| Lucubra version and install route | `lucubra 1.0.0b1`, `uv tool install` |
| Model, if the host lets you choose | Name shown in the host |
| `host.id` and `tier` from `lucubra_orient` | `claude-desktop`, `free` |
| Steps that passed | 1, 2, 3, 4, 5, 7, 8 |
| Steps that failed | Step 5 |
| What you did and what you expected | One line each |
| The exact error | Copy the text. Do not retype it. A screenshot is fine if you cannot copy |
| How long the slow steps took | Ingest 100 s, quiz step 4 min |
| Whether it happens every time | Yes, 3 of 3 |

Where the logs are:

- Lucubra writes server logs to standard error. Set `LUCUBRA_LOG=debug` in the host's `env` block for detail.
- The host keeps the MCP log. The path is on each host page. Claude Desktop writes `mcp-server-lucubra.log` in its logs folder.
- `lucubra status -w <workspace>` and `lucubra doctor` output are worth pasting.

## How to file the report

1. Open <https://github.com/uncoalesced/lucubra/issues/new?template=host-report.yml>.
2. Fill in every field of the host-report form with the items above.
3. Attach logs only after reading them for personal information.
4. One issue per host and plan combination. If you tested two hosts, file two reports.
5. A report of a successful run is as useful as a failure. File it too.

If you cannot reach GitHub, send the same items to the person who invited you.

## Known issues going into the beta

These were found while preparing this guide. You do not need to report them again unless the behaviour differs.

- After `uv tool install` on Windows, Lucubra may not find its Python workers (`missing  python`). The workaround is `LUCUBRA_PYTHON`, see above.
- `lucubra reset` and `lucubra rebuild` clear the search index silently, so search returns `results: 0` until you run `lucubra ingest` again.
- Files in unrecognised formats (`.mp4`, `.svg`) are skipped without a message.
- Paid plans start on Free budgets (responses capped at 2,000 tokens) until `plan_tier` is set.
- Photographs and scans have no searchable text until the assistant transcribes them. In one real study folder, 185 of 305 files were like that.
