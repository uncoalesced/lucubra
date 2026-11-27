# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
# /// script
# requires-python = ">=3.10"
# dependencies = [
#   "pillow>=11,<14",
# ]
# ///
"""Generate image fixtures under tests/fixtures/image/."""

from __future__ import annotations

import io
import math
import random
import sys

from _common import FIXTURES_ROOT, write_bytes


def _font(size: int):
    """Pillow's bundled Aileron Regular (CC0), so no system or third-party font is needed."""
    from PIL import ImageFont

    return ImageFont.load_default(size=size)


def manifest_entries() -> list[dict[str, object]]:
    """Return ADR 0011 manifest descriptors for all image/ fixtures."""
    return [
        {
            "path": "image/photo-notes.png",
            "format": "image",
            "stub": False,
            "generator": "scripts/fixtures/gen_image.py",
            "scenarios": ["S05"],
            "expect": {
                "outcome": "extracted",
                "needs_vision": [1],
            },
            "notes": "Synthetic handwritten-style chemistry notes on lined paper texture",
        },
        {
            "path": "image/diagram.jpg",
            "format": "image",
            "stub": False,
            "generator": "scripts/fixtures/gen_image.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "needs_vision": [1],
            },
            "notes": "Synthetic inclined-plane mechanics diagram as JPEG",
        },
        {
            "path": "image/sample.webp",
            "format": "image",
            "stub": False,
            "generator": "scripts/fixtures/gen_image.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "needs_vision": [1],
            },
            "notes": "Synthetic damped oscillation plot as WebP",
        },
    ]


def _build_photo_notes_png() -> bytes:
    from PIL import Image, ImageDraw

    rng = random.Random(20261114)
    width, height = 760, 520
    img = Image.new("RGB", (width, height), color=(248, 245, 236))
    pixels = img.load()
    for y in range(height):
        for x in range(width):
            delta = rng.randint(-4, 4)
            r = max(0, min(255, 248 + delta))
            g = max(0, min(255, 245 + delta))
            b = max(0, min(255, 236 + delta))
            pixels[x, y] = (r, g, b)

    draw = ImageDraw.Draw(img)
    for line_y in range(85, height - 30, 52):
        draw.line((30, line_y, width - 30, line_y), fill=(195, 210, 228), width=1)
    draw.line((88, 20, 88, height - 20), fill=(224, 165, 165), width=2)

    font_title = _font(28)
    font_body = _font(22)
    lines = [
        (108, 46, "Reaction Kinetics Notes", font_title),
        (108, 102, "Second-order rate law: rate = k [A]^2", font_body),
        (108, 154, "Half-life t_1/2 = 1 / (k * [A]_0)", font_body),
        (108, 206, "Plot 1/[A] against time t -> linear slope k", font_body),
        (108, 258, "Arrhenius: ln(k) = ln(A) - Ea / (R * T)", font_body),
    ]
    for x, y, text, fnt in lines:
        draw.text((x, y), text, fill=(28, 48, 92), font=fnt)

    # Underline heading with slight hand-drawn jitter
    pts = [(108 + i * 25, 81 + rng.randint(-1, 2)) for i in range(13)]
    draw.line(pts, fill=(28, 48, 92), width=2)

    buf = io.BytesIO()
    img.save(buf, format="PNG")
    return buf.getvalue()


def _build_diagram_jpg() -> bytes:
    from PIL import Image, ImageDraw

    img = Image.new("RGB", (640, 400), color=(255, 255, 255))
    draw = ImageDraw.Draw(img)
    font = _font(18)

    # Inclined plane triangle
    draw.polygon([(80, 320), (540, 320), (540, 120)], outline=(30, 30, 30), fill=(238, 242, 247))
    # Mass block on slope
    draw.polygon([(270, 237), (330, 211), (306, 156), (246, 182)], fill=(195, 218, 240))
    draw.line(
        [(270, 237), (330, 211), (306, 156), (246, 182), (270, 237)],
        fill=(25, 65, 120),
        width=3,
    )
    # Gravity and normal vectors
    draw.line((288, 196, 288, 300), fill=(185, 35, 35), width=3)
    draw.line((288, 196, 255, 120), fill=(35, 130, 60), width=3)
    draw.text((296, 282), "mg", fill=(185, 35, 35), font=font)
    draw.text((242, 95), "N", fill=(35, 130, 60), font=font)
    draw.text((150, 294), "theta = 23 deg", fill=(30, 30, 30), font=font)

    buf = io.BytesIO()
    img.save(buf, format="JPEG", quality=90, optimize=False)
    return buf.getvalue()


def _build_sample_webp() -> bytes:
    from PIL import Image, ImageDraw

    img = Image.new("RGB", (560, 320), color=(250, 250, 252))
    draw = ImageDraw.Draw(img)
    font = _font(16)

    # Axes
    draw.line((50, 270, 520, 270), fill=(40, 40, 40), width=2)
    draw.line((50, 40, 50, 270), fill=(40, 40, 40), width=2)

    pts = []
    for step in range(440):
        t = step / 40.0
        y_val = math.exp(-0.22 * t) * math.cos(2.6 * t)
        px = 55 + step
        py = int(155 - y_val * 95)
        pts.append((px, py))
    draw.line(pts, fill=(35, 95, 175), width=3)
    draw.text((170, 14), "Underdamped Oscillation x(t)", fill=(30, 30, 30), font=font)

    buf = io.BytesIO()
    img.save(buf, format="WEBP", quality=90, method=4)
    return buf.getvalue()


def generate() -> None:
    """Write all image/ fixtures to tests/fixtures/image/."""
    write_bytes(FIXTURES_ROOT / "image" / "photo-notes.png", _build_photo_notes_png())
    write_bytes(FIXTURES_ROOT / "image" / "diagram.jpg", _build_diagram_jpg())
    write_bytes(FIXTURES_ROOT / "image" / "sample.webp", _build_sample_webp())


def main() -> int:
    generate()
    return 0


if __name__ == "__main__":
    sys.exit(main())
