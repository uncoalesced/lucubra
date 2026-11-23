# Codex

| Item | Value |
|---|---|
| Host id reported by Lucubra | `codex` |
| Mode | Engine (local MCP server) |
| Default budget tier | Free |
| Elicitation forms | Supported by the host |
| Applies to | Codex CLI, the Codex IDE extension, and the Codex tab in the ChatGPT desktop app. All three read the same `config.toml` |
| Live-host status | Not verified on a live host |

ChatGPT web and mobile chats do not read the Codex configuration. They cannot run a local server, so this page does not apply to them.

## Before you start

Install Lucubra, create the workspace with `lucubra init -w <workspace>`, and run `lucubra ingest -w <workspace>` from a terminal ([getting started](../getting-started.md)). Use absolute paths.

## Configuration

Codex reads servers from the `[mcp_servers.<name>]` tables of its configuration file.

| Scope | File |
|---|---|
| User, all OS | `~/.codex/config.toml` (Windows: `%USERPROFILE%\.codex\config.toml`) |
| Project, trusted projects only | `.codex/config.toml` in the project |

On a Windows machine that has Codex installed, `%USERPROFILE%\.codex\config.toml` exists, which agrees with the path above.

Add this table. Use single quotes for Windows paths so that backslashes are not treated as escapes:

```toml
[mcp_servers.lucubra]
command = 'C:\path\to\lucubra.exe'
args = ["mcp", "-w", 'C:\Users\you\Documents\Study']

[mcp_servers.lucubra.env]
LUCUBRA_PYTHON = 'C:\path\to\python.exe'
```

macOS or Linux:

```toml
[mcp_servers.lucubra]
command = "/path/to/lucubra"
args = ["mcp", "-w", "/path/to/Study"]
```

Key names (`command`, `args`, `env`, optional `cwd`, `env_vars`) match the current Codex documentation, checked on 2026-11-23. The equivalent command line is:

```sh
codex mcp add lucubra --env LUCUBRA_PYTHON=/path/to/python -- /path/to/lucubra mcp -w /path/to/Study
```

`LUCUBRA_PYTHON` is only needed when the executable is a launcher separate from its interpreter, as after `uv tool install` on Windows ([troubleshooting](../troubleshooting.md#python-worker-not-found)).

Restart Codex after editing the file.

## Verify the connection

```sh
codex mcp list
```

`lucubra` should be listed. In the Codex terminal interface, `/mcp` shows the active servers. In the ChatGPT desktop app, type `/mcp` in the composer or open Settings and look for MCP servers. Then ask: "Call lucubra_orient." A working connection returns `"ok": true`, your file counts, and `"host": {"id": "codex", ... "tier": "free"}`.

## Budgets and plan tier

Codex defaults to the Free tier (2,000 tokens per response, wrap-up nudge at 25,000). On a paid ChatGPT plan, raise the tier once:

> Use the Lucubra tool lucubra_profile to set my plan_tier to paid.

or call `lucubra_profile` with `{"action": "set", "field": "plan_tier", "value": "paid"}`.

## Known limits

- Access to Codex on a free ChatGPT account is limited. The CLI and the IDE extension need a paid plan or an API key, and the Codex tab in the desktop app has limited free access (HOST_MATRIX, checked 2026-11-14).
- Codex is built for code. Expect it to prefer file edits over study dialogue unless you ask for study tasks.
- Not verified: Windows path handling for `command` and the behaviour of the project-scoped `.codex/config.toml` for MCP entries.
