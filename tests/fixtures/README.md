# Ingestion fixture corpus

This folder holds the fixture corpus for the ingestion tests. It covers the Tier 1 formats, malformed and hostile inputs, Indic scripts, Tier 2 tool stubs and a classification batch, as ADR 0011 section 7 defines. A generator script wrote every file. None comes from a real person's notes, so the corpus holds no personal data.

## Layout

| Folder | Contents |
|---|---|
| `text/` | Markdown, plain text, HTML, EPUB, a Jupyter notebook and source code, including a CRLF file with a BOM and Malayalam in both NFC and NFD |
| `pdf/` | Text and table PDFs, Hindi, Kannada and Malayalam PDFs, scanned pages that need vision, a page at the vision threshold, and encrypted and truncated files |
| `docx/`, `pptx/`, `xlsx/`, `csv/` | Office and spreadsheet files, including two zip bombs (`bomb-honest.docx` and `bomb-lying.docx`, where the entry header understates the size) |
| `image/` | PNG, JPEG and WebP images for the vision and handwriting path |
| `cad/` | A DXF drawing with layers, a block and text |
| `eda/` | KiCad, Gerber, LTspice and SPICE text formats |
| `med/` | A synthetic DICOM phantom with no patient data |
| `stubs/` | Placeholder `.dwg`, `.blend`, `.rvt` and `.skp` files, not real models. DWG and Blend must stay pending until the tool is found; RVT and SKP must fail as unsupported (D49). |
| `classify/` | PDFs named like course material (course codes, handouts, readings, ambiguous names) and a `profile.json`, for the classification and organisation tests |

`.gitattributes` sets `* -text`, so Git never rewrites the line endings or the BOM of a fixture. Some tests check exactly those bytes.

## MANIFEST.json

`MANIFEST.json` (`"schema": 1`) lists every fixture. Each entry has:

- `path`, `format`, `bytes` and `sha256`: the file and its exact bytes
- `stub`: true for the Tier 2 placeholders
- `generator`: the script that writes the file
- `scenarios`: the scenario ids (for example `S07`) that the fixture supports
- `expect`: what ingestion must produce.
  - `outcome` is one of:
    - `extracted`
    - `failed`, with a failure `code` such as `limit_exceeded`, `corrupt`, `encrypted` or `unsupported`
    - `pending`, with the missing `tool`
  - The other keys depend on the format: `contains`, `pages`, `page_chars`, `needs_vision`, `sheets`, `named_ranges`, `slides`, `layers`, `blocks`, `nets`, `components`, `analyses`, `script` and `search_words`.
  - Classification fixtures use `subject`, `course_code` and `units_hint`.
- `notes`: why the fixture exists

## Tests that use it

- `crates/lucubra-ingest/tests/corpus.rs` ingests every entry and checks the result against `expect`.
  - Set `LUCUBRA_FIXTURES_DIR` to point it at another copy of the corpus.
  - The PDF, Office, DXF and DICOM entries need the Python workers in the project virtual environment (`uv sync`). Without it they are skipped. CI runs `uv sync` first.
- `tests/test_fixture_manifest.py` checks three things:
  - every file is listed with a matching hash and size
  - no fixture or archive member carries a watermark
  - the byte-level properties: the BOM and CRLF, NFC against NFD, and the real sizes of the two zip bombs

## Rebuilding

The generators are deterministic. They use fixed seeds, fixed timestamps (`scripts/fixtures/_common.py`) and fixed zip entry times, so a rebuild gives the same bytes and hashes.

```sh
uv run scripts/fixtures/build_all.py
```

This rewrites the corpus and `MANIFEST.json`. If a hash changes when no generator changed, treat it as a generator bug, not as a new fixture.

To add a fixture:
1. Add it to the matching `gen_*.py`.
2. Give it an `expect` block and the scenario ids.
3. Rebuild.
4. Commit the new file and the manifest together.

## Scale corpus

The scale corpus for scenario S36 is not kept here. It is about 3,000 files and 5 GB. Write it outside the repository:

```sh
uv run scripts/fixtures/gen_scale_corpus.py --out <dir>
```

For example, use `E:\TestEnvironments\lucubra-perf` for `<dir>`. The script refuses an output folder on `C:` or inside the repository. `--count` and `--seed` change its size and contents.
