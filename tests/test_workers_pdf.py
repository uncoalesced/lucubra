# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""Tests for PDF extraction (GATES M2-3, M2-5). PDFs are built by hand here."""

from __future__ import annotations

import pytest
from lucubra_workers import handle
from lucubra_workers.pdf import VISION_MIN_CHARS, ExtractError, extract_pdf


def _escape(text: str) -> str:
    return text.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")


def make_pdf(pages: list[str], outline: list[tuple[str, int]] | None = None) -> bytes:
    """A minimal PDF: one Helvetica text line per page, optional outline."""
    outline = outline or []
    first_page = 4
    page_ids = [first_page + 2 * i for i in range(len(pages))]
    outline_root = first_page + 2 * len(pages)
    objects: dict[int, bytes] = {}
    catalog = b"<< /Type /Catalog /Pages 2 0 R"
    if outline:
        catalog += f" /Outlines {outline_root} 0 R".encode()
    objects[1] = catalog + b" >>"
    kids = " ".join(f"{p} 0 R" for p in page_ids)
    objects[2] = f"<< /Type /Pages /Kids [{kids}] /Count {len(pages)} >>".encode()
    objects[3] = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"
    for page_id, text in zip(page_ids, pages, strict=True):
        stream = f"BT /F1 12 Tf 72 720 Td ({_escape(text)}) Tj ET".encode() if text else b""
        objects[page_id] = (
            f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            f"/Resources << /Font << /F1 3 0 R >> >> /Contents {page_id + 1} 0 R >>"
        ).encode()
        objects[page_id + 1] = (
            f"<< /Length {len(stream)} >>\nstream\n".encode() + stream + b"\nendstream"
        )
    if outline:
        items = [outline_root + 1 + i for i in range(len(outline))]
        objects[outline_root] = (
            f"<< /Type /Outlines /First {items[0]} 0 R /Last {items[-1]} 0 R /Count {len(items)} >>"
        ).encode()
        for i, (title, page) in enumerate(outline):
            links = ""
            if i > 0:
                links += f" /Prev {items[i - 1]} 0 R"
            if i < len(items) - 1:
                links += f" /Next {items[i + 1]} 0 R"
            objects[items[i]] = (
                f"<< /Title ({_escape(title)}) /Parent {outline_root} 0 R "
                f"/Dest [{page_ids[page - 1]} 0 R /Fit]{links} >>"
            ).encode()
    out = bytearray(b"%PDF-1.7\n")
    offsets = {}
    for number in sorted(objects):
        offsets[number] = len(out)
        out += f"{number} 0 obj\n".encode() + objects[number] + b"\nendobj\n"
    xref = len(out)
    size = max(objects) + 1
    out += f"xref\n0 {size}\n0000000000 65535 f \n".encode()
    for number in range(1, size):
        out += f"{offsets[number]:010d} 00000 n \n".encode()
    out += f"trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
    return bytes(out)


@pytest.fixture
def pdf_file(tmp_path):
    def write(data: bytes, name: str = "a.pdf") -> str:
        path = tmp_path / name
        path.write_bytes(data)
        return str(path)

    return write


def test_pdf_pages_text_and_outline(pdf_file):
    path = pdf_file(
        make_pdf(
            [
                "Chlorophyll absorbs red and blue light in the chloroplast.",
                "Respiration releases energy from glucose molecules.",
            ],
            outline=[("Unit 1 Cells", 1), ("Unit 2 Energy", 2)],
        )
    )
    result = extract_pdf(path)
    assert result["meta"]["pages"] == 2
    assert [c["ref"] for c in result["chunks"]] == ["p:1", "p:2"]
    assert "Chlorophyll" in result["chunks"][0]["text"]
    assert result["needs_vision"] == []
    assert result["units_hint"] == ["Unit 1 Cells", "Unit 2 Energy"]
    assert result["extractor"].startswith("pypdfium2@")


def test_pdf_vision_threshold_boundary(pdf_file):
    below, at, above = (
        "a" * (VISION_MIN_CHARS - 1),
        "b" * VISION_MIN_CHARS,
        "c" * (VISION_MIN_CHARS + 1),
    )
    result = extract_pdf(pdf_file(make_pdf([below, at, above, ""])))
    assert result["needs_vision"] == [1, 4]
    assert [c["ref"] for c in result["chunks"]] == ["p:1", "p:2", "p:3"]


def test_pdf_corrupt_and_missing(pdf_file, tmp_path):
    good = make_pdf(["Some text that is long enough to be a page of notes."])
    with pytest.raises(ExtractError) as truncated:
        extract_pdf(pdf_file(good[: len(good) // 3], "cut.pdf"))
    assert truncated.value.code == "corrupt"
    with pytest.raises(ExtractError) as missing:
        extract_pdf(str(tmp_path / "absent.pdf"))
    assert missing.value.code == "corrupt"


def test_extract_op_through_handle(pdf_file):
    path = pdf_file(make_pdf(["Osmosis moves water across a membrane by diffusion."]))
    ok = handle({"id": "r1", "op": "extract", "path": path, "opts": {"format": "pdf"}})
    assert ok["id"] == "r1" and ok["ok"] is True
    assert ok["chunks"][0]["ref"] == "p:1"
    failed = handle(
        {"id": "r2", "op": "extract", "path": path + ".nope", "opts": {"format": "pdf"}}
    )
    assert failed["ok"] is False
    assert failed["error"]["code"] == "corrupt"
    unsupported = handle({"id": "r3", "op": "extract", "path": path, "opts": {"format": "xyz"}})
    assert unsupported["error"]["code"] == "unsupported"
    bad = handle({"id": "r4", "op": "extract", "opts": {"format": "pdf"}})
    assert bad["error"]["code"] == "bad_request"
