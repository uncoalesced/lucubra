# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
# /// script
# requires-python = ">=3.10"
# dependencies = [
#   "pillow>=11,<14",
#   "python-pptx>=1,<2",
# ]
# ///
"""Generate PPTX fixtures under tests/fixtures/pptx/."""

from __future__ import annotations

import io
import sys

from _common import FIXED_DT, FIXTURES_ROOT, repack_zip_deterministic, write_bytes


def manifest_entries() -> list[dict[str, object]]:
    """Return ADR 0011 manifest descriptors for all pptx/ fixtures."""
    return [
        {
            "path": "pptx/notes-and-image.pptx",
            "format": "pptx",
            "stub": False,
            "generator": "scripts/fixtures/gen_pptx.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "slides": 3,
                "needs_vision": [3],
                "contains": [
                    "First Law of Thermodynamics",
                    "Internal energy changes through heat transfer",
                    "Isolated systems evolve toward equilibrium",
                ],
                "notes_contains": [
                    "reversible isothermal expansion",
                ],
            },
            "notes": "Slide 1 title+body, slide 2 body+speaker notes, slide 3 image only",
        }
    ]


def _make_slide_diagram_png() -> bytes:
    from PIL import Image, ImageDraw

    img = Image.new("RGB", (480, 320), color=(248, 249, 250))
    draw = ImageDraw.Draw(img)
    draw.rectangle((40, 40, 440, 280), outline=(40, 60, 90), width=4)
    draw.line((80, 240, 400, 80), fill=(190, 60, 50), width=5)
    draw.ellipse((215, 135, 265, 185), outline=(30, 120, 70), width=4)
    buf = io.BytesIO()
    img.save(buf, format="PNG")
    return buf.getvalue()


def _build_notes_and_image_pptx() -> bytes:
    from pptx import Presentation
    from pptx.util import Inches

    prs = Presentation()
    props = prs.core_properties
    props.author = "fixture"
    props.title = "Thermodynamics Lecture"
    props.subject = "Physics"
    props.comments = "Study fixture"
    props.last_modified_by = "fixture"
    props.created = FIXED_DT
    props.modified = FIXED_DT

    # Slide 1: title and body
    slide1 = prs.slides.add_slide(prs.slide_layouts[1])
    slide1.shapes.title.text = "First Law of Thermodynamics"
    slide1.placeholders[
        1
    ].text = "Internal energy changes through heat transfer and mechanical work across a boundary."

    # Slide 2: body plus speaker notes
    slide2 = prs.slides.add_slide(prs.slide_layouts[1])
    slide2.shapes.title.text = "Entropy and Spontaneity"
    slide2.placeholders[
        1
    ].text = "Isolated systems evolve toward equilibrium as microstate multiplicity increases."
    notes2 = slide2.notes_slide
    notes2.notes_text_frame.text = (
        "Speaker note: contrast reversible isothermal expansion with free Joule expansion."
    )

    # Slide 3: one picture and no text
    slide3 = prs.slides.add_slide(prs.slide_layouts[6])
    img_stream = io.BytesIO(_make_slide_diagram_png())
    slide3.shapes.add_picture(img_stream, Inches(1.5), Inches(1.2), width=Inches(6.0))

    out = io.BytesIO()
    prs.save(out)
    return repack_zip_deterministic(out.getvalue())


def generate() -> None:
    """Write all pptx/ fixtures to tests/fixtures/pptx/."""
    write_bytes(
        FIXTURES_ROOT / "pptx" / "notes-and-image.pptx",
        _build_notes_and_image_pptx(),
    )


def main() -> int:
    generate()
    return 0


if __name__ == "__main__":
    sys.exit(main())
