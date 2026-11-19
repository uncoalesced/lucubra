# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
"""Run every fixture generator and regenerate tests/fixtures/MANIFEST.json."""

from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path

SCRIPTS_DIR = Path(__file__).resolve().parent

GENERATORS = [
    "gen_pdf.py",
    "gen_docx.py",
    "gen_pptx.py",
    "gen_xlsx.py",
    "gen_csv.py",
    "gen_text.py",
    "gen_image.py",
    "gen_cad.py",
    "gen_eda.py",
    "gen_med.py",
    "gen_stubs.py",
    "gen_classify.py",
    "gen_manifest.py",
]


def main() -> int:
    uv_bin = shutil.which("uv")
    if uv_bin is None:
        print("error: uv executable not found on PATH", file=sys.stderr)
        return 1
    for name in GENERATORS:
        script_path = SCRIPTS_DIR / name
        subprocess.run([uv_bin, "run", str(script_path)], check=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
