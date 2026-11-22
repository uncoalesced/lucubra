# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""DOCX and PPTX extraction (SPEC section 10.2, ADR 0011).

DOCX: sections split at headings, locators ``h:<n>``, tables as Markdown,
Heading 1, Heading 2 and Title text as ``units_hint``.
PPTX: one section per slide, locators ``s:<n>``, speaker notes appended, and a
slide with a picture but no text listed in ``needs_vision``.
"""

from __future__ import annotations

import os
import zipfile
from importlib import metadata
from typing import Any

from lucubra_workers.chunking import section_chunks, split_text
from lucubra_workers.errors import ExtractError
from lucubra_workers.zipguard import check_zip

MAX_FILE_BYTES = 1 << 30


def _version(dist: str) -> str:
    try:
        return f"{dist}@{metadata.version(dist)}"
    except metadata.PackageNotFoundError:
        return dist


def _precheck(path: str) -> None:
    try:
        size = os.path.getsize(path)
    except OSError as exc:
        raise ExtractError("corrupt", str(exc)) from exc
    if size > MAX_FILE_BYTES:
        raise ExtractError("limit_exceeded", f"{size} bytes is over the input limit")
    check_zip(path)


def _table_markdown(rows: list[list[str]]) -> str:
    if not rows:
        return ""
    width = max(len(r) for r in rows)
    lines = []
    for i, row in enumerate(rows):
        cells = [c.replace("\n", " ").replace("|", "\\|").strip() for c in row]
        cells += [""] * (width - len(cells))
        lines.append("| " + " | ".join(cells) + " |")
        if i == 0:
            lines.append("|" + " --- |" * width)
    return "\n".join(lines)


def _heading_level(style_name: str) -> int | None:
    if style_name == "Title":
        return 0
    if style_name.startswith("Heading"):
        digits = style_name[len("Heading") :].strip()
        return int(digits) if digits.isdigit() else 1
    return None


def extract_docx(path: str) -> dict[str, Any]:
    """Extract a DOCX. Raises ExtractError on failure."""
    _precheck(path)
    import docx
    from docx.table import Table
    from docx.text.paragraph import Paragraph

    try:
        document = docx.Document(path)
        sections = [""]
        hints: list[str] = []
        for block in document.iter_inner_content():
            if isinstance(block, Paragraph):
                text = block.text.strip()
                style = block.style.name if block.style is not None else ""
                level = _heading_level(style or "")
                if level is not None and text:
                    sections.append(text + "\n\n")
                    if level <= 2:
                        hints.append(text)
                elif text:
                    sections[-1] += text + "\n\n"
            elif isinstance(block, Table):
                rows = [[cell.text for cell in row.cells] for row in block.rows]
                sections[-1] += _table_markdown(rows) + "\n\n"
    except zipfile.BadZipFile as exc:
        raise ExtractError("corrupt", f"damaged container: {exc}") from exc
    return {
        "kind": "docx",
        "extractor": _version("python-docx"),
        "chunks": section_chunks(sections),
        "needs_vision": [],
        "units_hint": hints,
        "truncated": False,
        "meta": {"sections": len(sections)},
    }


def _shapes(shapes):
    for shape in shapes:
        if getattr(shape, "shapes", None) is not None:  # group shape
            yield from _shapes(shape.shapes)
        else:
            yield shape


def _is_picture(shape) -> bool:
    from pptx.enum.shapes import MSO_SHAPE_TYPE

    if shape.shape_type == MSO_SHAPE_TYPE.PICTURE:
        return True
    try:
        return shape.image is not None
    except (AttributeError, ValueError, KeyError):
        return False


def extract_pptx(path: str) -> dict[str, Any]:
    """Extract a PPTX. Raises ExtractError on failure."""
    _precheck(path)
    from pptx import Presentation

    try:
        presentation = Presentation(path)
        chunks: list[dict[str, str]] = []
        needs_vision: list[int] = []
        count = 0
        for number, slide in enumerate(presentation.slides, start=1):
            count = number
            parts: list[str] = []
            picture = False
            for shape in _shapes(slide.shapes):
                if _is_picture(shape):
                    picture = True
                if getattr(shape, "has_text_frame", False) and shape.text_frame.text.strip():
                    parts.append(shape.text_frame.text.strip())
                if getattr(shape, "has_table", False):
                    rows = [[c.text for c in row.cells] for row in shape.table.rows]
                    parts.append(_table_markdown(rows))
            body = "\n\n".join(parts)
            if slide.has_notes_slide:
                notes = slide.notes_slide.notes_text_frame.text.strip()
                if notes:
                    body = f"{body}\n\n[notes]\n{notes}" if body else f"[notes]\n{notes}"
            if not parts and picture:
                needs_vision.append(number)
            for part, piece in enumerate(split_text(body)):
                ref = f"s:{number}" if part == 0 else f"s:{number}.{part}"
                chunks.append({"ref": ref, "text": piece})
    except zipfile.BadZipFile as exc:
        raise ExtractError("corrupt", f"damaged container: {exc}") from exc
    return {
        "kind": "pptx",
        "extractor": _version("python-pptx"),
        "chunks": chunks,
        "needs_vision": needs_vision,
        "units_hint": [],
        "truncated": False,
        "meta": {"slides": count},
    }
