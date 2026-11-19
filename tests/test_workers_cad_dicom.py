# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""Tests for DXF and DICOM extraction and DICOM rendering (GATES M2-3, M2-11)."""

from __future__ import annotations

import datetime

import ezdxf
import numpy as np
import pytest
from lucubra_workers import handle
from lucubra_workers.cad import extract_dxf
from lucubra_workers.dicom import extract_dicom, render_dicom
from lucubra_workers.errors import ExtractError
from PIL import Image
from pydicom.dataset import FileDataset, FileMetaDataset
from pydicom.uid import ExplicitVRLittleEndian, SecondaryCaptureImageStorage, generate_uid


def make_dxf(path) -> str:
    doc = ezdxf.new("R2010")
    doc.layers.add("Walls")
    doc.layers.add("Dimensions")
    block = doc.blocks.new("Door")
    block.add_line((0, 0), (1, 0))
    msp = doc.modelspace()
    msp.add_text("North Elevation", dxfattribs={"layer": "Walls", "height": 2.5})
    msp.add_mtext("Reinforced concrete beam", dxfattribs={"layer": "Walls"})
    dim = msp.add_linear_dim(base=(0, 10), p1=(0, 0), p2=(5, 0), dxfattribs={"layer": "Dimensions"})
    dim.render()
    msp.add_blockref("Door", (2, 0), dxfattribs={"layer": "Walls"})
    doc.saveas(str(path))
    return str(path)


def make_dicom(path) -> str:
    meta = FileMetaDataset()
    meta.MediaStorageSOPClassUID = SecondaryCaptureImageStorage
    meta.MediaStorageSOPInstanceUID = generate_uid()
    meta.TransferSyntaxUID = ExplicitVRLittleEndian

    ds = FileDataset(str(path), {}, file_meta=meta, preamble=b"\0" * 128)
    ds.is_little_endian = True
    ds.is_implicit_VR = False
    ds.PatientName = "Anonymous^Test"
    ds.PatientID = "PHANTOM-01"
    ds.Modality = "CT"
    ds.StudyDescription = "Chest Phantom"
    ds.StudyDate = datetime.date(2026, 1, 1).strftime("%Y%m%d")
    ds.Rows = 64
    ds.Columns = 64
    ds.SamplesPerPixel = 1
    ds.PhotometricInterpretation = "MONOCHROME2"
    ds.BitsAllocated = 16
    ds.BitsStored = 16
    ds.HighBit = 15
    ds.PixelRepresentation = 0
    pixels = np.arange(64 * 64, dtype=np.uint16).reshape((64, 64))
    ds.PixelData = pixels.tobytes()
    ds.save_as(str(path), enforce_file_format=True)
    return str(path)


def test_dxf_layers_blocks_and_text(tmp_path):
    path = make_dxf(tmp_path / "drawing.dxf")
    result = extract_dxf(path)
    assert result["kind"] == "dxf"
    assert result["extractor"].startswith("ezdxf@")
    assert "Walls" in result["meta"]["layers"]
    assert "Dimensions" in result["meta"]["layers"]
    assert "Door" in result["meta"]["blocks"]
    by_ref = {c["ref"]: c["text"] for c in result["chunks"]}
    assert "North Elevation" in by_ref["l:Walls"]
    assert "Reinforced concrete beam" in by_ref["l:Walls"]
    assert "[block] Door" in by_ref["l:Walls"]


def test_dicom_metadata_vision_and_render(tmp_path):
    path = make_dicom(tmp_path / "phantom.dcm")
    result = extract_dicom(path)
    assert result["kind"] == "dicom"
    assert result["needs_vision"] == [1]
    text = result["chunks"][0]["text"]
    assert "Modality: CT" in text
    assert "Chest Phantom" in text

    out_dir = tmp_path / "renders"
    rendered = render_dicom(path, str(out_dir), long_edge=1600)
    assert len(rendered["rendered"]) == 1
    with Image.open(out_dir / "p1.png") as img:
        assert max(img.size) <= 1600
        assert img.size == (64, 64)
    second = render_dicom(path, str(out_dir), long_edge=1600)
    assert second["rendered"] == []
    assert second["skipped"] == [1]


def test_corrupt_dxf_and_dicom_fail_cleanly(tmp_path):
    bad_dxf = tmp_path / "bad.dxf"
    bad_dxf.write_text("not a dxf file", encoding="utf-8")
    with pytest.raises(ExtractError) as err_dxf:
        extract_dxf(str(bad_dxf))
    assert err_dxf.value.code == "corrupt"

    bad_dcm = tmp_path / "bad.dcm"
    bad_dcm.write_bytes(b"not dicom")
    reply = handle({"id": "r1", "op": "extract", "path": str(bad_dcm), "opts": {"format": "dicom"}})
    assert reply["ok"] is False
    assert reply["error"]["code"] == "corrupt"
