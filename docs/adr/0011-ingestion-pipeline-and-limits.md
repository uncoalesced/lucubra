# ADR 0011: Ingestion pipeline, worker limits and fixture manifest

- Status: Accepted
- Date: 2026-11-14
- Decision references: SPEC sections 4.2, 5 rules 2 and 8, 10.1, 10.2; ADR 0003; scenarios S03 to S09, S20, S30, S36, S38

## Context

M2 builds ingestion: discovery, hashing, routing, extraction by native Rust code or Python workers, vision detection, and FTS indexing. SPEC section 4.2 fixes the worker protocol and a 120-second default timeout, but no size limits, failure codes or event payloads. The fixture corpus is built in parallel by another agent, so the shape of its manifest has to be fixed before either side starts.

## Decision

### 1. Discovery and identity

1. Discovery walks the workspace root, including `inbox/`. It skips `.lucubra/`, `packs/`, `sessions/`, `dashboard/`, engine-written projections (ADR 0009), `*.transcript.md` under `notes/`, and names that start with `.` or `~$` (Office lock files).
2. A file is identified by the BLAKE3 hash of its contents. Paths are workspace-relative with `/` separators. A renamed file with the same hash keeps its history; two paths with one hash are one file with two locations.
3. A file whose size and modification time match the last `file.seen` for that path is not re-hashed.
4. Online-only placeholders (Windows `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS` or `FILE_ATTRIBUTE_OFFLINE`; macOS `SF_DATALESS`) are never opened. They are counted and reported, with total size, and skipped until the user agrees (SPEC section 5 rule 8, S20).

### 2. Routing

Magic bytes first, then extension. The format identifiers below are used in events, in the fixture manifest and in `lucubra doctor`.

| Identifier | Detected by | Extractor |
|---|---|---|
| `pdf` | `%PDF-` | worker |
| `docx`, `pptx`, `xlsx`, `xlsm`, `epub`, `ods` | zip magic plus `[Content_Types].xml` or `mimetype` entry, then extension | worker for `docx`, `pptx`; native for `xlsx`, `xlsm`, `ods`, `epub` |
| `xls` | OLE magic `D0 CF 11 E0` plus extension | native (`calamine`) |
| `csv`, `tsv` | extension | native |
| `md`, `txt`, `html`, `ipynb`, `code` | extension, UTF-8 check | native |
| `image` | PNG, JPEG, WebP, GIF magic | native (metadata only) |
| `dxf` | extension plus `SECTION` header | worker (`ezdxf`) |
| `kicad_sch`, `kicad_pcb`, `kicad_pro` | extension | native |
| `spice`, `ltspice_asc` | extension (`.cir`, `.sp`, `.net`, `.asc`) | native |
| `gerber` | extension (`.gbr`, `.gtl`, `.gbl`, ...) plus `%FS` | native |
| `dicom` | `DICM` at offset 128 | worker (`pydicom`) |
| `dwg` | `AC10` magic | Tier 2: ODA File Converter or LibreDWG, then `dxf` |
| `blend` | `BLENDER` magic | Tier 2: headless Blender |
| `rvt`, `skp` | extension | unsupported, with export steps (D49) |

Anything else is `unknown`: recorded as seen, not extracted.

### 3. Outcomes and failure codes

Each extraction ends in one outcome: `extracted`, `failed` or `pending`. A `failed` or `pending` file carries one code:

| Code | Outcome | Meaning |
|---|---|---|
| `timeout` | failed | The request passed its time limit; the worker was killed and restarted |
| `limit_exceeded` | failed | A size, ratio or entry-count limit in section 4 was hit |
| `corrupt` | failed | The parser rejected the file |
| `encrypted` | failed | Password-protected; never prompted for |
| `worker_crashed` | failed | The worker exited mid-request |
| `unsupported` | failed | A known format with no reader in v1 (`rvt`, `skp`) |
| `tool_missing` | pending | A Tier 2 tool is not installed; retried when `lucubra doctor` finds it |
| `online_only` | pending | A cloud placeholder not yet approved for download |

