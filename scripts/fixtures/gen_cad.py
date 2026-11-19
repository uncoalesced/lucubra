# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
# /// script
# requires-python = ">=3.10"
# dependencies = [
#   "ezdxf>=1.3,<2",
# ]
# ///
"""Generate CAD fixtures under tests/fixtures/cad/."""

from __future__ import annotations

import io
import sys

from _common import FIXTURES_ROOT, write_bytes


def manifest_entries() -> list[dict[str, object]]:
    """Return ADR 0011 manifest descriptors for all cad/ fixtures."""
    return [
        {
            "path": "cad/drawing.dxf",
            "format": "dxf",
            "stub": False,
            "generator": "scripts/fixtures/gen_cad.py",
            "scenarios": ["S07"],
            "expect": {
                "outcome": "extracted",
                "layers": ["Walls", "Dimensions"],
                "blocks": ["Valve"],
                "contains": [
                    "North Wall 4200 mm",
                    "Structural partition",
                    "Check clearance",
                ],
            },
            "notes": (
                "R2010 DXF with Walls/Dimensions layers, Valve block, TEXT, MTEXT, and linear dim"
            ),
        }
    ]


def _build_drawing_dxf() -> bytes:
    import ezdxf

    ezdxf.options.write_fixed_meta_data_for_testing = True
    doc = ezdxf.new("R2010", setup=True)
    doc.layers.add("Walls", color=7)
    doc.layers.add("Dimensions", color=3)

    valve_block = doc.blocks.new(name="Valve")
    valve_block.add_circle((0, 0), radius=15, dxfattribs={"layer": "Walls"})
    valve_block.add_line((-20, 0), (20, 0), dxfattribs={"layer": "Walls"})

    msp = doc.modelspace()
    msp.add_lwpolyline(
        [(0, 0), (420, 0), (420, 260), (0, 260), (0, 0)],
        dxfattribs={"layer": "Walls"},
    )
    msp.add_blockref("Valve", (210, 130), dxfattribs={"layer": "Walls"})
    msp.add_text(
        "North Wall 4200 mm",
        height=12,
        dxfattribs={"layer": "Walls", "insert": (20, 275)},
    )
    mtext = msp.add_mtext(
        "Structural partition\\PCheck clearance",
        dxfattribs={"layer": "Walls", "char_height": 10},
    )
    mtext.set_location((20, 220))

    dim = msp.add_linear_dim(
        base=(0, -40),
        p1=(0, 0),
        p2=(420, 0),
        dimstyle="EZDXF",
        dxfattribs={"layer": "Dimensions"},
    )
    dim.render()

    doc.classes.add_required_classes(doc.dxfversion)
    doc.classes.classes = dict(sorted(doc.classes.classes.items()))

    out = io.StringIO()
    doc.write(out)
    return out.getvalue().replace("\r\n", "\n").encode("utf-8")


def generate() -> None:
    """Write all cad/ fixtures to tests/fixtures/cad/."""
    write_bytes(FIXTURES_ROOT / "cad" / "drawing.dxf", _build_drawing_dxf())


def main() -> int:
    generate()
    return 0


if __name__ == "__main__":
    sys.exit(main())
