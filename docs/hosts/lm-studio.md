# LM Studio

| Item | Value |
|---|---|
| Host id reported by Lucubra | `lm-studio` |
| Mode | Engine, fully private when the model is local |
| Default budget tier | Local |
| Elicitation forms | Not documented by the host; onboarding questions are asked in conversation |
| Minimum host version | 0.3.17 (build 10), the first version with MCP support |
| Live-host status | Not verified on a live host. The server side was verified by a scripted stdio client that announces itself as `lm-studio` |

With a local model, nothing Lucubra reads leaves your computer. If you pick a cloud model inside LM Studio, the material goes to that provider like any other host.

## Before you start

Install Lucubra, create the workspace with `lucubra init -w <workspace>`, and run `lucubra ingest -w <workspace>` from a terminal ([getting started](../getting-started.md)). Use absolute paths.

Choose a model that supports tool calling. Small local models call tools unreliably, and the quality of Lucubra's output depends on the model. Give the model as large a context window as your memory allows; the Local tier assumes a small window and keeps responses short.

## Configuration

Open the Program tab in the right-hand sidebar, choose Install, then Edit mcp.json. The app opens the file in its editor. LM Studio uses the same `mcp.json` notation as Cursor, so the entry is an `mcpServers` map.

The file is normally `~/.lmstudio/mcp.json` (Windows: `%USERPROFILE%\.lmstudio\mcp.json`). The documentation page for the feature does not state the location, so use Edit mcp.json rather than creating the file yourself.

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

If the file already has an `mcpServers` object, copy only the `"lucubra": { ... }` entry into it. The stdio form with `command`, `args` and `env` was not shown on the LM Studio documentation page, which only documents a remote server; it follows the Cursor convention that the page names. Not verified on a live host. `LUCUBRA_PYTHON` is only needed when the executable is a launcher separate from its interpreter, as after `uv tool install` on Windows ([troubleshooting](../troubleshooting.md#python-worker-not-found)).

## Verify the connection

1. In a chat, check that `lucubra` appears among the enabled MCP servers and that its tools are available to the model.
2. Ask: "Call lucubra_orient." LM Studio shows the tool call and may ask you to approve it. A working connection returns `"ok": true`, your file counts, and `"host": {"id": "lm-studio", ... "tier": "local"}`.

## Budgets

LM Studio is assigned the Local tier, the smallest budgets:

| Limit | Local |
|---|---|
| `lucubra_orient` response | 700 tokens |
| Any other single response | 1,500 tokens |
| Wrap-up nudge | 12,000 |

If you run a model with a large context and want fuller responses, set `plan_tier` to `free` or `paid` with `lucubra_profile` (`{"action": "set", "field": "plan_tier", "value": "free"}`). Note that setting `local_model` to `true` in the profile forces the Local tier on every host.

## Known limits

- Tool-calling quality depends on the model. Models that ignore the tool list will never call Lucubra. Try a larger or tool-tuned model before reporting a Lucubra bug.
- Servers written for cloud models can fill a small context quickly. Lucubra keeps responses short for that reason, but long study sessions still need the wrap-up step.
- LM Studio reads no workspace instruction file, so the assistant learns about Lucubra only through the server's instructions and tool descriptions.
- Local models are slow at reading page images, so scanned notes are impractical unless the model is multimodal.
