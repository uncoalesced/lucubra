# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
# /// script
# requires-python = ">=3.10"
# dependencies = [
#   "pillow>=11,<14",
#   "pypdf>=7,<8",
#   "pypdfium2>=5,<6",
#   "reportlab>=5,<6",
# ]
# ///
"""Generate and verify PDF fixtures under tests/fixtures/pdf/."""

from __future__ import annotations

import argparse
import io
import re
import sys
import unicodedata

from _common import FIXED_PDF_DATE, FIXTURES_ROOT, MALAYALAM_TEXT, write_bytes

TEXT_3PAGES_CONTENT: list[tuple[str, str | None, list[str]]] = [
    (
        "Unit 1 Cells: Structure and Membranes",
        "Unit 1 Cells",
        [
            "Eukaryotic cells organise biochemical pathways inside specialised organelles.",
            "The phospholipid bilayer regulates selective transport across the plasma boundary.",
            "Ribosomes translate messenger RNA templates into folded polypeptide chains.",
            "Mitochondria harbour folded cristae where oxidative reactions sustain cellular work.",
        ],
    ),
    (
        "Unit 2 Energy: Metabolic Pathways",
        "Unit 2 Energy",
        [
            "Living cells conserve free energy in adenosine triphosphate during catabolism.",
            "Glycolysis splits six-carbon glucose into pyruvate within the cytosolic compartment.",
            "Pyruvate enters the citric acid cycle to supply reduced nicotinamide cofactors.",
            "Proton gradients drive rotational catalysis across inner membrane synthase complexes.",
        ],
    ),
    (
        "Unit 2 Energy: Electron Transport and Phototrophy",
        None,
        [
            (
                "The electron transport chain transfers reducing equivalents "
                "toward terminal acceptors."
            ),
            "Chloroplasts capture visible photons in thylakoid antenna complexes during daylight.",
            "Carbon fixation incorporates inorganic dioxide into three-carbon sugar phosphates.",
            "Balanced metabolic coupling links autotrophic capture with heterotrophic respiration.",
        ],
    ),
]

MIXED_THRESHOLD_PAGES = [
    "Small figure note: xylem vessel rises upward.",
    "Plant chlorophyll absorbs solar photons well.",
    "Fresh chloroplasts capture solar photons well.",
]

TABLE_ROWS = [
    ["Element", "Symbol", "Atomic Number"],
    ["Carbon", "C", "6"],
    ["Oxygen", "O", "8"],
    ["Silicon", "Si", "14"],
]

INDIC_SPECS: dict[str, dict[str, object]] = {
    "pdf/indic-hindi.pdf": {
        "title": "Hindi Science Notes",
        "script": "Devanagari",
        "lines": [
            "कोशिका जीवन की सबसे छोटी संरचनात्मक इकाई है।",
            "हरितलवक में प्रकाश संश्लेषण की प्रक्रिया सूर्य के प्रकाश में होती है।",
            "जल और कार्बन डाइऑक्साइड मिलकर शर्करा तथा ऑक्सीजन बनाते हैं।",
        ],
        "search_words": ["कोशिका", "संश्लेषण", "ऑक्सीजन"],
    },
    "pdf/indic-kannada.pdf": {
        "title": "Kannada Science Notes",
        "script": "Kannada",
        "lines": [
            "ಜೀವಕೋಶವು ಜೀವಿಗಳ ಮೂಲಭೂತ ರಚನಾತ್ಮಕ ಘಟಕವಾಗಿದೆ.",
            "ಹಸಿರು ಎಲೆಗಳಲ್ಲಿ ಸೂರ್ಯನ ಬೆಳಕಿನಿಂದ ದ್ಯುತಿಸಂಶ್ಲೇಷಣೆ ನಡೆಯುತ್ತದೆ.",
            "ನೀರು ಮತ್ತು ಇಂಗಾಲದ ಡೈಆಕ್ಸೈಡ್ ಸೇರಿ ಗ್ಲೂಕೋಸ್ ಉತ್ಪತ್ತಿಯಾಗುತ್ತದೆ.",
        ],
        "search_words": ["ಜೀವಕೋಶವು", "ದ್ಯುತಿಸಂಶ್ಲೇಷಣೆ", "ಗ್ಲೂಕೋಸ್"],
    },
    "pdf/indic-malayalam.pdf": {
        "title": "Malayalam Science Notes",
        "script": "Malayalam",
        "lines": [line for line in MALAYALAM_TEXT.splitlines() if line.strip()],
        "search_words": ["പ്രകാശസംശ്ലേഷണം", "അവൻ", "അവ\u0d28\u0d4d\u200d", "കോശങ്ങളിലെ"],
    },
}


