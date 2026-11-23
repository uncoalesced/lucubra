# Hermes Agent

| Item | Value |
|---|---|
| Host id reported by Lucubra | `hermes-agent` |
| Mode | Engine (local MCP server). Reachable from a phone through Hermes's messaging gateway |
| Default budget tier | Free |
| Elicitation forms | Form mode supported by the host |
| Live-host status | Not verified on a live host |

Hermes Agent is free software from Nous Research. It runs on Linux, macOS, WSL2, native Windows and Android (Termux). Its messaging gateway (Telegram, WhatsApp, Discord, Signal and others) lets a phone talk to a Hermes instance running on your computer, which is the only phone route to Lucubra's Engine mode found so far. Hermes also reads `AGENTS.md` automatically.

## Before you start

Install Lucubra, create the workspace with `lucubra init -w <workspace>`, and run `lucubra ingest -w <workspace>` from a terminal ([getting started](../getting-started.md)). Use absolute paths.

## Configuration

Hermes reads `~/.hermes/config.yaml` (Windows: `%USERPROFILE%\.hermes\config.yaml`). MCP servers are a map named `mcp_servers`. On a Windows machine with Hermes installed, both `%USERPROFILE%\.hermes\config.yaml` and `%LOCALAPPDATA%\hermes\config.yaml` exist, and which one a given Hermes build reads was not determined. Edit the first, and if the server does not appear after `/reload-mcp`, check the second.

```yaml
mcp_servers:
  lucubra:
    command: "C:\\path\\to\\lucubra.exe"
    args: ["mcp", "-w", "C:\\Users\\you\\Documents\\Study"]
    env:
      LUCUBRA_PYTHON: "C:\\path\\to\\python.exe"
```

macOS, Linux or WSL2:

```yaml
mcp_servers:
  lucubra:
    command: "/path/to/lucubra"
    args: ["mcp", "-w", "/path/to/Study"]
```

Fields for a stdio server are `command`, `args`, `env`, optional `cwd`, `enabled` (set to `false` to skip the server) and `tools.include` or `tools.exclude` to filter tools. The key names were checked against the Hermes documentation on 2026-11-23. In double-quoted YAML a Windows path needs doubled backslashes, or use single quotes with single backslashes. `LUCUBRA_PYTHON` is only needed when the executable is a launcher separate from its interpreter, as after `uv tool install` on Windows ([troubleshooting](../troubleshooting.md#python-worker-not-found)).

From WSL2, `command` must be a Linux path. A Windows `lucubra.exe` can be started from WSL2 but then `-w` must be a Windows path. Keep both on the same side.

After editing, start a new session with `hermes chat`, or run `/reload-mcp` inside a running session.

## Verify the connection

1. Hermes discovers server tools at start and registers them like its own. The startup summary in `agent.log` names any server that failed to register, with the error.
2. `hermes mcp test lucubra` diagnoses a connection problem, and `hermes mcp configure lucubra` opens the tool checklist. The documentation mentions these commands but does not describe their output.
3. Ask: "Call lucubra_orient." A working connection returns `"ok": true`, your file counts, and `"host": {"id": "hermes-agent", ... "tier": "free"}`.

## Budgets and plan tier

Hermes defaults to the Free tier regardless of the model you connect. Model costs depend on your provider. If your provider has large context and no tight limits, raise the tier once:

> Use the Lucubra tool lucubra_profile to set my plan_tier to paid.

## Known limits

- Hermes has its own persistent memory and skills. Lucubra does not depend on them and does not read them.
- Two configuration locations exist on Windows (see above).
- Exports are returned as file paths or summaries and sent through whichever gateway is active.
