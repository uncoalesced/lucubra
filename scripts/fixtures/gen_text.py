# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
"""Generate text, markup, code, notebook, EPUB, and Indic text fixtures."""

from __future__ import annotations

import io
import json
import sys
import unicodedata
import zipfile

from _common import (
    FIXED_ISO_DATE,
    FIXED_ZIP_TIME,
    FIXTURES_ROOT,
    MALAYALAM_TEXT,
    write_bytes,
    write_text_crlf_bom,
    write_text_lf,
)


def manifest_entries() -> list[dict[str, object]]:
    """Return ADR 0011 manifest descriptors for all text/ fixtures."""
    malayalam_lines = [line for line in MALAYALAM_TEXT.splitlines() if line.strip()]
    return [
        {
            "path": "text/notes.md",
            "format": "md",
            "stub": False,
            "generator": "scripts/fixtures/gen_text.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "contains": ["Wave Optics Summary", "Snell's Law", "Refractive Index"],
            },
            "notes": "UTF-8 LF Markdown study notes with headings",
        },
        {
            "path": "text/notes-crlf-bom.md",
            "format": "md",
            "stub": False,
            "generator": "scripts/fixtures/gen_text.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "contains": ["Chemical Equilibrium", "Le Chatelier"],
            },
            "notes": "UTF-8 with BOM and CRLF line endings",
        },
        {
            "path": "text/plain.txt",
            "format": "txt",
            "stub": False,
            "generator": "scripts/fixtures/gen_text.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "contains": ["Newtonian mechanics", "conservation of linear momentum"],
            },
            "notes": "Plain UTF-8 LF text study note",
        },
        {
            "path": "text/page.html",
            "format": "html",
            "stub": False,
            "generator": "scripts/fixtures/gen_text.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "contains": ["Acoustic Resonance", "Standing waves form in open pipes"],
            },
            "notes": "Clean HTML5 document with headings and list items",
        },
        {
            "path": "text/sample.c",
            "format": "code",
            "stub": False,
            "generator": "scripts/fixtures/gen_text.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "contains": ["greatest_common_divisor", "while (b != 0)"],
            },
            "notes": "Original C source fixture",
        },
        {
            "path": "text/Sample.java",
            "format": "code",
            "stub": False,
            "generator": "scripts/fixtures/gen_text.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "contains": ["class Sample", "binarySearch"],
            },
            "notes": "Original Java source fixture",
        },
        {
            "path": "text/notebook.ipynb",
            "format": "ipynb",
            "stub": False,
            "generator": "scripts/fixtures/gen_text.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "contains": ["Harmonic Oscillator", "def potential_energy", "0.125"],
            },
            "notes": "Jupyter notebook with one Markdown cell and two ruff-clean code cells",
        },
        {
            "path": "text/book.epub",
            "format": "epub",
            "stub": False,
            "generator": "scripts/fixtures/gen_text.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "contains": ["Chapter 1: Cell Division", "Chapter 2: Gene Expression"],
            },
            "notes": "Hand-built EPUB 3 archive with stored mimetype entry first and two chapters",
        },
        {
            "path": "text/malayalam-nfd.txt",
            "format": "txt",
            "stub": False,
            "generator": "scripts/fixtures/gen_text.py",
            "scenarios": ["S38"],
            "expect": {
                "outcome": "extracted",
                "script": "Malayalam",
                "contains": malayalam_lines,
            },
            "notes": "NFD-normalised copy of the Malayalam chillu and ZWJ test text",
        },
        {
            "path": "text/malayalam-nfc.txt",
            "format": "txt",
            "stub": False,
            "generator": "scripts/fixtures/gen_text.py",
            "scenarios": ["S38"],
            "expect": {
                "outcome": "extracted",
                "script": "Malayalam",
                "contains": malayalam_lines,
            },
            "notes": "NFC-normalised copy of the Malayalam chillu and ZWJ test text",
        },
    ]


