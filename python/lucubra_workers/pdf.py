# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""PDF extraction with pypdfium2 (SPEC section 10.2, ADR 0011).

One chunk per page with locator ``p:<n>``. A page with fewer than
``VISION_MIN_CHARS`` non-whitespace characters is listed in ``needs_vision``.
Outline titles at the top two levels are returned as ``units_hint``.
"""

from __future__ import annotations

import os
from importlib import metadata
from typing import Any

VISION_MIN_CHARS = 40
MAX_FILE_BYTES = 1 << 30
MAX_TEXT_BYTES = 64 << 20


class ExtractError(Exception):
    """An extraction failure with an ADR 0011 code."""

    def __init__(self, code: str, detail: str) -> None:
        super().__init__(detail)
        self.code = code
        self.detail = detail


def _extractor() -> str:
    try:
        return f"pypdfium2@{metadata.version('pypdfium2')}"
    except metadata.PackageNotFoundError:
        return "pypdfium2"


def visible_chars(text: str) -> int:
    """Count non-whitespace characters, the ADR 0011 vision rule."""
    return sum(1 for c in text if not c.isspace())


def _clean(text: str) -> str:
    return text.replace("\r\n", "\n").replace("\r", "\n").replace("\x00", "").strip()


def extract_pdf(path: str) -> dict[str, Any]:
    """Extract a PDF. Raises ExtractError on failure."""
    import pypdfium2 as pdfium

    try:
        size = os.path.getsize(path)
    except OSError as exc:
        raise ExtractError("corrupt", str(exc)) from exc
    if size > MAX_FILE_BYTES:
        raise ExtractError("limit_exceeded", f"{size} bytes is over the input limit")
    try:
        pdf = pdfium.PdfDocument(path)
    except pdfium.PdfiumError as exc:
        message = str(exc)
        if "password" in message.lower():
            raise ExtractError("encrypted", "the PDF is password-protected") from exc
        raise ExtractError("corrupt", message) from exc
    try:
        chunks: list[dict[str, str]] = []
        needs_vision: list[int] = []
        total = 0
        truncated = False
        pages = len(pdf)
        for index in range(pages):
            page = pdf[index]
            try:
                textpage = page.get_textpage()
                try:
                    text = _clean(textpage.get_text_bounded())
                finally:
                    textpage.close()
            finally:
                page.close()
            number = index + 1
            if visible_chars(text) < VISION_MIN_CHARS:
                needs_vision.append(number)
            if not text or truncated:
                continue
            total += len(text.encode("utf-8"))
            if total > MAX_TEXT_BYTES:
                truncated = True
                continue
            chunks.append({"ref": f"p:{number}", "text": text})
        units_hint = []
        for bookmark in pdf.get_toc(max_depth=2):
            title = (bookmark.get_title() or "").strip()
            if title:
                units_hint.append(title)
        return {
            "kind": "pdf",
            "extractor": _extractor(),
            "chunks": chunks,
            "needs_vision": needs_vision,
            "units_hint": units_hint,
            "truncated": truncated,
            "meta": {"pages": pages},
        }
    finally:
        pdf.close()
