# Claude Code

| Item | Value |
|---|---|
| Host id reported by Lucubra | `claude-code` |
| Mode | Engine (local MCP server) |
| Default budget tier | Paid |
| Elicitation forms | Supported by the host; Lucubra uses them when the host declares support |
| Pointer file | `CLAUDE.md` is written for this host |
| Live-host status | Not verified on a live host. The server side was verified by a scripted stdio client that announces itself as `claude-code` |

Claude Code is a coding surface. It works with Lucubra but is not the main target audience.

## Before you start

Install Lucubra, create the workspace with `lucubra init -w <workspace>`, and run `lucubra ingest -w <workspace>` from a terminal ([getting started](../getting-started.md)). Use absolute paths.

## Configuration

### With the command line

```sh
claude mcp add --transport stdio --scope user lucubra -- /path/to/lucubra mcp -w /path/to/Study
```

Windows PowerShell:

```powershell
claude mcp add --transport stdio --scope user lucubra -- C:\path\to\lucubra.exe mcp -w C:\Users\you\Documents\Study
```

- `--` separates Claude Code's own options from the server command. Everything after it goes to Lucubra unchanged.
- `--scope user` makes the server available in every project and stores it in `~/.claude.json`. The default scope, `local`, applies only to the project you are in. `--scope project` writes `.mcp.json` in the project root so a team can share it.
- To pass an environment variable, add `--env KEY=value` before the name but not directly in front of it. Put `--transport stdio` between them:

```sh
claude mcp add --env LUCUBRA_PYTHON=/path/to/python --transport stdio --scope user lucubra -- /path/to/lucubra mcp -w /path/to/Study
```

### With the Claude Code plugin

The repository carries a plugin package in `packaging/claude-code-plugin`. Its `.mcp.json` runs `uvx --prerelease allow lucubra mcp` without a `-w` argument, so the study folder is the folder Claude Code was started in. Start Claude Code from a workspace that has already been initialised with `lucubra init`. The plugin was not installed for this guide, so this route is Not verified on a live host.

### With a .mcp.json file

Place `.mcp.json` in the project root:

```json
{
  "mcpServers": {
    "lucubra": {
      "command": "/path/to/lucubra",
      "args": ["mcp", "-w", "/path/to/Study"],
      "env": {
        "LUCUBRA_PYTHON": "/path/to/python"
      }
    }
  }
}
```

On Windows write paths as `C:\\Users\\you\\...` or with forward slashes. Claude Code asks you to approve project-scoped servers the first time it sees them. `claude mcp reset-project-choices` clears those answers.

Key names and flags were checked against the current Claude Code documentation on 2026-11-23. Not run on a live host. `LUCUBRA_PYTHON` is only needed when the executable is a launcher separate from its interpreter, as after `uv tool install` on Windows ([troubleshooting](../troubleshooting.md#python-worker-not-found)).

## Verify the connection

```sh
claude mcp list
```

`lucubra` should show a connected status. `claude mcp get lucubra` prints the stored command. Inside a session, `/mcp` shows live status. Then ask: "Call lucubra_orient." A working connection returns `"ok": true`, your file counts, and `"host": {"id": "claude-code", ... "tier": "paid"}`.

If the status is a failure, run the command from the config by hand (`lucubra mcp -w <workspace>`); a workspace that was never initialised ends with `is not a Lucubra workspace; run lucubra init there first`.

## Budgets

Claude Code defaults to the Paid tier: 1,500 tokens for `lucubra_orient`, 6,000 for any other response, wrap-up nudge at 80,000 served tokens. If you use Claude Code on a free or API-limited setup and want smaller responses, set the tier with `lucubra_profile` (`action` `set`, `field` `plan_tier`, `value` `free`).

## Known limits

- Aimed at developers. Output formats such as the study brief are plain files in the workspace, not rich artifacts.
- No live verification of the `mcp add` flags or `.mcp.json` approval flow was possible in this beta cycle.
