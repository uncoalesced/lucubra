# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
"""Generate magic-byte stub fixtures under tests/fixtures/stubs/."""

from __future__ import annotations

import sys

from _common import FIXTURES_ROOT, write_bytes


def manifest_entries() -> list[dict[str, object]]:
    """Return ADR 0011 manifest descriptors for all stubs/ fixtures."""
    return [
        {
            "path": "stubs/drawing.dwg",
            "format": "dwg",
            "stub": True,
            "generator": "scripts/fixtures/gen_stubs.py",
            "scenarios": ["S07"],
            "expect": {
                "outcome": "pending",
                "code": "tool_missing",
                "tool": "oda",
            },
            "notes": "AC1032 AutoCAD R2018 DWG magic stub for Tier 2 routing",
        },
        {
            "path": "stubs/scene.blend",
            "format": "blend",
            "stub": True,
            "generator": "scripts/fixtures/gen_stubs.py",
            "scenarios": ["S08"],
            "expect": {
                "outcome": "pending",
                "code": "tool_missing",
                "tool": "blender",
            },
            "notes": "BLENDER-v300 magic stub for Tier 2 routing",
        },
        {
            "path": "stubs/model.rvt",
            "format": "rvt",
            "stub": True,
            "generator": "scripts/fixtures/gen_stubs.py",
            "scenarios": ["S09"],
            "expect": {
                "outcome": "failed",
                "code": "unsupported",
            },
            "notes": "OLE compound header stub for unsupported Revit format",
        },
        {
            "path": "stubs/model.skp",
            "format": "skp",
            "stub": True,
            "generator": "scripts/fixtures/gen_stubs.py",
            "scenarios": ["S09"],
            "expect": {
                "outcome": "failed",
                "code": "unsupported",
            },
            "notes": "SketchUp header stub for unsupported format routing",
        },
    ]


def generate() -> None:
    """Write all stubs/ fixtures to tests/fixtures/stubs/."""
    dwg_stub = b"AC1032" + b"\x00" * 58
    blend_stub = b"BLENDER-v300" + b"\x00" * 48 + b"ENDB"
    rvt_stub = bytes.fromhex("D0CF11E0A1B11AE1") + b"\x00" * 504
    skp_stub = b"\xff\xfe\xff\x0e" + "SketchUp Model".encode("utf-16-le") + b"\x00" * 32

    write_bytes(FIXTURES_ROOT / "stubs" / "drawing.dwg", dwg_stub)
    write_bytes(FIXTURES_ROOT / "stubs" / "scene.blend", blend_stub)
    write_bytes(FIXTURES_ROOT / "stubs" / "model.rvt", rvt_stub)
    write_bytes(FIXTURES_ROOT / "stubs" / "model.skp", skp_stub)


def main() -> int:
    generate()
    return 0


if __name__ == "__main__":
    sys.exit(main())
