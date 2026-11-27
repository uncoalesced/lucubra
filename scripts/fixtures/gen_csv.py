# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
"""Generate delimited text fixtures under tests/fixtures/csv/."""

from __future__ import annotations

import sys

from _common import FIXTURES_ROOT, write_text_crlf_bom, write_text_lf


def manifest_entries() -> list[dict[str, object]]:
    """Return ADR 0011 manifest descriptors for all csv/ fixtures."""
    return [
        {
            "path": "csv/data.csv",
            "format": "csv",
            "stub": False,
            "generator": "scripts/fixtures/gen_csv.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "contains": ["wavelength_nm", "absorbance", "Chlorophyll a"],
            },
            "notes": "UTF-8 LF comma-separated spectroscopy table",
        },
        {
            "path": "csv/data.tsv",
            "format": "tsv",
            "stub": False,
            "generator": "scripts/fixtures/gen_csv.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "contains": ["trial_id", "voltage_v", "current_ma"],
            },
            "notes": "UTF-8 LF tab-separated circuit measurement table",
        },
        {
            "path": "csv/data-crlf-bom.csv",
            "format": "csv",
            "stub": False,
            "generator": "scripts/fixtures/gen_csv.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "contains": ["region", "mean_temp_c", "Coastal Plain"],
            },
            "notes": "UTF-8 with BOM and CRLF line endings",
        },
    ]


def generate() -> None:
    """Write csv/data.csv, csv/data.tsv, and csv/data-crlf-bom.csv."""
    csv_text = (
        "pigment,wavelength_nm,absorbance\n"
        "Chlorophyll a,430,0.82\n"
        "Chlorophyll a,662,0.74\n"
        "Carotenoid,450,0.51\n"
    )
    tsv_text = "trial_id\tvoltage_v\tcurrent_ma\nT1\t1.50\t12.4\nT2\t3.00\t24.8\nT3\t4.50\t37.1\n"
    crlf_text = (
        "region,mean_temp_c,rainfall_mm\nCoastal Plain,28.4,1420\nHighland Plateau,21.1,890\n"
    )

    write_text_lf(FIXTURES_ROOT / "csv" / "data.csv", csv_text)
    write_text_lf(FIXTURES_ROOT / "csv" / "data.tsv", tsv_text)
    write_text_crlf_bom(FIXTURES_ROOT / "csv" / "data-crlf-bom.csv", crlf_text)


def main() -> int:
    generate()
    return 0


if __name__ == "__main__":
    sys.exit(main())
