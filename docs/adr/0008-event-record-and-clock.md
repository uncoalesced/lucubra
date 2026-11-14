# ADR 0008: Event record, device identity and hybrid logical clock

- Status: Accepted
- Date: 2026-11-14
- Decision references: D13, D43, SPEC sections 5 and 6, addendum A8, scenarios S18 and S19

## Context

SPEC section 6 fixes the record envelope and states that appends are atomic per line, that a torn last line is ignored on replay, and that replay on any device produces byte-identical state. It leaves open the device identifier length, the exact clock encoding, counter overflow, duplicate delivery, events written by a newer engine, and how a writer behaves after a crash left a torn line.

## Decision

1. **Envelope.** Each line is one JSON object with the fields `v`, `id`, `dev`, `seq`, `hlc`, `ts`, `type` and `data`, as in SPEC 6.1. `v` is the record schema version (currently 1). `id` is a ULID. `ts` is the UTC wall time in RFC 3339 with millisecond precision; it is informational and never used for ordering.
2. **Device identifier.** `d-` followed by 8 lowercase hexadecimal characters, generated once per install from a random source and stored in the app-data directory (ADR 0010). The SPEC example uses 4 characters; 8 reduce the chance that two devices share a log file.
3. **Clock encoding.** `<ms>-<counter>-<dev>`, where `<ms>` is the physical time in milliseconds since the Unix epoch, zero-padded to 13 digits, and `<counter>` is a logical counter zero-padded to 4 digits. The string is always parsed into the tuple `(ms, counter, dev)` and compared as a tuple, never as text.
4. **Clock rules.** On a local event the clock takes `max(physical now, last ms)`; the counter resets to 0 when the millisecond advances and increments otherwise. When the counter would exceed 9999, the clock advances to the next millisecond with counter 0. After replay, the local clock observes the greatest clock value seen in any log, so every new local event orders after everything the device has seen. An observed value more than 24 hours ahead of physical time is ignored, so a device with a bad clock, or a crafted event, cannot drive the clock to the end of the 13-digit range. A clock value that cannot be encoded is an error, never a written line.
5. **Total order.** Replay sorts events by `(hlc, dev, seq)`. Events with the same `id` are delivered once; later copies are ignored. This makes replay independent of file read order, duplicate delivery from conflict copies of a log, and partial sync.
6. **Writer.** A device appends only to `.lucubra/events/<device-id>.jsonl`. Each append takes an exclusive lock on the file, reads the last valid `seq` and clock value from the file, writes the complete line and its newline in one write call, calls `fsync` (`File::sync_all`), and releases the lock. The `seq` comes from the log itself, not from a separate counter file.
7. **Torn lines.** If the file does not end with a newline, the writer first writes a newline, so the torn fragment stays a separate line and the new record is intact. The writer never truncates or rewrites a log. Replay skips any line that does not parse as a record, counts it, and reports it as a warning; a torn final line is the expected case.
8. **Versions and unknown types.** Records are upcast on read according to `v` (A8, D43); old files are never rewritten. A record with a `v` greater than the engine supports, or with a `type` the engine does not know, is kept in the event set, skipped by the fold, and reported as a warning. It is never dropped from disk.
9. **Deterministic state.** State uses ordered maps only and holds no floating-point values. Its hash is BLAKE3 over a canonical serialisation, so the same event set yields the same hash on every device.

## Consequences

- Two processes on one device (for example the MCP server and the CLI) serialise their appends through the file lock.
- A clock that runs far ahead on one device pulls the others forward; ordering stays consistent because it never depends on wall time alone.
- Every new event type added in a later milestone extends the type list and, if folded, the state and the canonical serialisation, which changes state hashes for workspaces that contain such events.
