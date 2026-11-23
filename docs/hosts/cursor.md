# Cursor

| Item | Value |
|---|---|
| Host id reported by Lucubra | `cursor` |
| Mode | Engine (local MCP server) |
| Default budget tier | Free |
| Elicitation forms | Form mode supported; a rendering bug in the macOS Agents window was reported |
| Live-host status | Not verified on a live host |

Cursor reads `AGENTS.md` automatically, so the workspace index that Lucubra generates is visible to it when the workspace is the open folder.

## Before you start

Install Lucubra, create the workspace with `lucubra init -w <workspace>`, and run `lucubra ingest -w <workspace>` from a terminal ([getting started](../getting-started.md)). Use absolute paths.

## Configuration

| Scope | File |
|---|---|
| All projects | `~/.cursor/mcp.json` (Windows: `%USERPROFILE%\.cursor\mcp.json`) |
| One project | `.cursor/mcp.json` in the project |

On a Windows machine with Cursor installed, `%USERPROFILE%\.cursor\mcp.json` exists. If you already have servers in it, add only the `lucubra` entry.

```json
{
  "mcpServers": {
    "lucubra": {
      "type": "stdio",
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
      "type": "stdio",
      "command": "/path/to/lucubra",
      "args": ["mcp", "-w", "/path/to/Study"]
    }
  }
}
```

`command` must be on your path or be a full path. The key names were checked against the current Cursor documentation on 2026-11-23; the documentation's field table lists `type` as required for stdio servers while its examples omit it, so the snippet includes it. `LUCUBRA_PYTHON` is only needed when the executable is a launcher separate from its interpreter, as after `uv tool install` on Windows ([troubleshooting](../troubleshooting.md#python-worker-not-found)).

Cursor picks up the file when you save it. If the server does not appear, restart Cursor.

## Verify the connection

1. Open the MCP list from Customize in the sidebar. `lucubra` should be listed and can be toggled on and off.
2. To read startup errors, open the Output panel (`Ctrl+Shift+U`, or `Cmd+Shift+U` on macOS) and pick "MCP Logs".
3. In a chat, ask: "Call lucubra_orient." A working connection returns `"ok": true`, your file counts, and `"host": {"id": "cursor", ... "tier": "free"}`.

## Budgets and plan tier

Cursor defaults to the Free tier. On a paid Cursor plan, set `plan_tier` to `paid` once:

> Use the Lucubra tool lucubra_profile to set my plan_tier to paid.

## Known limits

- Whether MCP is available on the free Cursor plan was not confirmed. The documentation states no plan restriction for stdio servers, but team and enterprise administrators can restrict servers with an allowlist.
- Cursor is a code editor. Open the study workspace as the project folder so that `AGENTS.md` and the files are in scope.
- Tool approval prompts appear for each call unless you allow the tool.
