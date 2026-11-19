# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
# /// script
# requires-python = ">=3.10"
# dependencies = [
#   "python-docx>=1.1,<2",
# ]
# ///
"""Generate DOCX fixtures under tests/fixtures/docx/."""

from __future__ import annotations

import io
import struct
import sys
import zlib

from _common import FIXED_DT, FIXTURES_ROOT, repack_zip_deterministic, write_bytes

ZIP_MOD_TIME = 0
ZIP_MOD_DATE = 0x0021  # 1980-01-01


def manifest_entries() -> list[dict[str, object]]:
    """Return ADR 0011 manifest descriptors for all docx/ fixtures."""
    return [
        {
            "path": "docx/headings-table.docx",
            "format": "docx",
            "stub": False,
            "generator": "scripts/fixtures/gen_docx.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "contains": [
                    "Cell Structure Overview",
                    "Membrane Transport",
                    "Organelle",
                    "Mitochondrion",
                    "ATP synthesis",
                ],
            },
            "notes": "Heading 1, Heading 2, study paragraphs, and a 4x2 organelle table",
        },
        {
            "path": "docx/bomb-honest.docx",
            "format": "docx",
            "stub": False,
            "generator": "scripts/fixtures/gen_docx.py",
            "scenarios": ["S30"],
            "expect": {
                "outcome": "failed",
                "code": "limit_exceeded",
            },
            "notes": "Valid DOCX with an extra 1 GiB zero entry declared truthfully in zip headers",
        },
        {
            "path": "docx/bomb-lying.docx",
            "format": "docx",
            "stub": False,
            "generator": "scripts/fixtures/gen_docx.py",
            "scenarios": ["S30"],
            "expect": {
                "outcome": "failed",
                "code": "limit_exceeded",
            },
            "notes": "a reader that stops at the declared size reports corrupt",
        },
    ]


def _build_headings_table_docx() -> bytes:
    import docx

    doc = docx.Document()
    props = doc.core_properties
    props.author = "fixture"
    props.title = "Cell Structure Study Guide"
    props.subject = "Biology"
    props.comments = "Study fixture"
    props.last_modified_by = "fixture"
    props.created = FIXED_DT
    props.modified = FIXED_DT

    doc.add_heading("Cell Structure Overview", level=1)
    doc.add_paragraph(
        "Eukaryotic cells compartmentalise reactions inside membrane-bound organelles "
        "that separate incompatible enzymes."
    )
    doc.add_heading("Membrane Transport", level=2)
    doc.add_paragraph(
        "Channel and carrier proteins move polar solutes across the hydrophobic bilayer "
        "along or against electrochemical gradients."
    )

    rows = [
        ("Organelle", "Primary Role"),
        ("Nucleus", "Chromatin storage and transcription"),
        ("Mitochondrion", "Oxidative phosphorylation and ATP synthesis"),
        ("Golgi apparatus", "Protein glycosylation and vesicle sorting"),
    ]
    table = doc.add_table(rows=len(rows), cols=2)
    for r_idx, (col0, col1) in enumerate(rows):
        table.cell(r_idx, 0).text = col0
        table.cell(r_idx, 1).text = col1

    buf = io.BytesIO()
    doc.save(buf)
    return repack_zip_deterministic(buf.getvalue())


def _deflate_zeros(total_bytes: int) -> tuple[bytes, int]:
    """Stream-compress `total_bytes` of zeros with raw deflate and return (compressed, crc32)."""
    chunk = b"\x00" * (1024 * 1024)
    compressor = zlib.compressobj(level=9, method=zlib.DEFLATED, wbits=-15)
    parts: list[bytes] = []
    crc = 0
    remaining = total_bytes
    while remaining > 0:
        piece = chunk if remaining >= len(chunk) else chunk[:remaining]
        crc = zlib.crc32(piece, crc)
        compressed = compressor.compress(piece)
        if compressed:
            parts.append(compressed)
        remaining -= len(piece)
    parts.append(compressor.flush())
    return b"".join(parts), crc & 0xFFFFFFFF


