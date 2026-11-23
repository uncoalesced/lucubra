<p align="center">
  <img src="docs/assets/banner.png" alt="Lucubra" width="800">
</p>

# Lucubra

Lucubra is a local-first study engine for students. It indexes a folder of course material (PDF, Office documents, spreadsheets, EPUB, CAD drawings, DICOM images and more) and exposes it to any AI assistant that supports the Model Context Protocol (MCP). The assistant reads only the parts of your material that a task needs, so long courses fit into a limited context window, and your files stay in the folder you chose.

> **Beta.** Lucubra 1.0.0-beta.1 is pre-release software for testing. Expect rough edges, and keep a copy of your original files. Report problems through the issue templates in this repository.

## Features

- **One folder, many sources.** Point Lucubra at a workspace folder. It scans the files, hashes them, extracts text from every format it supports and records what it could not read.
- **Searchable index.** Full-text search over extracted material, with results that cite the file and location.
- **Progressive reading.** An index file for the root, subject indexes and unit cards let an assistant move from overview to detail without loading everything.
- **Local event log.** Every change is recorded in plain-text JSONL files inside the workspace. You can read them, back them up and delete them.
- **Works with your assistant.** Lucubra speaks MCP over standard input and output. It adapts its response sizes to the plan tier of the host you connect.
- **Sync-friendly.** Each device writes its own log, so a workspace kept in a synced folder does not conflict on writes.

### Not yet available in this build

- Planner, study sessions, practice and exam simulation (`lucubra_plan`, `lucubra_practice`).
- Export of study files and flashcards (`lucubra_export`).
- Fetching from listed websites (`fetch` action of `lucubra_ingest`).
- Calendar reading, the optional vector search, and the companion page for hosts that cannot run a local server. These are planned and opt-in.

The empty crates `lucubra-pack` and `lucubra-companion` are reserved for the planned pack builder and companion page.

## Install

Lucubra requires Python 3.11 or later for its extraction workers, and it installs with [uv](https://docs.astral.sh/uv/).

```sh
uv tool install --prerelease allow lucubra
lucubra --version
```

Install scripts are in `packaging/`:

- Windows (PowerShell): `packaging/install.ps1`
- macOS and Linux: `packaging/install.sh`

For Claude Desktop, download `lucubra.mcpb` from the release page and open it. Claude Desktop installs the bundle as an extension. The bundle uses `uv` to fetch Lucubra and its dependencies.

Pre-built wheels and the bundle are attached to each GitHub pre-release.

## Quick start

1. Create a workspace and index a folder of material:

   ```sh
   lucubra init --workspace ~/Study/Biology
   lucubra scan --workspace ~/Study/Biology
   lucubra ingest --workspace ~/Study/Biology
   ```

2. Check what was indexed, and what could not be read:

   ```sh
   lucubra status --workspace ~/Study/Biology
   lucubra doctor --workspace ~/Study/Biology
   ```

3. Connect your AI host. Add Lucubra to the host's MCP configuration with the workspace path. Each host has its own page with the exact settings:

   | Host | Default plan tier | Host page |
   |---|---|---|
   | Claude Desktop | Free | [docs/hosts/claude-desktop.md](docs/hosts/claude-desktop.md) |
   | Claude Code | Paid | [docs/hosts/claude-code.md](docs/hosts/claude-code.md) |
   | Codex | Free | [docs/hosts/codex.md](docs/hosts/codex.md) |
   | Antigravity | Free | [docs/hosts/antigravity.md](docs/hosts/antigravity.md) |
   | Cursor | Free | [docs/hosts/cursor.md](docs/hosts/cursor.md) |
   | Windsurf | Free | [docs/hosts/windsurf.md](docs/hosts/windsurf.md) |
   | VS Code Copilot | Free | [docs/hosts/vscode-copilot.md](docs/hosts/vscode-copilot.md) |
   | LM Studio | Local | [docs/hosts/lm-studio.md](docs/hosts/lm-studio.md) |
   | Hermes Agent | Free | [docs/hosts/hermes-agent.md](docs/hosts/hermes-agent.md) |
   | Jan or Open WebUI | Local | [docs/hosts/jan-open-webui.md](docs/hosts/jan-open-webui.md) |
   | Any other MCP client | Free | [docs/hosts/generic.md](docs/hosts/generic.md) |

4. Ask your assistant to orient itself. It calls `lucubra_orient` first and reads the index before it reads any unit.

For the full walkthrough, see [docs/getting-started.md](docs/getting-started.md). If something fails, see [docs/troubleshooting.md](docs/troubleshooting.md).

## Plan tiers and response budgets

Lucubra limits the size of each response so that a host with a small context window or a free plan does not fill up. The limits depend on the tier:

| Tier | Orientation response | Other responses | Session total before wrap-up |
|---|---|---|---|
| Free | 1,000 tokens | 2,000 tokens | 25,000 tokens |
| Paid | 1,500 tokens | 6,000 tokens | 80,000 tokens |
| Local | 700 tokens | 1,500 tokens | 12,000 tokens |

Lucubra picks the tier from the host's name. If you pay for a plan that allows longer responses, set your tier in your profile with `lucubra_profile`. Each host page explains how to set the tier. Use this setting when responses are shorter than your plan allows.

## Privacy

Lucubra makes no network connections of its own. It has no telemetry and no update check. Everything it stores stays in your workspace folder and in one application-data directory on your machine.

The text of your material leaves the machine only when your AI host receives it through MCP. The host then decides where that content goes. Read [DATA_HANDLING.md](DATA_HANDLING.md) for every path Lucubra reads and writes, what can leave the machine, how long each item is kept and how to delete it. [PRIVACY.md](PRIVACY.md) is the short version.

## Documentation

- [Getting started](docs/getting-started.md)
- [Troubleshooting](docs/troubleshooting.md)
- [Testing](TESTING.md)
- [Data handling](DATA_HANDLING.md)
- [Architecture decision records](docs/adr/)
- [Accessibility](ACCESSIBILITY.md)
- [Security policy](SECURITY.md)
- [Terms](TERMS.md)
- [Changelog](CHANGELOG.md)

Some code comments refer to design documents (SPEC, HOST_MATRIX and BUILD_PLAN) that are kept outside this repository. The decision records in `docs/adr/` carry the decisions that matter to contributors.

## Contributing

Contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) for the development setup, the checks that CI runs and the commit conventions. Everyone taking part must follow the [Code of Conduct](CODE_OF_CONDUCT.md).

To report a bug or an incompatibility with an AI host, use the issue templates. To report a security problem, use private reporting described in [SECURITY.md](SECURITY.md).

## Licence

Lucubra is licensed under the [Apache License, Version 2.0](LICENSE). See [NOTICE](NOTICE) for attribution.
