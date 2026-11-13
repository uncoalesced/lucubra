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
    return _error(request_id, "unknown_op", f"unsupported op: {op!r}")


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