def _build_notebook_json() -> str:
    nb = {
        "cells": [
            {
                "cell_type": "markdown",
                "metadata": {},
                "source": [
                    "# Harmonic Oscillator\n",
                    "\n",
                    "Potential energy varies quadratically with displacement from equilibrium.\n",
                ],
            },
            {
                "cell_type": "code",
                "execution_count": 1,
                "metadata": {},
                "outputs": [
                    {
                        "name": "stdout",
                        "output_type": "stream",
                        "text": ["0.125\n"],
                    }
                ],
                "source": [
                    "def potential_energy(stiffness: float, displacement: float) -> float:\n",
                    "    return 0.5 * stiffness * displacement * displacement\n",
                    "\n",
                    "\n",
                    "print(potential_energy(10.0, 0.5))",
                ],
            },
            {
                "cell_type": "code",
                "execution_count": 2,
                "metadata": {},
                "outputs": [
                    {
                        "data": {"text/plain": ["2.5"]},
                        "execution_count": 2,
                        "metadata": {},
                        "output_type": "execute_result",
                    }
                ],
                "source": [
                    "def spring_force(stiffness: float, displacement: float) -> float:\n",
                    "    return -stiffness * displacement\n",
                    "\n",
                    "\n",
                    "abs(spring_force(5.0, 0.5))",
                ],
            },
        ],
        "metadata": {
            "kernelspec": {
                "display_name": "Python 3",
                "language": "python",
                "name": "python3",
            },
            "language_info": {"name": "python", "version": "3.11"},
        },
        "nbformat": 4,
        "nbformat_minor": 5,
    }
    return json.dumps(nb, indent=2) + "\n"


def _build_epub_bytes() -> bytes:
    mimetype = b"application/epub+zip"
    container_xml = (
        b'<?xml version="1.0" encoding="UTF-8"?>\n'
        b'<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">\n'
        b"  <rootfiles>\n"
        b'    <rootfile full-path="OEBPS/content.opf" '
        b'media-type="application/oebps-package+xml"/>\n'
        b"  </rootfiles>\n"
        b"</container>\n"
    )
    content_opf = (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="bookid" version="3.0">\n'
        '  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">\n'
        '    <dc:identifier id="bookid">'
        "urn:uuid:00000000-0000-4000-8000-000000000001</dc:identifier>\n"
        "    <dc:title>Molecular Biology Primer</dc:title>\n"
        "    <dc:creator>fixture</dc:creator>\n"
        "    <dc:language>en</dc:language>\n"
        f'    <meta property="dcterms:modified">{FIXED_ISO_DATE}</meta>\n'
        "  </metadata>\n"
        "  <manifest>\n"
        '    <item id="nav" href="nav.xhtml" '
        'media-type="application/xhtml+xml" properties="nav"/>\n'
        '    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>\n'
        '    <item id="ch2" href="ch2.xhtml" media-type="application/xhtml+xml"/>\n'
        "  </manifest>\n"
        "  <spine>\n"
        '    <itemref idref="ch1"/>\n'
        '    <itemref idref="ch2"/>\n'
        "  </spine>\n"
        "</package>\n"
    ).encode()
    nav_xhtml = (
        b'<?xml version="1.0" encoding="UTF-8"?>\n'
        b'<html xmlns="http://www.w3.org/1999/xhtml" '
        b'xmlns:epub="http://www.idpf.org/2007/ops">\n'
        b"<head><title>Contents</title></head>\n"
        b"<body>\n"
        b'  <nav epub:type="toc">\n'
        b"    <ol>\n"
        b'      <li><a href="ch1.xhtml">Chapter 1: Cell Division</a></li>\n'
        b'      <li><a href="ch2.xhtml">Chapter 2: Gene Expression</a></li>\n'
        b"    </ol>\n"
        b"  </nav>\n"
        b"</body>\n"
        b"</html>\n"
    )
    ch1_xhtml = (
        b'<?xml version="1.0" encoding="UTF-8"?>\n'
        b'<html xmlns="http://www.w3.org/1999/xhtml">\n'
        b"<head><title>Chapter 1: Cell Division</title></head>\n"
        b"<body>\n"
        b"  <h1>Chapter 1: Cell Division</h1>\n"
        b"  <p>Mitotic spindle fibres segregate duplicated sister chromatids "
        b"into daughter nuclei.</p>\n"
        b"</body>\n"
        b"</html>\n"
    )
    ch2_xhtml = (
        b'<?xml version="1.0" encoding="UTF-8"?>\n'
        b'<html xmlns="http://www.w3.org/1999/xhtml">\n'
        b"<head><title>Chapter 2: Gene Expression</title></head>\n"
        b"<body>\n"
        b"  <h1>Chapter 2: Gene Expression</h1>\n"
        b"  <p>RNA polymerase transcribes coding sequences before cytosolic "
        b"translation occurs.</p>\n"
        b"</body>\n"
        b"</html>\n"
    )

    out = io.BytesIO()
    with zipfile.ZipFile(out, "w", compression=zipfile.ZIP_DEFLATED) as zout:
        entries = [
            ("mimetype", mimetype, zipfile.ZIP_STORED),
            ("META-INF/container.xml", container_xml, zipfile.ZIP_DEFLATED),
            ("OEBPS/content.opf", content_opf, zipfile.ZIP_DEFLATED),
            ("OEBPS/nav.xhtml", nav_xhtml, zipfile.ZIP_DEFLATED),
            ("OEBPS/ch1.xhtml", ch1_xhtml, zipfile.ZIP_DEFLATED),
            ("OEBPS/ch2.xhtml", ch2_xhtml, zipfile.ZIP_DEFLATED),
        ]
        for name, data, comp_type in entries:
            info = zipfile.ZipInfo(name, date_time=FIXED_ZIP_TIME)
            info.compress_type = comp_type
            info.external_attr = 0o644 << 16
            zout.writestr(info, data)
    return out.getvalue()


