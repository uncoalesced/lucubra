# ADR 0012: MCP server architecture, raw argument validation and response budgets

- Status: Accepted
- Date: 2026-11-19
- Decision references: D09, D12, D14, D17, D29, D44, SPEC sections 4.1, 4.4, 7, 8, 15, 22, scenarios S10 to S12, S23, S29, S37

## Context

Milestone M3 implements `lucubra-mcp` and the `lucubra mcp` CLI entry point. Verification against crates.io and upstream `rmcp` tag `rmcp-v3.11.0` surfaced three constraints that shape the server design:

1. `rmcp` 3.11.0 uses `ServerConfig` (an alias for `InitializeResult`) instead of the deprecated `ServerInfo` alias, and uses native async trait methods without `async-trait`.
2. `rmcp` 3.11.0 defaults `ProtocolVersion::LATEST` to `2026-07-28`, which removes the `initialize` handshake in favour of per-request `_meta` and disallows server-to-client requests such as `peer.elicit()`. `ProtocolVersion::LATEST_WITH_INITIALIZE` is `2025-11-25`.
3. `rmcp`'s built-in `ToolRouter` combined with `Parameters<T>` intercepts `serde` deserialization errors and converts them into plain-text tool errors (`"failed to deserialize parameters: ..."`), bypassing Lucubra's structured `{ok: false, code, fix}` envelope required for weak tool-callers (SPEC section 8.2, S23).

## Decision

1. **SDK pin and feature set.** Depend on `rmcp = "3.11"` (`Apache-2.0`) with `default-features = false` and features `["server", "macros", "transport-io", "elicitation", "base64"]`. Keep `transport-streamable-http-server` behind an optional `http-transport` Cargo feature on `lucubra-mcp`, disabled by default (D14).
2. **Protocol version ceiling.** Advertise `supported_protocol_versions` via `ProtocolVersion::known_up_to(&ProtocolVersion::V_2025_11_25)` and `ServerConfig::protocol_version = ProtocolVersion::LATEST_WITH_INITIALIZE` for v1 stdio sessions, preserving the `initialize` handshake for host detection (SPEC section 7) and server-initiated elicitation (M4).
3. **Direct `ServerHandler` dispatch with raw JSON pre-validation.** Implement `list_tools`, `get_tool` and `call_tool` directly on `LucubraServer`. JSON Schemas are generated from typed parameter structs via `schemars`, while `call_tool` inspects `CallToolRequestParams.arguments` as a raw JSON map before typed deserialization. Validation failures on any `lucubra_*` tool (including unknown tool name, unknown action, unknown key, missing field, type mismatch, invalid slug or bad `progress` entry) return `Ok(CallToolResult::error(...))` carrying the compact JSON envelope `{ok: false, code, fix, budget, warnings}` so host clients deliver the actionable `fix` to the model.
4. **Blocking I/O isolation.** `Workspace`, `rusqlite` caches and the Python worker `Supervisor` execute inside `tokio::task::spawn_blocking`, keeping the async transport loop non-blocking. Diagnostics write exclusively to `stderr`.
5. **Host detection and capability precedence.** `initialize` matches `clientInfo.name` against the host profile table to select default budgets, pointer files, artifact capability and export recipes, while reading `elicitation`, `sampling` and `roots` exclusively from `request.capabilities` rather than assuming support from the host name. Detected host metadata is recorded idempotently via `profile.set` only when changed.
6. **Budget enforcement and `wrap_up`.** Tokens are estimated as `chars.div_ceil(4)`. Every tool checks per-response limits (`orient` and general response ceilings per tier) and shrinks or paginates payloads before returning. Cumulative `served` tokens trigger `next: "wrap_up"` once the session threshold is crossed.
7. **Material isolation.** Extracted chunks, snippets and transcripts are sanitized of ANSI escapes, control characters and bidirectional overrides, and returned exclusively inside a structured `MaterialPayload` (`trust: "untrusted_study_material"`). Tool descriptions, prompt descriptions and server instructions never contain workspace content.
8. **Checkpoint atomicity.** The optional `progress` array present on every tool schema is validated before primary tool execution and appended as `step.completed` events with `sync_all` before the tool response is serialized.

## Consequences

- Every malformed tool call or unknown tool name from weak models receives a machine-readable `code` and a single-line `fix` without modifying `rmcp`.
- Supporting MCP `2026-07-28` inline lifecycle and `InputRequiredResult` later requires only widening `supported_protocol_versions` and adding an MRTR adapter for `lucubra_profile`.
- Adding a new tool or action requires registering its raw pre-validator rule alongside its `schemars` input struct; unit tests verify schema-to-validator parity.
