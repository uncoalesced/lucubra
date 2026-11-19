# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
"""Build tests/fixtures/MANIFEST.json from all fixture generators."""

from __future__ import annotations

import hashlib
import json
import sys

import gen_cad
import gen_classify
import gen_csv
import gen_docx
import gen_eda
import gen_image
import gen_med
import gen_pdf
import gen_pptx
import gen_stubs
import gen_text
import gen_xlsx
from _common import FIXTURES_ROOT, write_text_lf

GENERATOR_MODULES = [
    gen_pdf,
    gen_docx,
    gen_pptx,
    gen_xlsx,
    gen_csv,
    gen_text,
    gen_image,
    gen_cad,
    gen_eda,
    gen_med,
    gen_stubs,
    gen_classify,
]


def build_manifest() -> dict[str, object]:
    """Collect entries from every generator and fill sha256 and byte sizes."""
    collected: list[dict[str, object]] = []
    for mod in GENERATOR_MODULES:
        for raw_entry in mod.manifest_entries():
            rel_path = str(raw_entry["path"])
            target = FIXTURES_ROOT / rel_path
            if not target.is_file():
                raise FileNotFoundError(f"Missing fixture file for manifest: {target}")
            data = target.read_bytes()
            ordered = {
                "path": rel_path,
                "format": raw_entry["format"],
                "sha256": hashlib.sha256(data).hexdigest(),
                "bytes": len(data),
                "stub": raw_entry["stub"],
                "generator": raw_entry["generator"],
                "scenarios": raw_entry["scenarios"],
                "expect": raw_entry["expect"],
                "notes": raw_entry["notes"],
            }
            collected.append(ordered)

    collected.sort(key=lambda item: str(item["path"]))
    return {"schema": 1, "files": collected}


def generate() -> None:
    """Write tests/fixtures/MANIFEST.json cleanly with UTF-8 LF formatting."""
    manifest = build_manifest()
    text = json.dumps(manifest, indent=2, ensure_ascii=False) + "\n"
    write_text_lf(FIXTURES_ROOT / "MANIFEST.json", text)


def main() -> int:
    generate()
    return 0


if __name__ == "__main__":
    sys.exit(main())
