# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""Zip container limits checked before parsing (ADR 0011 section 4).

The declared sizes in the central directory are checked first. A file whose
headers under-declare sizes is caught later: Python's zip reader stops at the
declared size, so the CRC check fails and the file is reported as corrupt.
"""

from __future__ import annotations

import zipfile

from lucubra_workers.errors import ExtractError

MAX_DECLARED_BYTES = 512 << 20
MAX_ENTRIES = 20_000
RATIO_FLOOR_BYTES = 1 << 20
MAX_RATIO = 200


def check_zip(path: str) -> None:
    """Raise ExtractError when a zip container breaks a limit or is unreadable."""
    try:
        with zipfile.ZipFile(path) as archive:
            entries = archive.infolist()
    except (zipfile.BadZipFile, OSError, ValueError) as exc:
        raise ExtractError("corrupt", f"not a readable zip container: {exc}") from exc
    if len(entries) > MAX_ENTRIES:
        raise ExtractError("limit_exceeded", f"{len(entries)} entries is over the limit")
    declared = sum(e.file_size for e in entries)
    if declared > MAX_DECLARED_BYTES:
        raise ExtractError(
            "limit_exceeded", f"declares {declared} uncompressed bytes, over the limit"
        )
    for entry in entries:
        if entry.file_size > RATIO_FLOOR_BYTES:
            ratio = entry.file_size / max(entry.compress_size, 1)
            if ratio > MAX_RATIO:
                raise ExtractError(
                    "limit_exceeded",
                    f"entry {entry.filename} expands {ratio:.0f} times, over the limit",
                )