def _pack_zip_entries(
    entries: list[tuple[str, bytes, int, int, int]],
) -> bytes:
    """Assemble a ZIP archive from (name, comp_bytes, crc32, actual_uncomp, declared_uncomp)."""
    body = io.BytesIO()
    central = io.BytesIO()
    for name, comp_bytes, crc, _actual_uncomp, declared_uncomp in entries:
        name_bytes = name.encode("utf-8")
        offset = body.tell()
        local_hdr = struct.pack(
            "<IHHHHHIIIHH",
            0x04034B50,
            20,
            0,
            8,
            ZIP_MOD_TIME,
            ZIP_MOD_DATE,
            crc,
            len(comp_bytes),
            declared_uncomp,
            len(name_bytes),
            0,
        )
        body.write(local_hdr)
        body.write(name_bytes)
        body.write(comp_bytes)

        cd_hdr = struct.pack(
            "<IHHHHHHIIIHHHHHII",
            0x02014B50,
            20,
            20,
            0,
            8,
            ZIP_MOD_TIME,
            ZIP_MOD_DATE,
            crc,
            len(comp_bytes),
            declared_uncomp,
            len(name_bytes),
            0,
            0,
            0,
            0,
            0o644 << 16,
            offset,
        )
        central.write(cd_hdr)
        central.write(name_bytes)

    cd_offset = body.tell()
    cd_bytes = central.getvalue()
    body.write(cd_bytes)
    eocd = struct.pack(
        "<IHHHHIIH",
        0x06054B50,
        0,
        0,
        len(entries),
        len(entries),
        len(cd_bytes),
        cd_offset,
        0,
    )
    body.write(eocd)
    return body.getvalue()


def _minimal_docx_entries() -> list[tuple[str, bytes, int, int, int]]:
    content_types = (
        b'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        b'<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        b'<Default Extension="rels" '
        b'ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
        b'<Default Extension="xml" ContentType="application/xml"/>'
        b'<Default Extension="bin" ContentType="application/octet-stream"/>'
        b'<Override PartName="/word/document.xml" '
        b"ContentType="
        b'"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
        b'<Override PartName="/docProps/core.xml" '
        b'ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>'
        b"</Types>"
    )
    rels = (
        b'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        b'<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        b'<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/'
        b'officeDocument/2006/relationships/officeDocument" '
        b'Target="word/document.xml"/>'
        b'<Relationship Id="rId2" '
        b"Type="
        b'"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" '
        b'Target="docProps/core.xml"/>'
        b"</Relationships>"
    )
    core_xml = (
        b'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        b"<cp:coreProperties "
        b'xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" '
        b'xmlns:dc="http://purl.org/dc/elements/1.1/" '
        b'xmlns:dcterms="http://purl.org/dc/terms/" '
        b'xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">'
        b"<dc:title>Archive Limit Fixture</dc:title>"
        b"<dc:creator>fixture</dc:creator>"
        b"<cp:lastModifiedBy>fixture</cp:lastModifiedBy>"
        b'<dcterms:created xsi:type="dcterms:W3CDTF">2026-01-01T00:00:00Z</dcterms:created>'
        b'<dcterms:modified xsi:type="dcterms:W3CDTF">2026-01-01T00:00:00Z</dcterms:modified>'
        b"</cp:coreProperties>"
    )
    document_xml = (
        b'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        b'<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">'
        b"<w:body><w:p><w:r><w:t>Archive size limit test fixture.</w:t></w:r></w:p></w:body>"
        b"</w:document>"
    )

    raw_files = [
        ("[Content_Types].xml", content_types),
        ("_rels/.rels", rels),
        ("docProps/core.xml", core_xml),
        ("word/document.xml", document_xml),
    ]
    packed: list[tuple[str, bytes, int, int, int]] = []
    for name, raw in raw_files:
        comp = zlib.compress(raw, level=9)[2:-4]
        crc = zlib.crc32(raw) & 0xFFFFFFFF
        packed.append((name, comp, crc, len(raw), len(raw)))
    return packed


def _build_bomb_docx(*, actual_size: int, declared_size: int) -> bytes:
    entries = _minimal_docx_entries()
    comp, crc = _deflate_zeros(actual_size)
    entries.append(("word/bomb/zeros.bin", comp, crc, actual_size, declared_size))
    return _pack_zip_entries(entries)


def generate() -> None:
    """Write all docx/ fixtures to tests/fixtures/docx/."""
    write_bytes(FIXTURES_ROOT / "docx" / "headings-table.docx", _build_headings_table_docx())
    one_gib = 1024 * 1024 * 1024
    write_bytes(
        FIXTURES_ROOT / "docx" / "bomb-honest.docx",
        _build_bomb_docx(actual_size=one_gib, declared_size=one_gib),
    )
    six_hundred_forty_mib = 640 * 1024 * 1024
    write_bytes(
        FIXTURES_ROOT / "docx" / "bomb-lying.docx",
        _build_bomb_docx(actual_size=six_hundred_forty_mib, declared_size=1024),
    )


def main() -> int:
    generate()
    return 0


if __name__ == "__main__":
    sys.exit(main())
