# VS Code with GitHub Copilot

| Item | Value |
|---|---|
| Host id reported by Lucubra | `vscode-copilot` (matched from the client names `vscode`, `Visual Studio Code` or `copilot`) |
| Mode | Engine (local MCP server) |
| Default budget tier | Free |
| Elicitation forms | Form and URL modes supported by the host |
| Live-host status | Not verified on a live host. VS Code's MCP configuration is changing, and the two documented formats are both given below |

VS Code has the broadest MCP specification coverage of the editors Lucubra supports. Copilot has a free tier. The host matrix lists local stdio MCP as working on the free tiers of VS Code, Cursor and Windsurf (checked 2026-11-14) but this was not tested.

## Before you start

Install Lucubra, create the workspace with `lucubra init -w <workspace>`, and run `lucubra ingest -w <workspace>` from a terminal ([getting started](../getting-started.md)). Use absolute paths. Open the study workspace folder in VS Code so that `AGENTS.md` and the files are in scope.

## Configuration

### Editor-managed file (long-standing format)

| Scope | File |
|---|---|
| One workspace | `.vscode/mcp.json` |
| All workspaces | The user profile's `mcp.json`. Run "MCP: Open User Configuration" from the Command Palette. On Windows the file is `%APPDATA%\Code\User\mcp.json`, and one exists on a machine with VS Code installed |

The top-level key in this format is `servers`, not `mcpServers`.

```json
{
  "servers": {
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
  "servers": {
    "lucubra": {
      "type": "stdio",
      "command": "/path/to/lucubra",
      "args": ["mcp", "-w", "/path/to/Study"]
    }
  }
}
```

You can also run "MCP: Add Server" from the Command Palette and choose Command (stdio).

### Newer Copilot format

The current VS Code documentation describes a newer format and calls the `servers` format above deprecated for new Copilot configurations (the local harness is announced for removal in February 2027). The newer files use a top-level `mcpServers` key: `.mcp.json` in the workspace root (behind the experimental setting `chat.mcp.workspaceRootConfig.enabled`), or `~/.copilot/mcp-config.json` for the user.

```json
{
  "mcpServers": {
    "lucubra": {
      "type": "stdio",
      "command": "/path/to/lucubra",
      "args": ["mcp", "-w", "/path/to/Study"],
      "tools": ["*"]
    }
  }
}
```

Not verified on a live host. Use whichever format your VS Code version shows in the "MCP: Open User Configuration" file. `LUCUBRA_PYTHON` is only needed when the executable is a launcher separate from its interpreter, as after `uv tool install` on Windows ([troubleshooting](../troubleshooting.md#python-worker-not-found)).

## Verify the connection

1. Run "MCP: List Servers" from the Command Palette. `lucubra` should appear; choose Start Server if it is stopped, and Show Output to read its log.
2. Open Copilot Chat in agent mode and check that Lucubra's tools are listed under the tools control.
3. Ask: "Call lucubra_orient." Expand the tool call to confirm it ran. A working connection returns `"ok": true`, your file counts, and `"host": {"id": "vscode-copilot", ... "tier": "free"}`. A plausible answer without a visible tool call does not prove the connection.

## Budgets and plan tier

VS Code defaults to the Free tier. On a paid Copilot plan, set `plan_tier` to `paid` once:

> Use the Lucubra tool lucubra_profile to set my plan_tier to paid.

## Known limits

- Two configuration formats are in circulation; a file in the wrong format is silently ignored by some versions.
- Copilot asks for confirmation before running a tool unless you allow it for the session.
- Free-tier Copilot usage limits apply to the chat that calls Lucubra.
