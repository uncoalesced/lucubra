# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
# /// script
# requires-python = ">=3.10"
# dependencies = [
#   "pydicom>=3,<4",
# ]
# ///
"""Generate medical imaging fixtures under tests/fixtures/med/."""

from __future__ import annotations

import io
import math
import sys

from _common import FIXTURES_ROOT, write_bytes


def manifest_entries() -> list[dict[str, object]]:
    """Return ADR 0011 manifest descriptors for all med/ fixtures."""
    return [
        {
            "path": "med/phantom.dcm",
            "format": "dicom",
            "stub": False,
            "generator": "scripts/fixtures/gen_med.py",
            "scenarios": ["S04"],
            "expect": {
                "outcome": "extracted",
                "needs_vision": [1],
                "contains": ["Anonymous^Test", "Synthetic Phantom"],
            },
            "notes": "Synthetic 64x64 8-bit grayscale DICOM phantom, no real patient data",
        }
    ]


def _build_phantom_dcm() -> bytes:
    from pydicom.dataset import FileDataset, FileMetaDataset
    from pydicom.uid import ExplicitVRLittleEndian, SecondaryCaptureImageStorage

    sop_uid = "1.2.826.0.1.3680043.8.498.100001"
    file_meta = FileMetaDataset()
    file_meta.MediaStorageSOPClassUID = SecondaryCaptureImageStorage
    file_meta.MediaStorageSOPInstanceUID = sop_uid
    file_meta.TransferSyntaxUID = ExplicitVRLittleEndian
    file_meta.ImplementationClassUID = "1.2.826.0.1.3680043.8.498.990001"
    file_meta.ImplementationVersionName = "LUCUBRA_FIX"

    ds = FileDataset(None, {}, file_meta=file_meta, preamble=b"\x00" * 128)
    ds.PatientName = "Anonymous^Test"
    ds.PatientID = "PHANTOM01"
    ds.StudyDescription = "Synthetic Phantom"
    ds.SeriesDescription = "Circle Gradient 64x64"
    ds.StudyInstanceUID = "1.2.826.0.1.3680043.8.498.100002"
    ds.SeriesInstanceUID = "1.2.826.0.1.3680043.8.498.100003"
    ds.SOPInstanceUID = sop_uid
    ds.SOPClassUID = SecondaryCaptureImageStorage
    ds.StudyDate = "20260101"
    ds.StudyTime = "000000"
    ds.Modality = "OT"
    ds.Rows = 64
    ds.Columns = 64
    ds.SamplesPerPixel = 1
    ds.PhotometricInterpretation = "MONOCHROME2"
    ds.BitsAllocated = 8
    ds.BitsStored = 8
    ds.HighBit = 7
    ds.PixelRepresentation = 0

    pixels = bytearray(64 * 64)
    for row in range(64):
        for col in range(64):
            dist = math.hypot(row - 31.5, col - 31.5)
            pixels[row * 64 + col] = 220 if dist < 18.0 else (120 if dist < 26.0 else 20)
    ds.PixelData = bytes(pixels)

    buf = io.BytesIO()
    ds.save_as(buf, enforce_file_format=True)
    return buf.getvalue()


def generate() -> None:
    """Write all med/ fixtures to tests/fixtures/med/."""
    write_bytes(FIXTURES_ROOT / "med" / "phantom.dcm", _build_phantom_dcm())


def main() -> int:
    generate()
    return 0


if __name__ == "__main__":
    sys.exit(main())
