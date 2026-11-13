# ADR 0007: Source file headers

- Status: Accepted
- Date: 2026-11-13
- Decision references: D46, addendum A11, scenario S58

## Context

D46 replaces SPEC section 23: the provenance comment appears only in source code files, and no file other than source code carries a watermark or a banner stating that Lucubra generated it. Addendum A11 keeps the SPDX licence line in source files as an assumption.

## Decision

1. Source code files carry, within their first five lines, an SPDX line (`SPDX-License-Identifier: Apache-2.0`) followed by the provenance comment from D46, each in the file's own comment syntax. A shebang line may precede them.
2. Source code files are those with the extensions `.rs`, `.py`, `.ps1`, `.sh`, `.js`, `.mjs` and `.ts`. The provenance text appears exactly once in each of them.
3. All other files carry neither line: Markdown, documentation, configuration (`Cargo.toml`, `pyproject.toml`, `deny.toml`, CI workflows), lockfiles, data, fixtures, `LICENSE`, `NOTICE`, and every file Lucubra generates in a user's workspace.
4. `scripts/audit_headers.py` enforces these rules on every tracked and untracked, non-ignored file. Binary files are skipped. CI runs it, and its tests plant each kind of violation.

## Consequences

- Documentation, including this record, describes the provenance comment instead of quoting it, because the audit rejects the text outside source files.
- If the SPDX assumption in A11 is reversed, rule 1 and the audit change together.
- The BUILD_PLAN gate wording for M0 ("every source and config file") predates D46. The gate is run under this record's rules.
