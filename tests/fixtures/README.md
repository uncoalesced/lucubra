# M2 Ingestion Fixture Corpus

Deterministic, original test fixtures covering Tier 1 formats, boundary conditions, malformed inputs, Indic scripts, Tier 2 stubs, and classification batches as defined in ADR 0011. Rebuild the full corpus and update `MANIFEST.json` with:

```sh
uv run scripts/fixtures/build_all.py
```
