# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
"""Generate EDA fixtures under tests/fixtures/eda/."""

from __future__ import annotations

import json
import sys

from _common import FIXTURES_ROOT, write_text_lf


def manifest_entries() -> list[dict[str, object]]:
    """Return ADR 0011 manifest descriptors for all eda/ fixtures."""
    return [
        {
            "path": "eda/amp.kicad_sch",
            "format": "kicad_sch",
            "stub": False,
            "generator": "scripts/fixtures/gen_eda.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "components": [
                    {"ref": "R1", "value": "10k"},
                    {"ref": "R2", "value": "20k"},
                    {"ref": "C1", "value": "100nF"},
                    {"ref": "U1", "value": "OPA2134"},
                ],
                "nets": ["VIN", "VOUT", "GND"],
            },
            "notes": "Minimal KiCad 8 schematic with op-amp and passive feedback components",
        },
        {
            "path": "eda/amp.kicad_pcb",
            "format": "kicad_pcb",
            "stub": False,
            "generator": "scripts/fixtures/gen_eda.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "components": [
                    {"ref": "R1", "value": "10k"},
                    {"ref": "R2", "value": "20k"},
                    {"ref": "C1", "value": "100nF"},
                    {"ref": "U1", "value": "OPA2134"},
                ],
                "nets": ["VIN", "VOUT", "GND"],
            },
            "notes": "Minimal KiCad 8 PCB layout matching amp.kicad_sch",
        },
        {
            "path": "eda/amp.kicad_pro",
            "format": "kicad_pro",
            "stub": False,
            "generator": "scripts/fixtures/gen_eda.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "contains": ["amp.kicad_pro"],
            },
            "notes": "Minimal KiCad 8 JSON project metadata",
        },
        {
            "path": "eda/rc.cir",
            "format": "spice",
            "stub": False,
            "generator": "scripts/fixtures/gen_eda.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "components": ["V1", "R1", "C1"],
                "analyses": [".tran", ".ac"],
            },
            "notes": "SPICE netlist with transient and AC sweep commands",
        },
        {
            "path": "eda/filter.asc",
            "format": "ltspice_asc",
            "stub": False,
            "generator": "scripts/fixtures/gen_eda.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "components": ["R1", "C1"],
                "analyses": [".ac"],
            },
            "notes": "LTspice ASCII schematic with low-pass RC filter and AC sweep",
        },
        {
            "path": "eda/board.gtl",
            "format": "gerber",
            "stub": False,
            "generator": "scripts/fixtures/gen_eda.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "contains": ["Copper,L1,Top"],
            },
            "notes": "RS-274X Gerber top copper layer with format and file function attributes",
        },
    ]