def manifest_entries() -> list[dict[str, object]]:
    """Return ADR 0011 manifest descriptors for all pdf/ fixtures."""
    entries: list[dict[str, object]] = [
        {
            "path": "pdf/text-3pages.pdf",
            "format": "pdf",
            "stub": False,
            "generator": "scripts/fixtures/gen_pdf.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "pages": 3,
                "needs_vision": [],
                "contains": [
                    "organelles",
                    "adenosine triphosphate",
                    "electron transport",
                ],
                "units_hint": ["Unit 1 Cells", "Unit 2 Energy"],
            },
            "notes": "Three text-rich pages (>=200 chars each) with two unit bookmarks",
        },
        {
            "path": "pdf/mixed-threshold.pdf",
            "format": "pdf",
            "stub": False,
            "generator": "scripts/fixtures/gen_pdf.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "pages": 3,
                "page_chars": [39, 40, 41],
                "needs_vision": [1],
                "contains": ["chlorophyll"],
            },
            "notes": "Boundary pages for the 40-character vision threshold",
        },
        {
            "path": "pdf/scanned-2pages.pdf",
            "format": "pdf",
            "stub": False,
            "generator": "scripts/fixtures/gen_pdf.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "pages": 2,
                "page_chars": [0, 0],
                "needs_vision": [1, 2],
            },
            "notes": "Two rasterised pages with no embedded text layer",
        },
        {
            "path": "pdf/table.pdf",
            "format": "pdf",
            "stub": False,
            "generator": "scripts/fixtures/gen_pdf.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "pages": 1,
                "needs_vision": [],
                "contains": [
                    "Element",
                    "Symbol",
                    "Atomic Number",
                    "Carbon",
                    "Oxygen",
                    "Silicon",
                ],
            },
            "notes": "Ruled 4x3 element table",
        },
        {
            "path": "pdf/encrypted.pdf",
            "format": "pdf",
            "stub": False,
            "generator": "scripts/fixtures/gen_pdf.py",
            "scenarios": ["S30"],
            "expect": {
                "outcome": "failed",
                "code": "encrypted",
            },
            "notes": "User password protected PDF (password: fixture)",
        },
        {
            "path": "pdf/corrupt-truncated.pdf",
            "format": "pdf",
            "stub": False,
            "generator": "scripts/fixtures/gen_pdf.py",
            "scenarios": ["S30"],
            "expect": {
                "outcome": "failed",
                "code": "corrupt",
            },
            "notes": "First half of the bytes of pdf/text-3pages.pdf",
        },
    ]
    for rel_path, spec in INDIC_SPECS.items():
        lines = spec["lines"]
        entries.append(
            {
                "path": rel_path,
                "format": "pdf",
                "stub": False,
                "generator": "scripts/fixtures/gen_pdf.py",
                "scenarios": ["S38"],
                "expect": {
                    "outcome": "extracted",
                    "pages": 1,
                    "needs_vision": [],
                    "script": spec["script"],
                    "search_words": spec["search_words"],
                    "contains": list(lines),
                },
                "notes": (
                    f"Original {spec['script']} sentences as text with a ToUnicode map; "
                    "no font is embedded"
                ),
            }
        )
    return entries


def _new_canvas(buf: io.BytesIO, title: str):
    from reportlab.pdfgen import canvas

    c = canvas.Canvas(buf, invariant=1)
    c.setAuthor("fixture")
    c.setCreator("fixture")
    c.setSubject("study fixture")
    c.setTitle(title)
    c._doc.info.producer = "ReportLab"
    return c


def _build_text_3pages() -> bytes:
    buf = io.BytesIO()
    c = _new_canvas(buf, "Cellular Biology and Bioenergetics")
    for idx, (heading, bookmark, lines) in enumerate(TEXT_3PAGES_CONTENT):
        if bookmark is not None:
            key = f"unit_{idx + 1}"
            c.bookmarkPage(key)
            c.addOutlineEntry(bookmark, key, level=0, closed=False)
        c.setFont("Helvetica-Bold", 14)
        c.drawString(72, 740, heading)
        c.setFont("Helvetica", 11)
        y = 708
        for line in lines:
            c.drawString(72, y, line)
            y -= 20
        c.showPage()
    c.save()
    return buf.getvalue()


def _build_mixed_threshold() -> bytes:
    buf = io.BytesIO()
    c = _new_canvas(buf, "Vision Threshold Pages")
    for text in MIXED_THRESHOLD_PAGES:
        c.setFont("Helvetica", 12)
        c.drawString(72, 720, text)
        c.showPage()
    c.save()
    return buf.getvalue()


