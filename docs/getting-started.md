# Getting started with Lucubra

Lucubra is a local-first study engine. It reads your course material on your own computer, builds a searchable index, and hands small pieces of that material to an AI assistant through the Model Context Protocol (MCP). The program is a command-line tool named `lucubra` with Python extraction workers. An AI host starts it as a local process over stdio with `lucubra mcp -w <workspace>`.

This guide takes you from nothing to a first conversation. Host-specific configuration is in [docs/hosts](hosts/). Problems are covered in [troubleshooting](troubleshooting.md). Beta testers should also read [TESTING.md](../TESTING.md).

The commands and outputs in this guide were run on Windows 11 against a copy of a real 305-file study folder. Where a step could not be checked, the text says so.

## Requirements

| Item | Requirement |
|---|---|
| Operating system | Windows 10 or 11, macOS, or Linux |
| Python | 3.11 or later. `uv` installs one for you if none is present |
| uv | Recommended installer. See <https://docs.astral.sh/uv/> |
| Disk | The search index and rendered page images are stored outside the workspace, in the data directory. A 449 MB study folder (305 files, 480 scanned PDF pages) produced a 7 MB index and 419 MB of rendered page images. Plan for free space about equal to the size of your scanned material |
| AI host | Any host in the [host list](#connect-an-ai-host) that can run a local MCP server |
| Node.js | Optional. Only needed for `npx` based tools such as the MCP Inspector used in [TESTING.md](../TESTING.md) |

Optional external tools widen the set of readable formats. `lucubra doctor` reports which are present:

| Tool | Reads | Environment variable |
|---|---|---|
| ODA File Converter or LibreDWG `dwg2dxf` | `.dwg` | `LUCUBRA_ODA_PATH` or `LUCUBRA_LIBREDWG_PATH` |
| Blender | `.blend` | `LUCUBRA_BLENDER_PATH` |

## Install

Pick one option. All of them put a `lucubra` command on your path. Version `1.0.0b1` is a pre-release, so each installer must be told to allow pre-releases.

### Option 1: install script

The repository holds two scripts: `packaging/install.ps1` for Windows PowerShell and `packaging/install.sh` for macOS and Linux. Each installs `uv` with the official Astral installer when it is missing, then runs `uv tool install` for Lucubra, pre-releases included. Neither script edits your `PATH` itself. Running one again upgrades in place.

```powershell
powershell -ExecutionPolicy Bypass -File packaging\install.ps1
powershell -ExecutionPolicy Bypass -File packaging\install.ps1 -Version 1.0.0b1
powershell -ExecutionPolicy Bypass -File packaging\install.ps1 -WheelUrl <wheel URL from the release page>
```

```sh
sh packaging/install.sh
sh packaging/install.sh --version 1.0.0b1
sh packaging/install.sh --wheel-url <wheel URL from the release page>
```

The scripts were read but not run for this guide, because they install software outside the test environment. They end with `lucubra --version`, or a message that the `uv` tool folder is not on your `PATH` (run `uv tool update-shell`, then open a new terminal). They use `uv tool install`, so the Python worker issue described under option 2 applies to them as well. Read a script before you run it, as with any installer.

### Option 2: uv tool install

```sh
uv tool install --prerelease allow lucubra
```

`uv tool install` keeps `lucubra` and its Python packages in an isolated environment and places a launcher on your path. Run `uv tool update-shell` once if the launcher folder is not on your path, then open a new terminal.

Known issue in the beta build, seen on Windows: the launcher that `uv` places on your path is a copy of the executable, and Lucubra looks for its Python workers next to the executable it was started from. From the launcher it finds none, so `lucubra doctor` prints `missing  python` and `lucubra ingest` prints `worker unavailable: no Python found next to lucubra` and leaves PDF, DOCX and PPTX files waiting. Point Lucubra at the tool environment's interpreter:

```powershell
$env:LUCUBRA_PYTHON = "$(uv tool dir)\lucubra\Scripts\python.exe"      # Windows
export LUCUBRA_PYTHON="$(uv tool dir)/lucubra/bin/python"              # macOS, Linux (not verified)
```

Set the same variable in the `env` block of every host configuration (see [troubleshooting](troubleshooting.md#python-worker-not-found)). Running the executable inside the tool environment directly, `$(uv tool dir)\lucubra\Scripts\lucubra.exe`, also works because the interpreter sits beside it.

### Option 3: pip

```sh
python -m venv lucubra-venv
lucubra-venv/bin/pip install --pre lucubra        # Windows: lucubra-venv\Scripts\pip install --pre lucubra
```

With pip the command is `lucubra-venv/bin/lucubra` (Windows: `lucubra-venv\Scripts\lucubra.exe`). Use that full path in host configuration. The extraction workers are found next to the `lucubra` executable, so the executable must stay inside the environment that holds its dependencies.

### Option 4: wheel from the GitHub release

Download the wheel for your platform from <https://github.com/uncoalesced/lucubra/releases>, then:

```sh
uv tool install path/to/lucubra-<version>-<platform>.whl
```

This is the route used to produce the results in [TESTING.md](../TESTING.md) and the test report, with the wheel installed into a clean virtual environment:

```powershell
uv venv venv
uv pip install --python venv\Scripts\python.exe lucubra-0.0.1-py3-none-win_amd64.whl
venv\Scripts\lucubra.exe --version
```

### Option 5: Claude Desktop bundle

For Claude Desktop only, the release page carries `lucubra.mcpb`, an MCP bundle. Open Claude Desktop, go to Settings, then Extensions, and install the file. The bundle asks for the workspace folder and starts the server with `uv run`. It does not create the workspace, so the folder must already have been initialised with `lucubra init` (install the command-line tool with one of the options above, or the server will exit with `is not a Lucubra workspace`). See [Claude Desktop](hosts/claude-desktop.md). Not verified on a live Claude Desktop.

### Check the install

```sh
lucubra --version
```

The beta build prints a line of the form `lucubra X.Y.Z`. Report that string in any bug report.

## Create a workspace

A workspace is an ordinary folder. Lucubra adds a small layout to it and never moves or deletes your files without asking. Use an absolute path.

```powershell
lucubra init -w C:\Users\you\Documents\Study
```

`init` creates `inbox/`, `schedule/`, `subjects/`, `sessions/`, `packs/`, `dashboard/` and a hidden `.lucubra/` folder that holds the append-only event log, and writes a root `AGENTS.md`. Output ends with a line such as:

```text
workspace: C:\Users\you\Documents\Study
device: d-8d8b0a4c
projections: 1 written, 0 unchanged
state-hash: f16fbe8c8b043f8075b62cd5250666ae225dbfe86a80e5f229734eec211d01e6
```

`init` is safe to repeat. It prints a `conflict-named file left in place` line for each file whose name looks like a sync-conflict copy, for example `notes (1).pdf`. These are warnings only. Nothing is moved.

If the workspace folder lives in a synced drive (Google Drive, OneDrive, Dropbox), files that exist only online are counted by `scan` and never downloaded.

## Add your material

Copy or drop files anywhere inside the workspace. Existing course folders can be used in place, so the easiest start is to run `lucubra init -w` on the folder that already holds your notes. Files placed in `inbox/` are treated as new arrivals.

Readable formats in the beta build:

| Format | Notes |
|---|---|
| PDF | Text pages are indexed. Pages with almost no text (scans, photographed notes) are rendered to images for the AI host to read |
| DOCX, PPTX | Text, tables and speaker notes |
| XLSX, CSV and related | Sheet summaries and ranges |
| JPG, JPEG, PNG | Rendered for the AI host to read. No text is indexed until the host transcribes them |
| Markdown, plain text, EPUB | Indexed as text |
| DXF, IFC, DICOM, STL and similar | Metadata and structure; see `lucubra doctor` for the tools some formats need |

Files that Lucubra does not recognise (for example `.mp4`, `.svg`) are listed by `scan` as `[unknown]` and are not read.

## Scan, ingest, status and search

```sh
lucubra scan -w <workspace>
```

`scan` hashes every file and records new and changed ones. It does not read content. It prints `files: N new or changed, M unchanged`, then one line per file with its detected format.

```sh
lucubra ingest -w <workspace>
```

`ingest` runs `scan` and then extracts text from every file not yet indexed. It prints a progress line every 50 files and ends with counts:

```text
extraction: 295 done now, 0 already indexed, 0 failed or pending earlier
vision: 480 pages rendered, 0 already cached
```

`vision` counts pages and images with too little text to index. Those pages are rendered to PNG in the data directory for the AI host to read and transcribe during a conversation. Run `ingest` yourself before your first conversation: an ingest of 300 files took about two minutes on the test machine, which is longer than most AI hosts wait for a single tool call.

```sh
lucubra status -w <workspace>
```

`status` prints the workspace id, event counts and the state hash. It is the quick check that the event log is readable (`unreadable lines: 0`).

```sh
lucubra search -w <workspace> "input tax credit"
lucubra search -w <workspace> --substring taxab --limit 5
```

`search` prints `results: N` followed by one reference and a snippet per hit. The reference (for example `F:b5721a5a/p:1`) is what the AI host passes to `lucubra_read`. `--substring` matches inside words and needs three or more characters per term.

```sh
lucubra project -w <workspace>
```

`project` regenerates `AGENTS.md`, subject indexes and unit cards. Edits you make to generated files are preserved.

## Check external tools with doctor

```sh
lucubra doctor
```

Real output on a machine with the Python workers but neither optional tool:

```text
ok       python -> E:\TestEnvironments\lucubra-college-2026\venv\Scripts\python.exe
missing  oda (DWG converter (ODA File Converter or LibreDWG dwg2dxf))
         install: Install ODA File Converter (https://www.opendesign.com/guestfiles/oda_file_converter) or LibreDWG (`dwg2dxf`) and add it to PATH (or set LUCUBRA_ODA_PATH).
missing  blender (Blender (headless .blend reader))
         install: Install Blender (https://www.blender.org/download/) and add `blender` to PATH (or set LUCUBRA_BLENDER_PATH).
```

`ok python` is the line that matters. A missing `oda` or `blender` only affects `.dwg` and `.blend` files. Given a workspace (`lucubra doctor -w <workspace>`), `doctor` also retries files that were waiting on a tool.

## Connect an AI host

Each host has its own page with the exact config file path for every operating system, a copy-paste snippet, a verification step and known limits.

| Host | Page |
|---|---|
| Claude Desktop (Free and paid) | [claude-desktop.md](hosts/claude-desktop.md) |
| Claude Code | [claude-code.md](hosts/claude-code.md) |
| Codex (CLI, IDE, ChatGPT desktop Codex tab) | [codex.md](hosts/codex.md) |
| Antigravity | [antigravity.md](hosts/antigravity.md) |
| Cursor | [cursor.md](hosts/cursor.md) |
| Windsurf | [windsurf.md](hosts/windsurf.md) |
| VS Code with GitHub Copilot | [vscode-copilot.md](hosts/vscode-copilot.md) |
| LM Studio | [lm-studio.md](hosts/lm-studio.md) |
| Hermes Agent | [hermes-agent.md](hosts/hermes-agent.md) |
| Jan and Open WebUI | [jan-open-webui.md](hosts/jan-open-webui.md) |
| Any other MCP host | [generic.md](hosts/generic.md) |

Every configuration has the same shape: a command (`lucubra` or its full path) and the arguments `mcp -w <absolute workspace path>`. The workspace must already have been created with `lucubra init`, otherwise the server exits at start with `<path> is not a Lucubra workspace; run lucubra init there first`.

Lucubra reads the host name that the host announces when it connects and picks a response budget from it. Claude Desktop is treated as a Free plan. If you pay for your AI plan, tell Lucubra once; the host pages show how.

## First conversation

The server exposes five prompts. Where a host shows MCP prompts (usually behind a slash or a plus menu), choose one. Otherwise type the equivalent request.

| Prompt | What it does | Equivalent request |
|---|---|---|
| `onboard` | Orients on the workspace, asks any missing profile questions, scans `inbox/` | "Start Lucubra onboarding for my study folder." |
| `study` | Agrees study blocks, then teaches with checkpoints. Optional `subject`, `unit`, `style` | "Use Lucubra to start a study session on GST unit 1." |
| `plan` | Reviews exams and commitments and proposes a schedule. Optional `days`, `focus` | "Use Lucubra to plan my next 7 days." |
| `quiz` | Grounded practice from your own notes. Optional `subject`, `unit`, `mode` | "Quiz me on GST from my notes with Lucubra." |
| `wrap-up` | Writes a short session summary and closes the session. Optional `reason` | "Wrap up this Lucubra session." |

The assistant should call `lucubra_orient` first in every conversation. That call reports how many files are extracted, how many still need the host to read page images, your profile, and the next suggested step. On a fresh workspace it returns `"next":"onboard"`.

Study material returned by Lucubra is sent to the AI host's provider as part of the conversation. Do not connect a workspace that holds files you are not willing to share with that provider.

## Where data lives

| Data | Location |
|---|---|
| Your files | The workspace folder, untouched |
| Event log, preserved edits, conflicts | `<workspace>/.lucubra/` (small, text, safe to sync) |
| Generated indexes | `<workspace>/AGENTS.md`, `<workspace>/subjects/` |
| State cache, search index, rendered page images, device id | The data directory, outside the workspace |

The data directory is the platform's local application data folder for `lucubra`. Set `LUCUBRA_DATA_DIR` to move it. Nothing in the data directory is the only copy of anything you wrote: it can all be rebuilt from the workspace.

Other environment variables:

| Variable | Purpose |
|---|---|
| `LUCUBRA_DATA_DIR` | Data directory |
| `LUCUBRA_PYTHON` | Python interpreter for the extraction workers. By default the interpreter next to the `lucubra` executable is used |
| `LUCUBRA_ODA_PATH`, `LUCUBRA_LIBREDWG_PATH` | DWG converter |
| `LUCUBRA_BLENDER_PATH` | Blender |
| `LUCUBRA_LOG` | Log filter for `lucubra mcp`, written to standard error. For example `LUCUBRA_LOG=debug` |

Some hosts start servers with an almost empty environment. If you set any of these variables, put them in the host configuration's `env` block, not only in your shell.

## Reset

```sh
lucubra reset -w <workspace>          # asks you to type 'reset'
lucubra reset -w <workspace> --yes
```

`reset` deletes the workspace's state cache in the data directory and any generated file you have not edited, then regenerates the generated files. It never touches your sources, notes, the event log, preserved edits or conflict copies. Because the search index lives in the same cache, run `lucubra ingest` again afterwards. `lucubra rebuild -w <workspace>` has the same effect on the cache and prints the new state hash.

To remove Lucubra state completely, delete `<workspace>/.lucubra/` and the data directory by hand.