def generate() -> None:
    """Write all eda/ fixtures to tests/fixtures/eda/."""
    kicad_sch = """(kicad_sch
  (version 20231120)
  (generator "eeschema")
  (generator_version "8.0")
  (uuid "10000000-0000-4000-8000-000000000001")
  (paper "A4")
  (symbol
    (lib_id "Device:R")
    (at 100 100 0)
    (uuid "10000000-0000-4000-8000-000000000002")
    (property "Reference" "R1" (at 102 98 0))
    (property "Value" "10k" (at 102 102 0))
  )
  (symbol
    (lib_id "Device:R")
    (at 130 100 0)
    (uuid "10000000-0000-4000-8000-000000000003")
    (property "Reference" "R2" (at 132 98 0))
    (property "Value" "20k" (at 132 102 0))
  )
  (symbol
    (lib_id "Device:C")
    (at 130 120 0)
    (uuid "10000000-0000-4000-8000-000000000004")
    (property "Reference" "C1" (at 132 118 0))
    (property "Value" "100nF" (at 132 122 0))
  )
  (symbol
    (lib_id "Amplifier_Operational:OPA2134")
    (at 115 110 0)
    (uuid "10000000-0000-4000-8000-000000000005")
    (property "Reference" "U1" (at 115 104 0))
    (property "Value" "OPA2134" (at 115 116 0))
  )
  (label "VIN" (at 90 100 0))
  (label "VOUT" (at 145 100 0))
  (label "GND" (at 115 135 0))
)
"""

    kicad_pcb = """(kicad_pcb
  (version 20240108)
  (generator "pcbnew")
  (generator_version "8.0")
  (general (thickness 1.6))
  (paper "A4")
  (layers
    (0 "F.Cu" signal)
    (31 "B.Cu" signal)
  )
  (net 0 "")
  (net 1 "VIN")
  (net 2 "VOUT")
  (net 3 "GND")
  (footprint "Resistor_SMD:R_0603_1608Metric"
    (layer "F.Cu")
    (uuid "20000000-0000-4000-8000-000000000001")
    (at 100 100)
    (property "Reference" "R1" (at 0 -1.43 0))
    (property "Value" "10k" (at 0 1.43 0))
  )
  (footprint "Resistor_SMD:R_0603_1608Metric"
    (layer "F.Cu")
    (uuid "20000000-0000-4000-8000-000000000002")
    (at 110 100)
    (property "Reference" "R2" (at 0 -1.43 0))
    (property "Value" "20k" (at 0 1.43 0))
  )
  (footprint "Capacitor_SMD:C_0603_1608Metric"
    (layer "F.Cu")
    (uuid "20000000-0000-4000-8000-000000000003")
    (at 110 108)
    (property "Reference" "C1" (at 0 -1.43 0))
    (property "Value" "100nF" (at 0 1.43 0))
  )
  (footprint "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm"
    (layer "F.Cu")
    (uuid "20000000-0000-4000-8000-000000000004")
    (at 105 118)
    (property "Reference" "U1" (at 0 -3.2 0))
    (property "Value" "OPA2134" (at 0 3.2 0))
  )
)
"""

    kicad_pro = (
        json.dumps(
            {
                "board": {
                    "design_settings": {"defaults": {"board_outline_line_width": 0.1}},
                    "ipc2581": {},
                    "layer_presets": [],
                },
                "meta": {"filename": "amp.kicad_pro", "version": 1},
                "net_settings": {
                    "classes": [{"name": "Default", "clearance": 0.2, "track_width": 0.25}]
                },
                "sheets": [["10000000-0000-4000-8000-000000000001", "Root"]],
            },
            indent=2,
        )
        + "\n"
    )

    rc_cir = (
        "* First-order low-pass RC network\n"
        "V1 in 0 AC 1 PULSE(0 5 0 1n 1n 5m 10m)\n"
        "R1 in out 1k\n"
        "C1 out 0 100n\n"
        ".tran 0.1ms 10ms\n"
        ".ac dec 10 10 100k\n"
        ".end\n"
    )

    filter_asc = (
        "Version 4\n"
        "SHEET 1 880 680\n"
        "WIRE 160 128 96 128\n"
        "WIRE 272 128 240 128\n"
        "WIRE 272 160 272 128\n"
        "WIRE 272 256 272 224\n"
        "FLAG 272 256 0\n"
        "SYMBOL res 256 112 R90\n"
        "SYMATTR InstName R1\n"
        "SYMATTR Value 1k\n"
        "SYMBOL cap 256 160 R0\n"
        "SYMATTR InstName C1\n"
        "SYMATTR Value 100n\n"
        "TEXT 96 304 Left 2 !.ac dec 10 1 100k\n"
    )

    board_gtl = (
        "%FSLAX36Y36*%\n"
        "%MOMM*%\n"
        "%TF.FileFunction,Copper,L1,Top*%\n"
        "%TF.Part,Single*%\n"
        "%ADD10C,0.2500*%\n"
        "G01*\n"
        "D10*\n"
        "X0Y0D02*\n"
        "X10000000Y0D01*\n"
        "X10000000Y5000000D01*\n"
        "M02*\n"
    )

    write_text_lf(FIXTURES_ROOT / "eda" / "amp.kicad_sch", kicad_sch)
    write_text_lf(FIXTURES_ROOT / "eda" / "amp.kicad_pcb", kicad_pcb)
    write_text_lf(FIXTURES_ROOT / "eda" / "amp.kicad_pro", kicad_pro)
    write_text_lf(FIXTURES_ROOT / "eda" / "rc.cir", rc_cir)
    write_text_lf(FIXTURES_ROOT / "eda" / "filter.asc", filter_asc)
    write_text_lf(FIXTURES_ROOT / "eda" / "board.gtl", board_gtl)


def main() -> int:
    generate()
    return 0


if __name__ == "__main__":
    sys.exit(main())
