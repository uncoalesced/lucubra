# Any other MCP host

| Item | Value |
|---|---|
| Host id reported by Lucubra | `generic` for any client name Lucubra does not recognise |
| Mode | Engine (local MCP server) |
| Default budget tier | Free |
| Live-host status | The stdio transport was verified with the MCP Inspector 2.13.0 command-line client and with a scripted client |

Any application that can start a local MCP server over stdio can use Lucubra. The profile is chosen from the client name the application sends when it connects. A client that Lucubra does not recognise gets the generic profile with the Free tier. A host that is not listed in the other pages but is a known product, for example one added after this beta, is still handled this way.

## The server command

```text
lucubra mcp -w <absolute path of the workspace>
```

- Transport: stdio. Lucubra writes protocol messages to standard output and logs to standard error. Set `LUCUBRA_LOG=debug` for more detail.
- The workspace must already exist (`lucubra init -w <workspace>`). Otherwise the process exits with `is not a Lucubra workspace; run lucubra init there first`.
- Use a full path to the `lucubra` executable and an absolute workspace path. Hosts start servers from folders you do not control.
- Environment: pass `LUCUBRA_PYTHON` when the executable is a launcher that is not beside its interpreter, and `LUCUBRA_DATA_DIR` if you moved the data directory. Many hosts start servers with an empty environment, so these belong in the host's own `env` setting. See [troubleshooting](../troubleshooting.md#a-host-starts-the-server-with-an-empty-environment).

## The common configuration shape

Most hosts accept some form of this JSON. The top-level key (`mcpServers` or `servers`) and whether `type` is required differ between hosts, so check your host's documentation.

```json
{
  "mcpServers": {
    "lucubra": {
      "type": "stdio",
      "command": "/path/to/lucubra",
      "args": ["mcp", "-w", "/path/to/Study"],
      "env": {
        "LUCUBRA_PYTHON": "/path/to/python"
      }
    }
  }
}
```

## Verify without a host

The MCP Inspector command-line client talks to the server directly. Pass the environment variables with `-e`, because the Inspector does not hand your shell's environment to the server:

```sh
npx -y @modelcontextprotocol/inspector --cli -e LUCUBRA_DATA_DIR=/path/to/data /path/to/lucubra mcp -w /path/to/Study --method tools/list
npx -y @modelcontextprotocol/inspector --cli -e LUCUBRA_DATA_DIR=/path/to/data /path/to/lucubra mcp -w /path/to/Study --method tools/call --tool-name lucubra_orient
```

`tools/list` returns 11 tools: `lucubra_orient`, `lucubra_profile`, `lucubra_ingest`, `lucubra_read`, `lucubra_search`, `lucubra_submit`, `lucubra_plan`, `lucubra_session`, `lucubra_practice`, `lucubra_export`, `lucubra_status`. The Inspector also lists the five prompts with `--method prompts/list`.

## Budget tiers

Set the tier that matches your plan or model with `lucubra_profile` (`{"action": "set", "field": "plan_tier", "value": "free"}`). `free` caps responses at 2,000 tokens, `paid` at 6,000, `local` at 1,500. The three tiers are described in [Claude Desktop](claude-desktop.md#free-plan).

## Known limits

- Hosts that cannot run a local process (web chat, phone apps) cannot use the stdio server at all.
- Without elicitation support the assistant asks profile questions in chat.
- A host that renders no artifacts gets plain files in the workspace instead of rich output.
