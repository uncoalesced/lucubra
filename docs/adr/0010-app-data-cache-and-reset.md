# ADR 0010: App-data directory, state cache and reset

- Status: Accepted
- Date: 2026-11-14
- Decision references: D13, SPEC section 5 rules 1 to 3, SPEC section 22, scenarios S19 and S42

## Context

SPEC section 5 keeps the SQLite state cache and the device identifier in the operating system's app-data directory, outside any synced folder. The build environment forbids writes to `C:\` outside the vault, so tests need a way to move that directory. The workspace path differs between devices, so it cannot identify the cache. SPEC section 22 says `lucubra reset` never touches sources or user notes but does not define what it clears.

## Decision

1. **Location.** The data directory is the platform's local data directory for the application `lucubra` (via the `directories` crate). The environment variable `LUCUBRA_DATA_DIR` overrides it; every test and every gate command sets it.
2. **Layout.**
   - `device_id`: the device identifier (ADR 0008), created on first use.
   - `workspaces/<workspace-id>/cache.sqlite`: the state cache for one workspace.
3. **Workspace identity.** The `workspace.initialized` event carries a ULID `workspace_id`, which keys the cache. `lucubra init` writes it; the same workspace synced to another device maps to the same cache key there.
4. **Cache contents.** A `meta` table (schema version, fold fingerprint, high-water order key), an `applied` table of event identifiers, and a `state` table of `(collection, key, value)` rows. The state hash is computed from rows read back out of SQLite in key order, so the rebuild check exercises the cache and not the in-memory fold.
5. **Refresh.** A refresh runs in one `BEGIN IMMEDIATE` transaction, so two processes on one device cannot overwrite each other's work. When every unapplied event sorts after the high-water mark, the cache applies them in order. Otherwise (an older event arrived by sync, or an applied event is missing) the cache is rebuilt from all events. The fold fingerprint names the record version and the known and folded event types; when an engine upgrade changes it, the cache is cleared on open, so events skipped by an older engine are folded.
6. **Reset (Joel, 2026-11-14: cache only).** `lucubra reset` deletes the workspace's cache directory and every projection whose hash is recorded in a sidecar (ADR 0009), then rebuilds the cache and regenerates the projections. It never deletes `.lucubra/events/`, `.lucubra/preserved/`, `.lucubra/conflicts/`, `.lucubra/written/`, `sources/`, `notes/` or any other user file. It asks for confirmation unless `--yes` is given.

## Consequences

- Removing all Lucubra state remains a manual step (delete `.lucubra/` and the data directory), as SPEC section 22 describes.
- S42's "state cleared on request" is met for derived state only; event logs survive a reset by design.