`needs_vision` is not a failure. It is a list of pages, slides or images attached to an `extracted` outcome.

### 4. Limits

| Limit | Value |
|---|---|
| Per-request timeout | 120 s default, configurable per workspace |
| Input file size | 1 GiB; larger files are `limit_exceeded` before any read |
| Zip containers: declared uncompressed total | 512 MiB, checked from the central directory before extraction |
| Zip containers: entry count | 20,000 |
| Zip containers: actual bytes read | 512 MiB in total, counted while streaming, so a header that under-declares sizes is still caught |
| Zip containers: ratio | an entry above 1 MiB uncompressed with a ratio above 200:1 is `limit_exceeded` |
| Extracted text per file | 64 MiB; text past the cap is dropped and the file is flagged `truncated` |
| Rendered page image | long edge 1,600 px (SPEC section 10.1) |
| Worker memory | a job object on Windows and `RLIMIT_AS` on Unix, 2 GiB default; not enforced where the OS does not allow it |

Limits apply before and during parsing, in the worker or in the native extractor, whichever runs.

### 5. Vision detection

A PDF page is `needs_vision` when it has **fewer than 40** non-whitespace extractable characters. A slide is `needs_vision` when it has no text and at least one picture. An image file is always `needs_vision`, on demand.

### 6. Events

Extracted text, chunks, the FTS index and rendered images are rebuildable, so they live in the app-data cache. Only these facts go into the event log:

- `file.seen`: `path`, `hash`, `bytes`, `mtime_ms`, `online_only`.
- `file.extracted`: `hash`, `format`, `extractor` (name and version), `chunks`, `units_hint` (outline or headings, when found), `needs_vision` (list), `truncated`.
- `file.failed`: `hash`, `path`, `format`, `outcome` (`failed` or `pending`), `code`, `detail`, plus `tool` for `tool_missing`.

Chunk references follow SPEC section 10.1: `F:<first 8 hex of the hash>` plus a locator (`p:12`, `s:4`, `x:Sheet1!B2:F40`, `l:<layer>`, `o:<object>`, `c:<cell>` for notebooks, `h:<heading index>` for text). The `S:` and `U:` parts are added once the file is classified (M4).

### 7. Fixture manifest

The corpus lives in `tests/fixtures/`, with outputs committed and generators in `scripts/fixtures/`. `tests/fixtures/MANIFEST.json` describes every file:

```json
{
  "schema": 1,
  "files": [
    {
      "path": "pdf/mixed-threshold.pdf",
      "format": "pdf",
      "sha256": "<hex>",
      "bytes": 4123,
      "stub": false,
      "generator": "scripts/fixtures/gen_pdf.py",
      "scenarios": ["S04"],
      "expect": {
        "outcome": "extracted",
        "pages": 3,
        "page_chars": [39, 40, 41],
        "needs_vision": [1],
        "contains": ["chlorophyll"]
      },
      "notes": "Boundary pages for the 40-character threshold"
    }
  ]
}
```

- `path` is relative to `tests/fixtures/`; `format` is an identifier from section 2; `sha256` and `bytes` describe the committed file.
- `stub: true` marks a file that only carries the right magic bytes (`dwg`, `blend`, `rvt`, `skp`); its `expect` covers routing and the outcome only.
- `expect.outcome` and `expect.code` use the values in section 3.
- Page and slide numbers are 1-based. `page_chars` counts non-whitespace characters, the same rule as section 5.
- `contains` lists strings that extraction must return, compared after NFC normalisation.
- Other `expect` keys are optional and format-specific: `slides`, `notes_contains`, `sheets` (`name`, `formulas` and `values` as A1 maps, `named_ranges`), `layers`, `blocks`, `components`, `nets`, `analyses`, `script`, `search_words`, `subject`, `course_code`, `tool`.

## Consequences

- An extraction-cache miss on a second device re-extracts locally; nothing extracted is synced.
- The 40-character threshold and the limits are initial values, tuned in M10.
- Tier 2 tools and online-only detection cannot be produced as fixture files: stubs cover routing, and attribute checks are tested with mocks.
