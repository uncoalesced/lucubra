# ADR 0003: Full-text search tokenisation

- Status: Accepted
- Date: 2026-11-13
- Decision references: D11, SPEC sections 10.1 and 11

## Context

Search uses SQLite FTS5. SPEC section 10.1 asked whether the bundled SQLite includes FTS5, and whether the default `unicode61` tokenizer splits Indic words at combining marks.

## Evidence

`rusqlite` 0.42.0 with the `bundled` feature (SQLite 3.54.1) reports `ENABLE_FTS5 = 1`. Samples in English, Hindi, Malayalam, Tamil and Bengali were indexed with three tokenizers and queried by whole word, by prefix and by a substring inside a word.

| Tokenizer | Indic words kept whole | Whole word | Prefix | Inner substring |
|---|---|---|---|---|
| `unicode61` (default) | No. Words split at every combining mark: 48 tokens for 19 words | Matched, as a phrase of fragments | Matched | Tamil only |
| `unicode61` with categories `L* N* Co Mn Mc` | Yes: 19 tokens for 19 words | Matched in all five scripts | Matched in all five scripts | No match in any script |
| `trigram` | Not applicable | Matched in all five scripts | Matched in all five scripts | Matched in all five scripts |

A further test showed that a zero-width non-joiner (U+200C) inside a Hindi word splits it into two tokens under the chosen `unicode61` configuration, and that a trailing zero-width joiner (U+200D), used by the older Malayalam chillu encoding, is dropped from the token. Both characters are in Unicode category Cf.

The default tokenizer's matches in the first row are an artefact: it matches a word only as the same sequence of fragments, so ranking and prefix queries operate on fragments rather than words.

## Decision

1. The primary index uses `unicode61` with token characters `L* N* Co Mn Mc`, so combining marks stay inside words. It serves word queries, prefix queries and BM25 ranking.
2. A secondary index uses the `trigram` tokenizer and serves substring queries of three or more characters, which matters for agglutinative languages such as Malayalam and Tamil.
3. Text is normalised with `normalize_for_fts` before indexing and before querying. It removes the soft hyphen, the zero-width space, the zero-width non-joiner, the zero-width joiner, the word joiner and the byte order mark.

The tests in `crates/lucubra-core/tests/fts5.rs` encode each row above and fail if the behaviour changes.

## Consequences

- The trigram index adds substantial size for the text it covers. M2 measures it on the fixture corpus and may limit the trigram index to non-Latin text if the cost is high.
- Removing joiners merges spellings that differ only in a joiner. That is the intended behaviour for search; stored source text is unchanged.
- Queries shorter than three characters cannot use the trigram index and fall back to the primary index.

## Amendment (M2, 2026-11-14)

`normalize_for_fts` now applies Unicode NFC first, then folds the joiner encoding of the six Malayalam chillu letters (consonant, U+0D4D, U+200D) to the atomic letters U+0D7A to U+0D7F, then removes the characters listed above. NFD and NFC spellings, and joiner and atomic chillu spellings, index and search as the same word. The ingest index schema version was raised so existing indexes rebuild with the new rule.
