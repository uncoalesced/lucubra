# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""Tests for DOCX and PPTX extraction and the zip guard (GATES M2-4, M2-11)."""

from __future__ import annotations

import io
import struct
import zipfile

import pytest
from lucubra_workers import handle, zipguard
from lucubra_workers.errors import ExtractError
from lucubra_workers.office import extract_docx, extract_pptx


def make_docx(path) -> str:
    import docx

    document = docx.Document()
    document.add_paragraph("Preface text before any heading.")
    document.add_heading("Unit 1 Cells", level=1)
    document.add_paragraph("Chloroplasts hold chlorophyll.")
    document.add_heading("Membranes", level=2)
    table = document.add_table(rows=2, cols=2)
    table.cell(0, 0).text = "Part"
    table.cell(0, 1).text = "Role"
    table.cell(1, 0).text = "Nucleus"
    table.cell(1, 1).text = "Holds DNA"
    document.add_heading("Detail", level=3)
    document.add_paragraph("Lipid bilayer.")
    document.save(str(path))
    return str(path)


def make_pptx(path) -> str:
    from PIL import Image
    from pptx import Presentation
    from pptx.util import Inches

    prs = Presentation()
    first = prs.slides.add_slide(prs.slide_layouts[1])
    first.shapes.title.text = "Unit 2 Energy"
    first.placeholders[1].text = "ATP stores energy."
    second = prs.slides.add_slide(prs.slide_layouts[1])
    second.shapes.title.text = "Respiration"
    second.notes_slide.notes_text_frame.text = "Mention mitochondria here."
    third = prs.slides.add_slide(prs.slide_layouts[6])  # blank layout
    image = io.BytesIO()
    Image.new("RGB", (8, 8), "white").save(image, format="PNG")
    image.seek(0)
    third.shapes.add_picture(image, Inches(1), Inches(1))
    prs.save(str(path))
    return str(path)


def test_docx_sections_tables_and_hints(tmp_path):
    result = extract_docx(make_docx(tmp_path / "a.docx"))
    refs = [c["ref"] for c in result["chunks"]]
    assert refs == ["h:0", "h:1", "h:2", "h:3"]
    texts = [c["text"] for c in result["chunks"]]
    assert texts[0] == "Preface text before any heading."
    assert texts[1].startswith("Unit 1 Cells")
    assert "| Nucleus | Holds DNA |" in texts[2]
    assert result["units_hint"] == ["Unit 1 Cells", "Membranes"]
    assert result["extractor"].startswith("python-docx@")


def test_pptx_slides_notes_and_vision(tmp_path):
    result = extract_pptx(make_pptx(tmp_path / "a.pptx"))
    assert result["meta"]["slides"] == 3
    assert [c["ref"] for c in result["chunks"]] == ["s:1", "s:2"]
    assert "ATP stores energy." in result["chunks"][0]["text"]
    assert result["chunks"][1]["text"].endswith("[notes]\nMention mitochondria here.")
    assert result["needs_vision"] == [3]


def _append(path: str, name: str, data: bytes, method: int) -> None:
    with zipfile.ZipFile(path, "a", compression=method) as archive:
        archive.writestr(name, data)


def test_ratio_bomb_is_limit_exceeded(tmp_path):
    path = make_docx(tmp_path / "bomb.docx")
    _append(path, "word/media/pad.bin", bytes(4 << 20), zipfile.ZIP_DEFLATED)
    with pytest.raises(ExtractError) as err:
        extract_docx(path)
    assert err.value.code == "limit_exceeded"
    assert "expands" in err.value.detail


def test_declared_size_limit(tmp_path, monkeypatch):
    monkeypatch.setattr(zipguard, "MAX_DECLARED_BYTES", 1 << 20)
    path = make_docx(tmp_path / "big.docx")
    _append(path, "word/media/pad.bin", bytes(2 << 20), zipfile.ZIP_STORED)
    with pytest.raises(ExtractError) as err:
        extract_docx(path)
    assert err.value.code == "limit_exceeded"
    assert "declares" in err.value.detail


def test_damaged_container_is_corrupt_through_handle(tmp_path):
    path = make_docx(tmp_path / "bad.docx")
    data = bytearray(open(path, "rb").read())
    with zipfile.ZipFile(path) as archive:
        info = archive.getinfo("word/document.xml")
    # Flip bytes inside the compressed document part.
    start = info.header_offset + 30 + len(info.filename) + len(info.extra) + 4
    for i in range(start, start + 16):
        data[i] ^= 0xFF
    open(path, "wb").write(bytes(data))
    reply = handle({"id": "r1", "op": "extract", "path": path, "opts": {"format": "docx"}})
    assert reply["ok"] is False
    assert reply["error"]["code"] == "corrupt"
    not_zip = tmp_path / "plain.pptx"
    not_zip.write_bytes(b"not a zip at all")
    reply = handle({"id": "r2", "op": "extract", "path": str(not_zip), "opts": {"format": "pptx"}})
    assert reply["error"]["code"] == "corrupt"


def test_lying_header_zip_bomb_is_limit_exceeded(tmp_path):
    path = make_docx(tmp_path / "bomb-lying.docx")
    payload = bytes(4 << 20)
    _append(path, "word/media/lying.bin", payload, zipfile.ZIP_DEFLATED)
    data = bytearray(open(path, "rb").read())
    target = struct.pack("<I", len(payload))
    lied = struct.pack("<I", 1024)
    replaced = data.replace(target, lied)
    assert replaced != data
    open(path, "wb").write(replaced)
    with pytest.raises(ExtractError) as err:
        extract_docx(path)
    assert err.value.code == "limit_exceeded"