def _build_scanned_2pages() -> bytes:
    from PIL import Image, ImageDraw, ImageFont
    from reportlab.lib.utils import ImageReader

    buf = io.BytesIO()
    c = _new_canvas(buf, "Scanned Biology Notes")
    font = ImageFont.load_default(size=26)
    pages_text = [
        [
            "Scanned Page 1: Membrane Lipids",
            "Amphipathic molecules form bilayers in water.",
            "Hydrophobic tails face inward away from solvent.",
        ],
        [
            "Scanned Page 2: Osmotic Balance",
            "Water diffuses across semipermeable barriers.",
            "Isotonic media prevent excessive cell swelling.",
        ],
    ]
    for lines in pages_text:
        img = Image.new("L", (800, 1000), color=250)
        draw = ImageDraw.Draw(img)
        y = 110
        for line in lines:
            draw.text((70, y), line, fill=20, font=font)
            y += 58
        png_buf = io.BytesIO()
        img.save(png_buf, format="PNG", optimize=False)
        png_buf.seek(0)
        c.drawImage(ImageReader(png_buf), 36, 42, width=523, height=654)
        c.showPage()
    c.save()
    return buf.getvalue()


def _build_table() -> bytes:
    from reportlab.lib import colors
    from reportlab.platypus import Table, TableStyle

    buf = io.BytesIO()
    c = _new_canvas(buf, "Element Properties Table")
    c.setFont("Helvetica-Bold", 13)
    c.drawString(72, 740, "Selected Elements")
    table = Table(TABLE_ROWS, colWidths=[140, 90, 120], rowHeights=26)
    table.setStyle(
        TableStyle(
            [
                ("FONTNAME", (0, 0), (-1, 0), "Helvetica-Bold"),
                ("FONTNAME", (0, 1), (-1, -1), "Helvetica"),
                ("FONTSIZE", (0, 0), (-1, -1), 11),
                ("GRID", (0, 0), (-1, -1), 0.75, colors.black),
                ("BACKGROUND", (0, 0), (-1, 0), colors.lightgrey),
                ("ALIGN", (1, 0), (-1, -1), "CENTER"),
                ("VALIGN", (0, 0), (-1, -1), "MIDDLE"),
            ]
        )
    )
    _, height = table.wrapOn(c, 400, 300)
    table.drawOn(c, 72, 700 - height)
    c.showPage()
    c.save()
    return buf.getvalue()


def _build_encrypted() -> bytes:
    import pypdf
    from pypdf.generic import ArrayObject, ByteStringObject

    src = io.BytesIO()
    c = _new_canvas(src, "Protected Study Sheet")
    c.setFont("Helvetica", 12)
    c.drawString(72, 720, "Restricted material encrypted for fixture testing.")
    c.showPage()
    c.save()

    writer = pypdf.PdfWriter()
    writer.append_pages_from_reader(pypdf.PdfReader(io.BytesIO(src.getvalue())))
    writer.add_metadata(
        {
            "/Author": "fixture",
            "/Creator": "fixture",
            "/Producer": "pypdf",
            "/Title": "Protected Study Sheet",
            "/CreationDate": FIXED_PDF_DATE,
            "/ModDate": FIXED_PDF_DATE,
        }
    )
    fixed_id = ByteStringObject(b"lucubra-pdf-enc0")
    writer._ID = ArrayObject((fixed_id, fixed_id))
    writer.generate_file_identifiers = lambda: None
    writer.encrypt("fixture", algorithm="RC4-128")
    out = io.BytesIO()
    writer.write(out)
    return out.getvalue()


def _build_indic(spec: dict[str, object]) -> bytes:
    """A one-page PDF whose text is extractable but embeds no font program.

    Each distinct character gets a two-byte code (Identity-H) and a ToUnicode
    entry, so text extraction returns the original sentences. The font is
    named but not embedded, which keeps proprietary or licensed font data out
    of the corpus. Viewers substitute a font, so the page may not render the
    script correctly; extraction is what the fixture tests.
    """
    lines = [str(line) for line in spec["lines"]]  # type: ignore[attr-defined]
    chars = sorted({ch for line in lines for ch in line})
    code = {ch: i + 1 for i, ch in enumerate(chars)}

    def hex_of(line: str) -> str:
        return "".join(f"{code[ch]:04X}" for ch in line)

    ops = ["BT", "/F1 13 Tf", "16 TL", "54 730 Td"]
    for line in lines:
        ops.append(f"<{hex_of(line)}> Tj T*")
    ops.append("ET")
    content = "\n".join(ops).encode("ascii")

    bf = "\n".join(f"<{code[ch]:04X}> <{ord(ch):04X}>" for ch in chars)
    cmap = (
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n"
        "/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n"
        "/CMapName /Lucubra-UCS def\n/CMapType 2 def\n"
        "1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n"
        f"{len(chars)} beginbfchar\n{bf}\nendbfchar\n"
        "endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend"
    ).encode("ascii")

    title = str(spec["title"]).encode("ascii", "replace")
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
        b"/Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
        b"<< /Type /Font /Subtype /Type0 /BaseFont /LucubraFixture "
        b"/Encoding /Identity-H /DescendantFonts [6 0 R] /ToUnicode 7 0 R >>",
        b"<< /Length %d >>\nstream\n" % len(content) + content + b"\nendstream",
        b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /LucubraFixture "
        b"/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> "
        b"/FontDescriptor 8 0 R /DW 600 /CIDToGIDMap /Identity >>",
        b"<< /Length %d >>\nstream\n" % len(cmap) + cmap + b"\nendstream",
        b"<< /Type /FontDescriptor /FontName /LucubraFixture /Flags 32 "
        b"/FontBBox [0 -250 1000 900] /ItalicAngle 0 /Ascent 900 /Descent -250 "
        b"/CapHeight 700 /StemV 80 >>",
        b"<< /Title (" + title + b") /Producer (fixture) >>",
    ]
    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets = []
    for n, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out += b"%d 0 obj\n" % n + body + b"\nendobj\n"
    xref = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objects) + 1)
    for off in offsets:
        out += b"%010d 00000 n \n" % off
    out += b"trailer\n<< /Size %d /Root 1 0 R /Info %d 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (
        len(objects) + 1,
        len(objects),
        xref,
    )
    return bytes(out)


