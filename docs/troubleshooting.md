# Troubleshooting

Every message quoted here was produced by the beta build (`lucubra 0.0.1`) on Windows 11 during verification on a copy of a real study folder, unless a line says it was not reproduced. Start with the three quick checks, then find your symptom below. If nothing fits, file a report with the host-report form in [TESTING.md](../TESTING.md).

## Quick checks

```sh
lucubra --version
lucubra doctor
lucubra status -w <workspace>
```

| Check | Healthy output |
|---|---|
| `lucubra --version` | `lucubra X.Y.Z` |
| `lucubra doctor` | `ok       python -> <path>` as the first line. `missing  oda` and `missing  blender` are normal unless you have `.dwg` or `.blend` files |
| `lucubra status -w <workspace>` | `events skipped: 0` and `unreadable lines: 0` |

Real `doctor` output with the workers present and the optional tools absent:

```text
ok       python -> E:\TestEnvironments\lucubra-college-2026\venv\Scripts\python.exe
missing  oda (DWG converter (ODA File Converter or LibreDWG dwg2dxf))
         install: Install ODA File Converter (https://www.opendesign.com/guestfiles/oda_file_converter) or LibreDWG (`dwg2dxf`) and add it to PATH (or set LUCUBRA_ODA_PATH).
missing  blender (Blender (headless .blend reader))
         install: Install Blender (https://www.blender.org/download/) and add `blender` to PATH (or set LUCUBRA_BLENDER_PATH).
```

## Python worker not found

Symptoms:

```text
missing  python (Python worker interpreter)
         install: Install via `uv tool install lucubra` or set LUCUBRA_PYTHON to the venv Python.
```

```text
worker unavailable: no Python found next to lucubra; set LUCUBRA_PYTHON to the interpreter
extraction: 0 done now, 0 already indexed, 0 failed or pending earlier
waiting for a Python worker: 2 files; they are extracted on the next ingest with a worker
```

`ingest` exits with success but PDF, DOCX and PPTX files stay unread. The same state shows in a host as files with status pending and no search results.

Cause: Lucubra looks for `python.exe` (`python3` or `python` on macOS and Linux) in the folder of the executable it was started from, or in `LUCUBRA_PYTHON`. After `uv tool install` on Windows, the command on your path is a copy of the executable in a folder that has no interpreter. The hint printed by `doctor` suggests `uv tool install`, which is what caused the message, so ignore it.

Fix, either way:

1. Set `LUCUBRA_PYTHON` to the interpreter of the environment that holds Lucubra:

```powershell
$env:LUCUBRA_PYTHON = "$(uv tool dir)\lucubra\Scripts\python.exe"
lucubra doctor
```

   Expected: `ok       python -> ...\lucubra\Scripts\python.exe`. With the variable set, two test files extracted (`inbox/GST ANS.pdf` 18 chunks, `inbox/GST Question Bank.docx` 2 chunks).
2. Or use the executable inside the tool or virtual environment as the command, for example `$(uv tool dir)\lucubra\Scripts\lucubra.exe`.

For a host, put the variable in the host's `env` block (see below). On macOS and Linux the equivalent interpreter is `$(uv tool dir)/lucubra/bin/python`; this was not verified there.

A wrong path gives this message instead:

```text
worker unavailable: worker at E:\nope\python.exe did not answer a ping (worker_crashed: cannot start worker: The system cannot find the path specified. (os error 3))
```

Check the path, and that the interpreter has Lucubra's packages installed (`pypdfium2`, `python-docx`, `python-pptx`, `pdfplumber`, `ezdxf`, `pydicom`).

## A host starts the server with an empty environment

Some hosts and tools launch servers with a minimal environment, so variables set in your shell are lost. The MCP Inspector does this too. Symptoms: the server uses the wrong data directory (on Windows it silently writes to `%LOCALAPPDATA%\lucubra` instead of your configured one), or the worker is missing even though `doctor` works in your terminal.

Fix: put every `LUCUBRA_*` variable you rely on in the host's `env` setting, next to `command` and `args`. The variables are `LUCUBRA_DATA_DIR`, `LUCUBRA_PYTHON`, `LUCUBRA_ODA_PATH`, `LUCUBRA_LIBREDWG_PATH`, `LUCUBRA_BLENDER_PATH` and `LUCUBRA_LOG`. For the Inspector command line:

```sh
npx -y @modelcontextprotocol/inspector --cli -e LUCUBRA_DATA_DIR=<data dir> -e LUCUBRA_PYTHON=<python> <lucubra> mcp -w <workspace> --method tools/list
```

Use absolute paths. Data directory and workspace must be the same ones you used for `lucubra ingest`, because the search index lives in the data directory.

## Not a Lucubra workspace

```text
lucubra: E:\path\to\folder is not a Lucubra workspace; run `lucubra init` there first
```

Printed by every command that opens a workspace (`status`, `scan`, `ingest`, `search`, `project`, `reset`, `rebuild`, `mcp`, and `doctor -w`) with exit status 1. In a host this means `lucubra mcp` exits before it starts. In a host this appears as a server that fails to start or disconnects at once. Run `lucubra init -w <workspace>` and check that `-w` is an absolute path to the right folder.

## Search returns "results: 0"

`lucubra search` exits with success and prints only `results: 0` when:

