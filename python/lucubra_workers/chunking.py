# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""Text chunking shared by the worker extractors.

Mirrors the Rust native extractors: pieces of at most ``CHUNK_CHARS``
characters, split at paragraph breaks, then line breaks, then characters.
"""

from __future__ import annotations

CHUNK_CHARS = 2000


def split_text(text: str, limit: int = CHUNK_CHARS) -> list[str]:
    """Split text into pieces of at most ``limit`` characters."""
    pieces: list[str] = []
    current = ""

    def flush() -> None:
        nonlocal current
        if current.strip():
            pieces.append(current)
        current = ""

    for para in text.split("\n\n"):
        para = para.strip("\n")
        if not para.strip():
            continue
        if len(para) <= limit:
            if len(current) + len(para) + 2 > limit:
                flush()
            current = f"{current}\n\n{para}" if current else para
            continue
        flush()
        for line in para.split("\n"):
            while len(line) > limit:
                flush()
                pieces.append(line[:limit])
                line = line[limit:]
            if len(current) + len(line) + 1 > limit:
                flush()
            current = f"{current}\n{line}" if current else line
        flush()
    flush()
    return pieces


def section_chunks(sections: list[str], prefix: str = "h") -> list[dict[str, str]]:
    """Chunks ``<prefix>:<n>`` and ``<prefix>:<n>.<part>`` for each section."""
    chunks = []
    for number, body in enumerate(sections):
        for part, piece in enumerate(split_text(body)):
            ref = f"{prefix}:{number}" if part == 0 else f"{prefix}:{number}.{part}"
            chunks.append({"ref": ref, "text": piece})
    return chunks
