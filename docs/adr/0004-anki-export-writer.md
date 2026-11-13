# ADR 0004: Anki export writer

- Status: Accepted
- Date: 2026-11-13
- Decision references: D25, SPEC section 14

## Context

Lucubra exports cards one way to Anki as an `.apkg` file. SPEC section 14 asked for the licence of an `.apkg` writer such as `genanki`.

## Evidence

Read from the PyPI and crates.io APIs on 2026-11-12 and 2026-11-13:

| Candidate | Version | Licence | Notes |
|---|---|---|---|
| `genanki` (PyPI) | 0.13.1, released 2023-11-12 | MIT | Requires `cached-property`, `frozendict`, `chevron` and `pyyaml` |
| `frozendict` (PyPI) | 2.5.1 | LGPL-3.0-only | Required dependency of `genanki` |
| `genanki-rs` (crate) | 0.4.0, released 2023-07-29 | MIT | Depends on `rusqlite` 0.25 and `zip` 0.5, several major versions behind current releases |
| `zip` (crate) | 9.5.1 | MIT | |
| `rusqlite` (crate) | 0.42.0 | MIT | Already used by `lucubra-core` |

## Decision

Lucubra writes `.apkg` files itself in Rust, with the `zip` and `rusqlite` crates. The writer produces the collection database, the media map and the numbered media files. No code from Anki or from `genanki` is copied. The TSV export in SPEC section 14 remains as the fallback.

## Consequences

- `genanki` is not used, because its required `frozendict` dependency is LGPL-3.0-only and would break the permissive dependency policy for the default install.
- The writer must track the collection schema that current Anki releases import. M6 adds an import check against a current Anki release, run by hand and recorded in the gate ledger.
- Anki's own licence and schema documentation were not read for this decision. The writer relies on the file format alone.
