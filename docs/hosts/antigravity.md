# Antigravity

| Item | Value |
|---|---|
| Host id reported by Lucubra | `antigravity` |
| Mode | Engine (local MCP server) |
| Default budget tier | Free |
| Elicitation forms | Not documented by the host; onboarding questions are asked in conversation |
| Renders artifacts | Yes |
| Pointer file | `GEMINI.md` is written for this host (Antigravity also reads `AGENTS.md`) |
| Live-host status | Not verified on a live host |

Antigravity and Antigravity CLI replaced Gemini CLI for Free, Pro and Ultra consumer accounts on 2026-06-18. This is the Google route to Engine mode, including outside the United States where the Gemini app cannot add custom servers.

## Before you start

Install Lucubra, create the workspace with `lucubra init -w <workspace>`, and run `lucubra ingest -w <workspace>` from a terminal ([getting started](../getting-started.md)). Use absolute paths.

## Configuration

| Scope | File |
|---|---|
| Global, app, CLI and IDE | `~/.gemini/config/mcp_config.json` (Windows: `%USERPROFILE%\.gemini\config\mcp_config.json`) |
| One workspace | `<workspace>/.agents/mcp_config.json` |

In the app, the MCP Servers section has a Copy Config File Path button that shows the file in use. On a Windows machine with Antigravity installed, `%USERPROFILE%\.gemini\config\mcp_config.json` exists.

The file holds one JSON object with an `mcpServers` map:

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

Supported fields for a local server are `command`, `args`, `env` and `cwd`. Antigravity supports stdio and Streamable HTTP transports. The key names were checked against the Antigravity MCP documentation on 2026-11-23. `LUCUBRA_PYTHON` is only needed when the executable is a launcher separate from its interpreter, as after `uv tool install` on Windows ([troubleshooting](../troubleshooting.md#python-worker-not-found)).

You can also add the entry from the interface: in the IDE open the "..." menu of the agent panel, choose MCP Servers, then Manage MCP Servers and View raw config. In the CLI, type `/mcp`.

## Verify the connection

- App: the server shows a green status dot and a tool count (11 for Lucubra).
- CLI: `/mcp` shows live status and connection logs.
- IDE: the MCP Servers panel lists `lucubra` as connected.

Then ask: "Call lucubra_orient." A working connection returns `"ok": true`, your file counts, and `"host": {"id": "antigravity", ... "tier": "free"}`.

## Budgets and plan tier

Antigravity defaults to the Free tier. On Pro or Ultra, set `plan_tier` to `paid` once:

> Use the Lucubra tool lucubra_profile to set my plan_tier to paid.

## Known limits

- Schema portability risk. The Lucubra tool schemas use JSON Schema forms (`"type": ["string", "null"]` and bare `true` item schemas) that hosts translating tool schemas into the stricter OpenAPI-style function format used by some Google tools may map imperfectly. An MCP Inspector run in strict mode reports 7 errors and 112 warnings for the 11 tools. Whether Antigravity rejects or drops any of them has not been tested. If the server connects but some tools are missing or arguments are ignored, report it with the host-report form in [TESTING.md](../../TESTING.md).
- Legacy HTTP+SSE and the `httpUrl` field are not supported by the host. Lucubra uses stdio, which is unaffected.
- Elicitation is not documented for this host.
