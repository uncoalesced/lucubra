# Claude Desktop

| Item | Value |
|---|---|
| Host id reported by Lucubra | `claude-desktop` (the app announces itself as `claude-ai`) |
| Mode | Engine (local MCP server) |
| Default budget tier | Free |
| Elicitation forms | Treated as unsupported; onboarding questions are asked in conversation |
| Renders artifacts | Yes |
| Live-host status | Not verified on a live host. The server side was verified by a scripted stdio client that announces itself as `claude-ai` |

Local MCP servers work in Claude Desktop on Windows, macOS and Linux without a plan restriction. There are two ways to connect Lucubra: the `lucubra.mcpb` bundle, or a manual entry in `claude_desktop_config.json`.

## Before you start

1. Install Lucubra ([getting started](../getting-started.md)) and note the full path of the `lucubra` executable.
2. Create the workspace with `lucubra init -w <workspace>` and run `lucubra ingest -w <workspace>` once from a terminal. Ingesting hundreds of files takes minutes, longer than a chat tool call should run.
3. Use absolute paths everywhere. Claude Desktop starts servers from its own folder, so a relative workspace path points nowhere.

Find the executable path:

```powershell
(Get-Command lucubra).Source              # Windows
```

```sh
command -v lucubra                       # macOS, Linux
```

## Option A: the .mcpb bundle

Download `lucubra.mcpb` from the GitHub release page. In Claude Desktop open Settings, then Extensions, and install the file (dragging it into the window also works in current versions). The bundle asks for the workspace folder ("Study folder") and starts the server with `uv run`, so `uv` must be installed. The manifest does not create the workspace: initialise the folder first with `lucubra init -w <workspace>` from the command-line tool, otherwise the server exits with `is not a Lucubra workspace; run lucubra init there first`.

Not verified on a live Claude Desktop. If the bundle does not install, use option B and report it with the host-report form in [TESTING.md](../../TESTING.md).

## Option B: edit the config file

Open Claude Desktop, then Settings, Developer, Edit Config. The button opens the file the app actually reads, which matters on Windows because two locations exist.

| OS | File |
|---|---|
| Windows, installer from claude.ai | `%APPDATA%\Claude\claude_desktop_config.json` |
| Windows, Microsoft Store build | `%LOCALAPPDATA%\Packages\Claude_<package id>\LocalCache\Roaming\Claude\claude_desktop_config.json` |
| macOS | `~/Library/Application Support/Claude/claude_desktop_config.json` |
| Linux | `~/.config/Claude/claude_desktop_config.json` |

On a Windows machine with the Store build, both locations held a `claude_desktop_config.json`. Which one the app reads was not determined. Use Edit Config.

Add the server under `mcpServers`. If the file already has other servers, add only the `lucubra` entry.

```json
{
  "mcpServers": {
    "lucubra": {
      "command": "C:\\path\\to\\lucubra.exe",
      "args": ["mcp", "-w", "C:\\Users\\you\\Documents\\Study"],
      "env": {
        "LUCUBRA_PYTHON": "C:\\path\\to\\python.exe"
      }
    }
  }
}
```

macOS or Linux:

```json
{
  "mcpServers": {
    "lucubra": {
      "command": "/path/to/lucubra",
      "args": ["mcp", "-w", "/Users/you/Documents/Study"]
    }
  }
}
```

Notes on the snippet:

- Escape every backslash in JSON as `\\`, or use forward slashes.
- `LUCUBRA_PYTHON` is only needed when the executable is a launcher that is not in the same folder as the interpreter, as happens after `uv tool install` on Windows. If `command` points at the executable inside the tool or virtual environment, drop the `env` block. See [troubleshooting](../troubleshooting.md#python-worker-not-found).
- Claude Desktop passes a minimal environment to servers. Add `LUCUBRA_DATA_DIR` to `env` if you moved the data directory.
- The key names (`mcpServers`, `command`, `args`, `env`) match the current Model Context Protocol documentation for Claude Desktop, checked on 2026-11-23.

Quit Claude Desktop completely (the tray or menu bar icon, not only the window) and start it again.

## Verify the connection

1. In the message box open the "Add files, connectors, and more" menu, then Connectors. `lucubra` should be listed, with 11 tools.
2. Ask: "Call lucubra_orient and show me the result." A working connection returns `"ok": true`, your file counts, and `"host": {"id": "claude-desktop", ... "tier": "free"}`.
3. If the server is missing or errors, read the logs:

| OS | Log folder |
|---|---|
| Windows | `%APPDATA%\Claude\logs` (Store build: the same path under the package folder) |
| macOS | `~/Library/Logs/Claude` |
| Linux | `~/.config/Claude/logs` |

`mcp.log` records connection failures. `mcp-server-lucubra.log` holds the server's standard error. Set `"LUCUBRA_LOG": "debug"` in `env` for more detail.

## Free plan

Free is the default tier for Claude Desktop, so nothing needs configuring. The budgets are:

| Limit | Free |
|---|---|
| `lucubra_orient` response | 1,000 tokens |
| Any other single response | 2,000 tokens |
| Wrap-up nudge, tokens served in a session | 25,000 |

A response that does not fit is cut and marked `"truncated": true` with a `next_offset`, so the assistant reads the rest with a second call. On Free, plan for short sessions. When the served total reaches the wrap-up limit, the next tool result says `"next": "wrap_up"` and the assistant should close the session and write the handoff summary.

## Paid plan (Pro, Max, Team)

Lucubra cannot see which Claude plan you have, so a paid user is also given Free budgets. Responses are then capped at 2,000 tokens, which reads as "too short". Switch the tier once and it is stored in your workspace:

Natural language, in a Claude Desktop conversation:

> Use the Lucubra tool lucubra_profile to set my plan_tier to paid.

The exact tool call, for any host that lets you edit arguments:

```json
{"name": "lucubra_profile", "arguments": {"action": "set", "field": "plan_tier", "value": "paid"}}
```

Expected result: `{"ok": true, "data": {"updated": ["plan_tier"]}}`. The next `lucubra_orient` reports `"tier": "paid"` and a wrap-up limit of 80,000. Verified against the server with a client that announces itself as `claude-ai`: before the call the budget limit was 25,000, after it 80,000.

| Limit | Paid |
|---|---|
| `lucubra_orient` response | 1,500 tokens |
| Any other single response | 6,000 tokens |
| Wrap-up nudge | 80,000 |

Valid values are `free`, `paid` and `local`. The setting follows the workspace, not the app. To raise the tier for Claude Desktop only and leave other hosts alone, set `plan_tier_by_host` to `{"claude-desktop": "paid"}` through the same tool (`field` `plan_tier_by_host`).

## Known limits

- No forms: Claude Desktop is treated as not supporting MCP elicitation, so profile questions arrive as chat questions.
- A long first `lucubra_ingest` call can run past the app's tool timeout. Ingest from a terminal first.
- Scanned pages and photographed notes need the assistant to read page images. Free usage limits can run out before a large batch is transcribed.
- The `.mcpb` route and the Microsoft Store config location are not verified on a live host.