def _non_ws_counts(pdf_bytes: bytes) -> list[int]:
    import pypdfium2 as pdfium

    doc = pdfium.PdfDocument(pdf_bytes)
    try:
        counts = []
        for page in doc:
            txt = page.get_textpage().get_text_range()
            counts.append(len(re.sub(r"\s+", "", txt)))
        return counts
    finally:
        doc.close()


def generate() -> None:
    """Build every pdf/ fixture and validate threshold and scanned page counts."""
    text_3p = _build_text_3pages()
    mixed = _build_mixed_threshold()
    scanned = _build_scanned_2pages()
    table = _build_table()
    encrypted = _build_encrypted()

    mixed_counts = _non_ws_counts(mixed)
    if mixed_counts != [39, 40, 41]:
        raise RuntimeError(f"mixed-threshold.pdf page_chars={mixed_counts}, expected [39, 40, 41]")
    scanned_counts = _non_ws_counts(scanned)
    if scanned_counts != [0, 0]:
        raise RuntimeError(f"scanned-2pages.pdf page_chars={scanned_counts}, expected [0, 0]")

    outputs: dict[str, bytes] = {
        "pdf/text-3pages.pdf": text_3p,
        "pdf/mixed-threshold.pdf": mixed,
        "pdf/scanned-2pages.pdf": scanned,
        "pdf/table.pdf": table,
        "pdf/encrypted.pdf": encrypted,
        "pdf/corrupt-truncated.pdf": text_3p[: len(text_3p) // 2],
    }
    for rel_path, spec in INDIC_SPECS.items():
        outputs[rel_path] = _build_indic(spec)

    for rel_path, data in outputs.items():
        write_bytes(FIXTURES_ROOT / rel_path, data)


def verify_indic() -> int:
    """Extract Indic PDFs with pypdfium2 and verify NFC match against source strings."""
    import pypdfium2 as pdfium

    all_ok = True
    for rel_path, spec in INDIC_SPECS.items():
        pdf_path = FIXTURES_ROOT / rel_path
        doc = pdfium.PdfDocument(pdf_path)
        try:
            extracted_raw = "\n".join(page.get_textpage().get_text_range() for page in doc)
        finally:
            doc.close()
        extracted_nfc = unicodedata.normalize("NFC", extracted_raw)
        extracted_lines = [line.strip() for line in extracted_nfc.splitlines() if line.strip()]
        expected_lines = [unicodedata.normalize("NFC", str(line)) for line in spec["lines"]]  # type: ignore[attr-defined]
        print(f"=== {rel_path} ({spec['script']}) ===")
        for idx, exp in enumerate(expected_lines):
            got = extracted_lines[idx] if idx < len(extracted_lines) else ""
            matched = exp == got
            all_ok = all_ok and matched
            print(f"  line {idx + 1}: match={matched}")
            print(f"    expected: {exp}")
            print(f"    got     : {got}")
        for word in spec["search_words"]:  # type: ignore[attr-defined]
            w_nfc = unicodedata.normalize("NFC", str(word))
            found = w_nfc in extracted_nfc
            all_ok = all_ok and found
            print(f"  word {w_nfc!r}: found={found}")
    return 0 if all_ok else 1


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--verify",
        action="store_true",
        help="Extract Indic PDFs via pypdfium2 and verify NFC round-trip matches",
    )
    args = parser.parse_args(argv)
    if args.verify:
        return verify_indic()
    generate()
    return 0


if __name__ == "__main__":
    sys.exit(main())
