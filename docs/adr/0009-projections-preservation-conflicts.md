# ADR 0009: Projections, edit preservation and sync-conflict copies

- Status: Accepted
- Date: 2026-11-14
- Decision references: D13, D46, SPEC section 5 rules 4 to 7, SPEC section 11, scenarios S31, S45 and S58

## Context

D46 removed the header that once marked generated workspace files. SPEC section 5 rule 4 still requires that a user's edit to a generated file is saved before the file is regenerated, which now depends only on comparing the file with the hash the engine last wrote. The SQLite cache cannot hold that hash: it is deleted on rebuild (S19), and it is local to one device, so a second device would treat every synced projection as a user edit. Cloud sync clients also create renamed copies of files edited on two devices at once (rule 5).

## Decision

1. **Deterministic rendering.** Projections are rendered from replayed state only. They use LF line endings and contain no timestamps, device identifiers, headers or provenance text. Two devices with the same state produce byte-identical files.
2. **Projection set (M1).** The root `AGENTS.md`, one `subjects/<subject>/agents_<subject>.md` per subject, and one `subjects/<subject>/<unit>/UNIT.md` per unit. Links between them are wikilinks, so the workspace opens as a graph in Obsidian. The root index states how to use Lucubra from the folder and stays under 1,000 tokens (SPEC section 11).
3. **Written-hash sidecar.** Each device records the BLAKE3 hash of every projection it writes in `.lucubra/written/<device-id>.json`, a JSON object mapping the workspace-relative path (forward slashes) to the hash. Only that device writes its sidecar, so sync never produces conflicting writes to it. The file is written to a temporary file in the same directory and renamed into place.
4. **Preservation rule.** Before a projection is written, the engine reads every device's sidecar. If the file on disk is byte-identical to the new rendering, nothing is written. Otherwise, if the file exists and its hash is not recorded for that path in any sidecar, it is a user edit: it is copied to `.lucubra/preserved/<path>.<UTC timestamp>.md` and a `projection.preserved` event is logged with the path, the hash and the preserved location. The new rendering is then written and recorded in the sidecar. A missing or unreadable sidecar can only cause an extra preserved copy, never a lost edit.
5. **Conflict-copy names.** For each projection path, a sibling file is treated as a sync-conflict copy of it when its name, with the same extension, matches one of these forms of the projection's base name `B`:
   - Dropbox: `B (... conflicted copy ...)`. Dropbox appends the editor's name, the words "conflicted copy" and the date (help.dropbox.com, "What's a conflicted copy?").
   - OneDrive: `B-<computer name>`. OneDrive keeps both versions and appends the computer name (Microsoft Q&A and forum reports; no primary Microsoft page found on 2026-11-14).
   - Google Drive for desktop: `B (n)` (third-party reports only; no primary Google page found on 2026-11-14).
   - iCloud Drive: `B n` with `n` of 2 or more (Apple Technical Note TN2336, "bounced file name").
   A sibling that is itself a projection path is never treated as a copy. A name match alone is not enough, because user files such as `UNIT-summary.md` match the OneDrive form: the sibling must also hold content some device recorded for that projection, or open with the same first line (the heading) as the projection's current rendering.
6. **Conflict handling.** A conflict copy of a projection is moved to `.lucubra/conflicts/<path>`, keeping its name; nothing is deleted. A file outside the projection set whose name carries a conflict marker (`conflicted copy`, or a ` (n)` suffix) is reported to the user and never moved. `.lucubra/` and `sources/` are not scanned for user conflicts.

## Consequences

- `.lucubra/written/` is added to the workspace tree in SPEC section 5. It is regenerable in the sense of rule 1: losing it causes at most extra preserved copies.
- The OneDrive and Google Drive patterns rest on secondary sources and are checked again in the M10 host runs.
- Reset (ADR 0010) removes only projections whose hash is recorded in a sidecar; an edited projection is preserved by the regeneration that follows.
