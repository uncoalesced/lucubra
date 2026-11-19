# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
"""Shared constants and deterministic packaging helpers for fixture generators."""

from __future__ import annotations

import datetime
import io
import zipfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
FIXTURES_ROOT = REPO_ROOT / "tests" / "fixtures"

FIXED_ZIP_TIME = (1980, 1, 1, 0, 0, 0)
FIXED_ISO_DATE = "2026-01-01T00:00:00Z"
FIXED_PDF_DATE = "D:20260101000000Z"
FIXED_DT = datetime.datetime(2026, 1, 1, 0, 0, 0, tzinfo=datetime.timezone.utc)
FIXED_DT_NAIVE = datetime.datetime(2026, 1, 1, 0, 0, 0)

MALAYALAM_TEXT = (
    "കോശങ്ങളിലെ പ്രകാശസംശ്ലേഷണം സൂര്യപ്രകാശത്തിൽ നടക്കുന്നു.\n"
    "അവൻ പ്രകാശപഠനം നടത്തുന്നു; അവ\u0d28\u0d4d\u200d ഈ കുറിപ്പുകൾ വായിക്കുന്നു.\n"
    "താപനിലയും ജലലഭ്യതയും ചെടികളുടെ വളർച്ചയെ സ്വാധീനിക്കുന്നു.\n"
)


def write_bytes(path: Path, data: bytes) -> None:
    """Create parent directories and write raw bytes cleanly."""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)


def write_text_lf(path: Path, text: str, *, bom: bool = False) -> None:
    """Write UTF-8 text with LF line endings (and optional BOM)."""
    normalised = text.replace("\r\n", "\n").replace("\r", "\n")
    payload = normalised.encode("utf-8-sig" if bom else "utf-8")
    write_bytes(path, payload)


def write_text_crlf_bom(path: Path, text: str) -> None:
    """Write UTF-8 text with a leading BOM and CRLF line endings."""
    normalised = text.replace("\r\n", "\n").replace("\r", "\n").replace("\n", "\r\n")
    write_bytes(path, normalised.encode("utf-8-sig"))


def repack_zip_deterministic(
    raw_bytes: bytes,
    *,
    stored_first: str | None = None,
) -> bytes:
    """Repackage a zip archive with fixed entry order, timestamps, and permissions."""
    out = io.BytesIO()
    with zipfile.ZipFile(io.BytesIO(raw_bytes), "r") as zin:
        names = sorted(zin.namelist())
        if stored_first is not None and stored_first in names:
            names.remove(stored_first)
            ordered = [stored_first, *names]
        else:
            ordered = names
        with zipfile.ZipFile(out, "w", compression=zipfile.ZIP_DEFLATED) as zout:
            for name in ordered:
                info = zipfile.ZipInfo(name, date_time=FIXED_ZIP_TIME)
                info.external_attr = 0o644 << 16
                if name == stored_first:
                    info.compress_type = zipfile.ZIP_STORED
                else:
                    info.compress_type = zipfile.ZIP_DEFLATED
                zout.writestr(info, zin.read(name))
    return out.getvalue()
