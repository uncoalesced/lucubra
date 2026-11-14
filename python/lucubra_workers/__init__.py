# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""Extractor workers for Lucubra.

A worker is a long-lived subprocess that reads one JSON request per line on
standard input and writes one JSON response per line on standard output.
"""

from __future__ import annotations

import json
from importlib import metadata
from typing import IO, Any

try:
    __version__ = metadata.version("lucubra")
except metadata.PackageNotFoundError:
    __version__ = "0.0.0+unknown"


def _error(request_id: Any, code: str, detail: str) -> dict[str, Any]:
    return {"id": request_id, "ok": False, "error": {"code": code, "detail": detail}}


def handle(request: Any) -> dict[str, Any]:
    """Return the response for one decoded request."""
    if not isinstance(request, dict):
        return _error(None, "bad_request", "request must be a JSON object")
    request_id = request.get("id")
    op = request.get("op")
    if op == "ping":
        return {"id": request_id, "ok": True, "worker": __version__}
    if op == "extract":
        return _extract(request_id, request)
    return _error(request_id, "unknown_op", f"unsupported op: {op!r}")


def _extract(request_id: Any, request: dict[str, Any]) -> dict[str, Any]:
    from lucubra_workers.errors import ExtractError
    from lucubra_workers.office import extract_docx, extract_pptx
    from lucubra_workers.pdf import extract_pdf

    path = request.get("path")
    opts = request.get("opts") or {}
    fmt = opts.get("format") if isinstance(opts, dict) else None
    if not isinstance(path, str) or not path:
        return _error(request_id, "bad_request", "extract needs a path")
    extractors = {"pdf": extract_pdf, "docx": extract_docx, "pptx": extract_pptx}
    extractor = extractors.get(fmt)
    if extractor is None:
        return _error(request_id, "unsupported", f"no worker extractor for {fmt!r}")
    try:
        result = extractor(path)
    except ExtractError as exc:
        return _error(request_id, exc.code, exc.detail)
    except MemoryError:
        return _error(request_id, "limit_exceeded", "out of memory while extracting")
    except Exception as exc:  # one bad file must not stop the worker
        return _error(request_id, "corrupt", f"{type(exc).__name__}: {exc}")
    return {"id": request_id, "ok": True, **result}


def serve(stdin: IO[str], stdout: IO[str]) -> None:
    """Answer requests line by line until standard input closes."""
    for line in stdin:
        line = line.strip()
        if not line:
            continue
        try:
            response = handle(json.loads(line))
        except json.JSONDecodeError as exc:
            response = _error(None, "bad_request", f"invalid JSON: {exc.msg}")
        stdout.write(json.dumps(response, ensure_ascii=False) + "\n")
        stdout.flush()