def generate() -> None:
    """Write all text/ fixtures to tests/fixtures/text/."""
    notes_md = (
        "# Wave Optics Summary\n\n"
        "## Refractive Index\n\n"
        "Phase velocity slows when monochromatic light enters an optically denser medium.\n\n"
        "## Snell's Law\n\n"
        "The ratio of sines of incidence and refraction equals the inverse index ratio.\n"
    )
    notes_crlf = (
        "# Chemical Equilibrium\n\n"
        "## Le Chatelier Principle\n\n"
        "Raising reactant concentration shifts reversible reactions toward products.\n"
    )
    plain_txt = (
        "Newtonian mechanics links net external force to linear momentum change.\n"
        "In an isolated collision without external impulse, conservation of linear "
        "momentum holds.\n"
    )
    page_html = (
        "<!DOCTYPE html>\n"
        '<html lang="en">\n'
        "<head>\n"
        '  <meta charset="utf-8">\n'
        "  <title>Acoustic Resonance</title>\n"
        "</head>\n"
        "<body>\n"
        "  <h1>Acoustic Resonance</h1>\n"
        "  <p>Standing waves form in open pipes at integer multiples of the fundamental.</p>\n"
        "  <h2>Harmonic Series</h2>\n"
        "  <ul><li>First harmonic: antinodes at both open ends.</li></ul>\n"
        "</body>\n"
        "</html>\n"
    )
    sample_c = (
        "#include <stdio.h>\n\n"
        "int greatest_common_divisor(int a, int b) {\n"
        "    while (b != 0) {\n"
        "        int next = a % b;\n"
        "        a = b;\n"
        "        b = next;\n"
        "    }\n"
        "    return a;\n"
        "}\n"
    )
    sample_java = (
        "public final class Sample {\n"
        "    public static int binarySearch(int[] values, int target) {\n"
        "        int low = 0;\n"
        "        int high = values.length - 1;\n"
        "        while (low <= high) {\n"
        "            int mid = low + (high - low) / 2;\n"
        "            if (values[mid] == target) {\n"
        "                return mid;\n"
        "            }\n"
        "            if (values[mid] < target) {\n"
        "                low = mid + 1;\n"
        "            } else {\n"
        "                high = mid - 1;\n"
        "            }\n"
        "        }\n"
        "        return -1;\n"
        "    }\n"
        "}\n"
    )

    nfd_text = unicodedata.normalize("NFD", MALAYALAM_TEXT)
    nfc_text = unicodedata.normalize("NFC", MALAYALAM_TEXT)
    if nfd_text == nfc_text:
        raise RuntimeError("Malayalam fixture text must differ between NFD and NFC forms")

    write_text_lf(FIXTURES_ROOT / "text" / "notes.md", notes_md)
    write_text_crlf_bom(FIXTURES_ROOT / "text" / "notes-crlf-bom.md", notes_crlf)
    write_text_lf(FIXTURES_ROOT / "text" / "plain.txt", plain_txt)
    write_text_lf(FIXTURES_ROOT / "text" / "page.html", page_html)
    write_text_lf(FIXTURES_ROOT / "text" / "sample.c", sample_c)
    write_text_lf(FIXTURES_ROOT / "text" / "Sample.java", sample_java)
    write_text_lf(FIXTURES_ROOT / "text" / "notebook.ipynb", _build_notebook_json())
    write_bytes(FIXTURES_ROOT / "text" / "book.epub", _build_epub_bytes())
    write_text_lf(FIXTURES_ROOT / "text" / "malayalam-nfd.txt", nfd_text)
    write_text_lf(FIXTURES_ROOT / "text" / "malayalam-nfc.txt", nfc_text)


def main() -> int:
    generate()
    return 0


if __name__ == "__main__":
    sys.exit(main())
