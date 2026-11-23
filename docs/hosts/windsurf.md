# Windsurf

| Item | Value |
|---|---|
| Host id reported by Lucubra | `windsurf` (also matched from the client name `codeium`) |
| Mode | Engine (local MCP server) |
| Default budget tier | Free |
| Elicitation forms | Not documented by the host |
| Live-host status | Not verified on a live host. The configuration location and snippet below are not confirmed against current documentation |

Windsurf reads `AGENTS.md` automatically when the workspace is the open folder.

The Windsurf documentation address now redirects to a different product's documentation, and the page it lands on does not describe the Windsurf configuration file. The path and format below come from earlier Windsurf documentation and the shared `mcpServers` convention. Treat them as Not verified on a live host and use the in-app route first.

## Before you start

Install Lucubra, create the workspace with `lucubra init -w <workspace>`, and run `lucubra ingest -w <workspace>` from a terminal ([getting started](../getting-started.md)). Use absolute paths.

## Configuration

Preferred route: in Windsurf open the Cascade panel, choose the MCP servers control (the plug or hammer icon) and use View raw config, or open Windsurf Settings and search for "MCP". The app opens the file it reads.

Expected file:

| OS | File |
|---|---|
| Windows | `%USERPROFILE%\.codeium\windsurf\mcp_config.json` |
| macOS, Linux | `~/.codeium/windsurf/mcp_config.json` |

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
      "args": ["mcp", "-w", "/path/to/Study"]
    }
  }
}
```

`LUCUBRA_PYTHON` is only needed when the executable is a launcher separate from its interpreter, as after `uv tool install` on Windows ([troubleshooting](../troubleshooting.md#python-worker-not-found)). After saving, use the refresh control of the MCP panel or restart Windsurf.

## Verify the connection

1. The MCP panel should list `lucubra` as enabled with 11 tools.
2. In Cascade, ask: "Call lucubra_orient." A working connection returns `"ok": true`, your file counts, and `"host": {"id": "windsurf", ... "tier": "free"}`.

## Budgets and plan tier

Windsurf defaults to the Free tier. On a paid Windsurf plan, set `plan_tier` to `paid` once:

> Use the Lucubra tool lucubra_profile to set my plan_tier to paid.

## Known limits

- Local stdio MCP works on the free tier according to the host matrix (checked 2026-11-14), but this was not tested.
- Windsurf has limited the total number of tools it will load across all servers. Lucubra adds 11. If other servers are installed and Lucubra tools are missing, disable servers you do not need.
- Elicitation support is not documented.
