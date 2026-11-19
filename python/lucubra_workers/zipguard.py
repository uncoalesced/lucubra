# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""Zip container limits checked before and while streaming (ADR 0011 section 4)."""

from __future__ import annotations

import struct
import zipfile
import zlib

from lucubra_workers.errors import ExtractError

MAX_DECLARED_BYTES = 512 << 20
MAX_READ_BYTES = 512 << 20
MAX_ENTRIES = 20_000
RATIO_FLOOR_BYTES = 1 << 20
MAX_RATIO = 200
_CHUNK_SIZE = 64 * 1024


def _zip_error(exc: Exception) -> ExtractError:
    msg = str(exc)
    if "encrypt" in msg.lower() or "password" in msg.lower():
        return ExtractError("encrypted", f"encrypted zip container: {msg}")
    return ExtractError("corrupt", f"damaged zip container: {msg}")


def _stream_deflated(raw_file, entry: zipfile.ZipInfo, total_read: int) -> int:
    """Decompress a deflated entry from its local header without trusting declared sizes."""
    raw_file.seek(entry.header_offset)
    local_hdr = raw_file.read(30)
    if len(local_hdr) < 30 or local_hdr[:4] != b"PK\x03\x04":
        raise ExtractError("corrupt", f"invalid local header for {entry.filename}")
    fname_len, extra_len = struct.unpack_from("<HH", local_hdr, 26)
    raw_file.seek(entry.header_offset + 30 + fname_len + extra_len)

    decomp = zlib.decompressobj(-zlib.MAX_WBITS)
    compressed = max(entry.compress_size, 1)
    remaining = entry.compress_size
    entry_read = 0
    crc = 0

    while not decomp.eof:
        if decomp.unconsumed_tail:
            in_buf = b""
            out = decomp.decompress(decomp.unconsumed_tail, _CHUNK_SIZE)
        else:
            to_read = _CHUNK_SIZE if remaining == 0 else min(_CHUNK_SIZE, remaining)
            in_buf = raw_file.read(to_read)
            if not in_buf:
                break
            if remaining > 0:
                remaining -= len(in_buf)
            out = decomp.decompress(in_buf, _CHUNK_SIZE)

        if out:
            entry_read += len(out)
            total_read += len(out)
            crc = zlib.crc32(out, crc)
            if total_read > MAX_READ_BYTES:
                raise ExtractError("limit_exceeded", f"inflated bytes exceed {MAX_READ_BYTES}")
            if entry_read > RATIO_FLOOR_BYTES and entry_read / compressed > MAX_RATIO:
                raise ExtractError(
                    "limit_exceeded",
                    f"entry {entry.filename} expands {entry_read / compressed:.0f} times",
                )
        elif not in_buf and not decomp.unconsumed_tail:
            break

    tail = decomp.flush(_CHUNK_SIZE)
    if tail:
        entry_read += len(tail)
        total_read += len(tail)
        crc = zlib.crc32(tail, crc)
        if total_read > MAX_READ_BYTES:
            raise ExtractError("limit_exceeded", f"inflated bytes exceed {MAX_READ_BYTES}")
        if entry_read > RATIO_FLOOR_BYTES and entry_read / compressed > MAX_RATIO:
            raise ExtractError(
                "limit_exceeded",
                f"entry {entry.filename} expands {entry_read / compressed:.0f} times",
            )

    if not decomp.eof:
        raise ExtractError("corrupt", f"truncated deflate stream in {entry.filename}")
    if (crc & 0xFFFFFFFF) != entry.CRC:
        raise ExtractError("corrupt", f"bad CRC-32 for {entry.filename}")
    return total_read


def check_zip(path: str) -> None:
    """Raise ExtractError when a zip container breaks a limit or is unreadable."""
    try:
        with zipfile.ZipFile(path) as archive:
            entries = archive.infolist()
            if len(entries) > MAX_ENTRIES:
                raise ExtractError("limit_exceeded", f"{len(entries)} entries is over the limit")
            declared = sum(e.file_size for e in entries)
            if declared > MAX_DECLARED_BYTES:
                raise ExtractError(
                    "limit_exceeded",
                    f"declares {declared} uncompressed bytes, over the limit",
                )
            for entry in entries:
                if entry.flag_bits & 0x1:
                    raise ExtractError("encrypted", "archive contains an encrypted entry")
                if entry.file_size > RATIO_FLOOR_BYTES:
                    ratio = entry.file_size / max(entry.compress_size, 1)
                    if ratio > MAX_RATIO:
                        raise ExtractError(
                            "limit_exceeded",
                            f"entry {entry.filename} expands {ratio:.0f} times, over the limit",
                        )

            total_read = 0
            with open(path, "rb") as raw_file:
                for entry in entries:
                    if entry.compress_type == zipfile.ZIP_DEFLATED:
                        total_read = _stream_deflated(raw_file, entry, total_read)
                        continue
                    entry_read = 0
                    compressed = max(entry.compress_size, 1)
                    with archive.open(entry) as stream:
                        while True:
                            chunk = stream.read(_CHUNK_SIZE)
                            if not chunk:
                                break
                            entry_read += len(chunk)
                            total_read += len(chunk)
                            if total_read > MAX_READ_BYTES:
                                raise ExtractError(
                                    "limit_exceeded",
                                    f"inflated bytes exceed {MAX_READ_BYTES}",
                                )
                            if (
                                entry_read > RATIO_FLOOR_BYTES
                                and entry_read / compressed > MAX_RATIO
                            ):
                                raise ExtractError(
                                    "limit_exceeded",
                                    f"entry {entry.filename} expands "
                                    f"{entry_read / compressed:.0f} times",
                                )
    except ExtractError:
        raise
    except (zipfile.BadZipFile, zlib.error, EOFError, RuntimeError, OSError, ValueError) as exc:
        raise _zip_error(exc) from exc
