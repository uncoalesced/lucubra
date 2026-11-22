# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""DICOM extraction and rendering with pydicom (SPEC section 10.2, ADR 0011)."""

from __future__ import annotations

import os
from importlib import metadata
from typing import Any

from lucubra_workers.errors import ExtractError

MAX_FILE_BYTES = 1 << 30

_METADATA_TAGS = (
    "Modality",
    "StudyDescription",
    "SeriesDescription",
    "BodyPartExamined",
    "PatientID",
    "PatientName",
    "StudyDate",
    "Manufacturer",
    "Rows",
    "Columns",
    "NumberOfFrames",
    "PhotometricInterpretation",
)


def _extractor() -> str:
    try:
        return f"pydicom@{metadata.version('pydicom')}"
    except metadata.PackageNotFoundError:
        return "pydicom"


def extract_dicom(path: str) -> dict[str, Any]:
    """Extract DICOM metadata and detect pixel data needing vision."""
    import pydicom
    from pydicom.errors import InvalidDicomError

    try:
        size = os.path.getsize(path)
    except OSError as exc:
        raise ExtractError("corrupt", str(exc)) from exc
    if size > MAX_FILE_BYTES:
        raise ExtractError("limit_exceeded", f"{size} bytes is over the input limit")

    try:
        ds = pydicom.dcmread(path, defer_size="1 KB")
    except (InvalidDicomError, OSError, EOFError, ValueError) as exc:
        raise ExtractError("corrupt", f"unreadable DICOM: {exc}") from exc

    lines = ["Format: DICOM medical image"]
    meta: dict[str, Any] = {}
    for tag in _METADATA_TAGS:
        if tag in ds:
            val = str(ds.get(tag)).strip()
            if val:
                lines.append(f"{tag}: {val}")
                meta[tag] = val

    has_pixels = "PixelData" in ds or (
        int(getattr(ds, "Rows", 0) or 0) > 0 and int(getattr(ds, "Columns", 0) or 0) > 0
    )
    needs_vision = [1] if has_pixels else []
    meta["has_pixel_data"] = has_pixels

    return {
        "kind": "dicom",
        "extractor": _extractor(),
        "chunks": [{"ref": "h:0", "text": "\n".join(lines)}],
        "needs_vision": needs_vision,
        "units_hint": [],
        "truncated": False,
        "meta": meta,
    }


def render_dicom(
    path: str,
    out_dir: str,
    long_edge: int = 1600,
    force: bool = False,
) -> dict[str, Any]:
    """Render DICOM pixel data to a bounded PNG in ``out_dir``."""
    import numpy as np
    import pydicom
    from PIL import Image
    from pydicom.errors import InvalidDicomError

    os.makedirs(out_dir, exist_ok=True)
    target = os.path.join(out_dir, "p1.png")
    if not force and os.path.isfile(target) and os.path.getsize(target) > 0:
        return {"kind": "dicom", "rendered": [], "skipped": [1]}

    try:
        ds = pydicom.dcmread(path)
    except (InvalidDicomError, OSError, EOFError, ValueError) as exc:
        raise ExtractError("corrupt", f"unreadable DICOM: {exc}") from exc

    if "PixelData" not in ds:
        return {"kind": "dicom", "rendered": [], "skipped": []}

    try:
        arr = ds.pixel_array
    except Exception as exc:
        raise ExtractError("corrupt", f"cannot decode DICOM pixels: {exc}") from exc

    if arr.ndim >= 3 and arr.shape[-1] not in (3, 4):
        arr = arr[0]
    arr = arr.astype(np.float64)
    lo = float(arr.min())
    hi = float(arr.max())
    if hi > lo:
        scaled = ((arr - lo) / (hi - lo) * 255.0).clip(0, 255).astype(np.uint8)
    else:
        scaled = np.zeros(arr.shape, dtype=np.uint8)

    image = Image.fromarray(scaled)
    if max(image.size) > long_edge:
        image.thumbnail((long_edge, long_edge), Image.Resampling.LANCZOS)
    image.save(target, format="PNG")
    return {"kind": "dicom", "rendered": [{"page": 1, "path": target}], "skipped": []}