1. The index is empty. Running `lucubra reset` or `lucubra rebuild` deletes the whole cache for the workspace, including the search index and rendered pages, and prints nothing about it. Run `lucubra ingest -w <workspace>` again. In testing, a reset followed by `search GST` printed `results: 0`; after `ingest` the same search printed `results: 2`.
2. The data directory changed. The index is looked up by workspace id inside `LUCUBRA_DATA_DIR`. A different data directory is an empty index.
3. The material is scanned. Photographs and scanned pages have no text until the assistant reads and transcribes them. In the verification folder, 185 of 305 files produced no searchable text at all, among them 136 of 137 files in one subject folder and both files in another. Search for those subjects returns nothing even though ingest succeeded. `lucubra_orient` reports these as `needs_vision_files`.
4. The word does not occur. Matching is by whole word by default. Try `--substring` (three or more characters per term) or a related word. `entrepreneur` returned 0 hits while `entrepreneurship` returned hits.

## Ingest takes a long time

An ingest of 305 files with 480 scanned pages took 98 to 108 seconds, in three runs. Most of that is rendering pages to images. Later runs skip files already indexed (`295 already indexed`, under a second). Run the first ingest in a terminal, not through the assistant: a host can give up on a tool call before it finishes.

## Files that are not read

`scan` prints a format per file. Files shown as `[unknown]` (in the test folder, one `.mp4` and six `.svg` files) are not extracted and are not reported as skipped. `ingest` prints nothing about them and `lucubra_orient` does not count them. Convert the content to PDF, DOCX or images to include it.

Revit (`.rvt`) and SketchUp (`.skp`) files cannot be read at all. Export a PDF, IFC, DWG or images and put the export in `inbox/`.

## Many "conflict-named file left in place" lines

`init` and `project` list every file whose name looks like a sync-conflict copy. Browser duplicate downloads (`report (1).pdf`) and names that end in a number in parentheses (`GST notes (06032026).pdf`) match. In the test folder that was 28 lines. They are warnings. Nothing is moved or deleted. The last line of the output (`state-hash: ...`) shows the command finished.

## Command line errors

| Message | Meaning |
|---|---|
| `lucubra: unknown argument 'foo'` followed by the command list | Mistyped command. Exit status 2 |
| `error: the following required arguments were not provided: <QUERY>...` | A required argument such as the search words is missing. Exit status 2 |
| `reset cancelled` | You did not type `reset` at the prompt. Use `--yes` to skip it |

## The host does not show Lucubra

1. Use an absolute path for `command` and for the workspace in `-w`.
2. Quit the host completely and start it again. Claude Desktop must be quit from the tray or menu bar, not only closed.
3. Check the JSON or TOML syntax. A stray comma or an unescaped backslash in a Windows path makes the host ignore the file.
4. Run the exact command from the host configuration in a terminal. It should wait silently for input. Press `Ctrl+C` to stop it. Any error text it prints is what the host sees.
5. Read the host's MCP log. Locations are in the host page. Add `"LUCUBRA_LOG": "debug"` to `env` for more detail on standard error.
6. Compare the host page's config key names with your file. Hosts differ: `mcpServers` for most, `servers` for older VS Code files, `[mcp_servers.<name>]` for Codex, `mcp_servers:` for Hermes.

## Responses are cut short

A response that does not fit its token budget ends with `"truncated": true` and a `next_offset`, and the assistant should ask for the next part. If you pay for your AI plan and still see short answers, the plan tier is probably Free. Claude Desktop, Codex, Antigravity, Cursor, Windsurf, VS Code and Hermes all start on Free. Set it once:

```json
{"name": "lucubra_profile", "arguments": {"action": "set", "field": "plan_tier", "value": "paid"}}
```

An invalid value returns `{"ok": false, "code": "invalid_arguments", "fix": "plan_tier must be one of: free, paid, local"}`. The budgets are 1,000, 2,000 and 25,000 tokens for orient, any response and the wrap-up nudge on Free; 1,500, 6,000 and 80,000 on Paid; 700, 1,500 and 12,000 on Local.

## Tool errors in a conversation

Failed tool calls return `"ok": false`, a `code` and a `fix` line that the assistant is told to follow. Example from a read with a wrong reference:

```text
{"ok":false,"code":"not_found","fix":"no file matches file_hash 'nope'; call lucubra_search or lucubra_ingest first", ...}
```

References have the form `F:<first 8 characters of the file hash>/p:<page>` (or `h:`, `s:`, `x:`), and come from `lucubra_search`. If the assistant invents a reference, ask it to search again.

## Inspector reports portability warnings

`tools/list --strict` in the MCP Inspector exits with status 6 and reports 7 errors and 112 warnings for the 11 tools (bare `true` item schemas and `"type": ["x", "null"]` arrays). Without `--strict` the Inspector prints a one-line summary on standard error and exits normally. These are known. They matter only for hosts that translate tool schemas into a stricter dialect. If a host connects but loses tools or arguments, say so in the host report.

## Disk space

The data directory held 419 MB of rendered page images for 480 scanned PDF pages, next to a 7 MB search index. Rendering happens in the data directory on your system drive by default. Move it with `LUCUBRA_DATA_DIR` before the first ingest, and set the same value for every host.

## Start over

`lucubra reset -w <workspace>` clears derived state and keeps your files and event log (see [getting started](getting-started.md#reset)). Run `lucubra ingest -w <workspace>` afterwards. To remove everything, delete `<workspace>/.lucubra/` and the data directory.
