# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""PDF extraction and page rendering with pypdfium2 and pdfplumber (SPEC 10.2, ADR 0011).

One chunk per page with locator ``p:<n>``. A page with fewer than
``VISION_MIN_CHARS`` non-whitespace characters is listed in ``needs_vision``.
Outline titles at the top two levels are returned as ``units_hint``.
Ruled tables detected by ``pdfplumber`` are appended as Markdown tables.
"""

from __future__ import annotations

import os
from importlib import metadata
from typing import Any

from lucubra_workers.errors import ExtractError

__all__ = [
    "ExtractError",
    "VISION_MIN_CHARS",
    "extract_pdf",
    "render_pdf_pages",
    "visible_chars",
]

VISION_MIN_CHARS = 40
MAX_FILE_BYTES = 1 << 30
MAX_TEXT_BYTES = 64 << 20
DEFAULT_LONG_EDGE = 1600


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


def _table_markdown(rows: list[list[str | None]]) -> str:
    cleaned = [
        [
            ("" if cell is None else cell).replace("\n", " ").replace("|", "\\|").strip()
            for cell in r
        ]
        for r in rows
        if r and any(cell is not None and str(cell).strip() for cell in r)
    ]
    if not cleaned:
        return ""
    width = max(len(r) for r in cleaned)
    lines = []
    for i, row in enumerate(cleaned):
        padded = row + [""] * (width - len(row))
        lines.append("| " + " | ".join(padded) + " |")
        if i == 0:
            lines.append("|" + " --- |" * width)
    return "\n".join(lines)


def _extract_tables_by_page(path: str) -> dict[int, str]:
    try:
        import pdfplumber
    except ImportError:
        return {}

    tables_by_page: dict[int, str] = {}
    try:
        with pdfplumber.open(path) as plumber_pdf:
            for index, page in enumerate(plumber_pdf.pages, start=1):
                raw_tables = page.extract_tables() or []
                rendered = [md for tbl in raw_tables if (md := _table_markdown(tbl))]
                if rendered:
                    tables_by_page[index] = "\n\n".join(rendered)
    except Exception:
        return {}
    return tables_by_page


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

    tables_by_page = _extract_tables_by_page(path)
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
                    text = _clean(textpage.get_text_range())
                finally:
                    textpage.close()
            finally:
                page.close()
            number = index + 1
            if visible_chars(text) < VISION_MIN_CHARS:
                needs_vision.append(number)
            table_md = tables_by_page.get(number, "")
            if table_md:
                text = f"{text}\n\n[tables]\n{table_md}" if text else f"[tables]\n{table_md}"
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
        raw_meta = pdf.get_metadata_dict(skip_empty=True)
        meta: dict[str, Any] = {"pages": pages}
        if title := (raw_meta.get("Title") or "").strip():
            meta["title"] = title
        if subject := (raw_meta.get("Subject") or "").strip():
            meta["subject"] = subject
        return {
            "kind": "pdf",
            "extractor": _extractor(),
            "chunks": chunks,
            "needs_vision": needs_vision,
            "units_hint": units_hint,
            "truncated": truncated,
            "meta": meta,
        }
    finally:
        pdf.close()


def render_pdf_pages(
    path: str,
    pages: list[int],
    out_dir: str,
    long_edge: int = DEFAULT_LONG_EDGE,
    force: bool = False,
) -> dict[str, Any]:
    """Render requested 1-based PDF pages to PNG files bounded by ``long_edge``."""
    import pypdfium2 as pdfium
    from PIL import Image

    try:
        pdf = pdfium.PdfDocument(path)
    except pdfium.PdfiumError as exc:
        message = str(exc)
        if "password" in message.lower():
            raise ExtractError("encrypted", "the PDF is password-protected") from exc
        raise ExtractError("corrupt", message) from exc

    os.makedirs(out_dir, exist_ok=True)
    rendered: list[dict[str, Any]] = []
    skipped: list[int] = []
    try:
        total_pages = len(pdf)
        for page_num in pages:
            if not isinstance(page_num, int) or page_num < 1 or page_num > total_pages:
                continue
            target = os.path.join(out_dir, f"p{page_num}.png")
            if not force and os.path.isfile(target) and os.path.getsize(target) > 0:
                skipped.append(page_num)
                continue
            page = pdf[page_num - 1]
            try:
                width_pt, height_pt = page.get_size()
                scale = min(long_edge / max(width_pt, height_pt, 1.0), 4.0)
                bitmap = page.render(scale=scale)
                try:
                    image = bitmap.to_pil()
                finally:
                    bitmap.close()
            finally:
                page.close()
            if max(image.size) > long_edge:
                image.thumbnail((long_edge, long_edge), Image.Resampling.LANCZOS)
            image.save(target, format="PNG")
            rendered.append({"page": page_num, "path": target})
    finally:
        pdf.close()
    return {"kind": "pdf", "rendered": rendered, "skipped": skipped}
