# ADR 0002: Claude Desktop bundle type

- Status: Accepted, with a runtime check outstanding
- Date: 2026-11-13
- Decision references: D15, SPEC section 4.3, addendum A8

## Context

Claude Desktop installs MCP Bundles (`.mcpb`). SPEC section 4.3 chose a bundle with `server.type = "uv"` and named a per-platform `binary` bundle as the fallback. It also described the `uv` type as experimental.

## Evidence

Read on 2026-11-12 and 2026-11-13:

- The MCPB manifest specification (`modelcontextprotocol/mcpb`, `MANIFEST.md`) defines `uv` as a Python server type for manifest version 0.4 and later. It does not describe the type as experimental. A `uv` bundle must contain `pyproject.toml` and must not contain `server/lib/` or `server/venv/`. The host installs Python and the dependencies with uv.
- The specification states that `mcp_config` is optional for `uv`. The `@anthropic-ai/mcpb` command line tool, version 2.1.2, rejects a `uv` manifest without it (`server.mcp_config: Required`). With `mcp_config` set to `uv run --directory ${__dirname} src/server.py`, the manifest validates and packs.
- The Claude Help Center article on desktop extensions (10949351) states that extensions from the official directory update automatically by default, and that privately distributed `.mcpb` files must be reinstalled manually. It does not state which plans may install extensions.
- A spike bundle (`lucubra-mcpb-spike-0.0.1.mcpb`, 1,086 bytes) was built. Run with `uv` as a host would, it answered `initialize`, listed its tool and returned a tool result over stdio.

Not tested: installation of the spike bundle in Claude Desktop on Windows or macOS, and installation on a Free account.

## Decision

1. The primary bundle uses `server.type = "uv"` with manifest version 0.4. Its `pyproject.toml` depends on the `lucubra` package from PyPI, and its manifest always sets `mcp_config`, which satisfies both the specification and the current validator.
2. The fallback remains a `binary` bundle per platform. It is built in M9 only if the `uv` bundle fails the runtime check.
3. Lucubra contains no update check (D43). Bundle updates reach users through the official directory when listed there, and through a new `.mcpb` file otherwise.

## Consequences

- The runtime check on Claude Desktop, including a Free account, stays open and is required before M9 closes.
- If the validator and the specification disagree again in a later release, the validator wins, because it is what authors and the directory run.
