# Jan and Open WebUI

| Item | Value |
|---|---|
| Host id reported by Lucubra | `jan-open-webui` when the client name is `jan`, starts with `jan-` or `jan `, or contains `open-webui` |
| Mode | Engine if the MCP connection works |
| Default budget tier | Local |
| Elicitation forms | Open WebUI supports them; Jan does not document them |
| Live-host status | Not verified on a live host |

Jan connects to stdio servers directly. Open WebUI supports only Streamable HTTP MCP natively, so a stdio server such as Lucubra needs a proxy between them. Use Jan if you can; use Open WebUI only if you already run it.

## Before you start

Install Lucubra, create the workspace with `lucubra init -w <workspace>`, and run `lucubra ingest -w <workspace>` from a terminal ([getting started](../getting-started.md)). Use absolute paths.

## Jan

Jan takes the server through a form, not a config file.

1. Open Settings, then MCP Servers. Turn on "Allow All MCP Tool Permissions" only if you accept running tools without a prompt for each call.
2. Click `+` in the upper-right corner of the MCP box and fill in:

| Field | Value |
|---|---|
| Server Name | `lucubra` |
| Command | Full path of `lucubra` (Windows: `C:\path\to\lucubra.exe`) |
| Arguments | `mcp -w C:\Users\you\Documents\Study` (one argument per entry if the form splits them: `mcp`, `-w`, the workspace path) |
| Environment Variables | `LUCUBRA_PYTHON` = path to the interpreter, only if needed ([troubleshooting](../troubleshooting.md#python-worker-not-found)) |

3. Choose a model that supports tool calling. The model picker shows a "tools" badge. For a local model, turn on tool calling through the edit button in Model Capabilities.

Verify: the server shows as activated in the MCP Servers list. Ask: "Call lucubra_orient." Jan shows the tool call as a card that you approve inline. A working connection returns `"ok": true`, your file counts, and `"host": {"id": "jan-open-webui", ... "tier": "local"}`.

The field names come from the Jan documentation, checked on 2026-11-23. Jan keeps these settings in its own app data, and the file location is not documented.

## Open WebUI

Run the `mcpo` proxy, which exposes a stdio MCP server as an OpenAPI endpoint:

```sh
uvx mcpo --port 8000 --api-key "choose-a-secret" -- /path/to/lucubra mcp -w /path/to/Study
```

Windows:

```powershell
uvx mcpo --port 8000 --api-key "choose-a-secret" -- C:\path\to\lucubra.exe mcp -w C:\Users\you\Documents\Study
```

Leave it running. Open `http://localhost:8000/docs` to see the generated API. Then in Open WebUI, as an administrator, open Settings, Integrations, External Tool Servers, click Add Connection, and enter `http://localhost:8000` as an OpenAPI connection with the same API key. The Open WebUI documentation says to add an mcpo endpoint as an OpenAPI connection, not as an MCP connection.

Config-file mode runs several servers behind one proxy: `mcpo --config /path/to/config.json`, where the file has an `mcpServers` object with `command` and `args`, and each server appears at `http://localhost:8000/<name>`.

Not verified on a live host. In particular the proxy connects to Lucubra under its own client name, so Lucubra most likely detects the generic profile, which gets the Free tier instead of Local. If responses are larger than your model handles, set the tier yourself:

> Use the Lucubra tool lucubra_profile to set my plan_tier to local.

or call `lucubra_profile` with `{"action": "set", "field": "plan_tier", "value": "local"}`.

## Known limits

- Open WebUI needs the proxy, and the proxy ties the stdio server's life to its own process. Restart `mcpo` after editing the workspace path.
- An OpenAPI connection exposes tools, so Lucubra's prompts (`onboard`, `study`, `plan`, `quiz`, `wrap-up`) are probably not offered. Not verified. Ask for the same steps in plain language.
- Tool-calling quality depends on the model, as for LM Studio.
- Local models are slow at reading page images.
