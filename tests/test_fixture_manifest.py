# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""Verify tests/fixtures/MANIFEST.json integrity, sizes, formats, and watermark hygiene."""

from __future__ import annotations

import hashlib
import io
import json
import unicodedata
import zipfile
import zlib
from pathlib import Path

FIXTURES_ROOT = Path(__file__).resolve().parent / "fixtures"
MANIFEST_PATH = FIXTURES_ROOT / "MANIFEST.json"

VALID_FORMATS = {
    "pdf",
    "docx",
    "pptx",
    "xlsx",
    "xlsm",
    "epub",
    "ods",
    "xls",
    "csv",
    "tsv",
    "md",
    "txt",
    "html",
    "ipynb",
    "code",
    "image",
    "dxf",
    "kicad_sch",
    "kicad_pcb",
    "kicad_pro",
    "spice",
    "ltspice_asc",
    "gerber",
    "dicom",
    "dwg",
    "blend",
    "rvt",
    "skp",
    "unknown",
}

EXCLUDED_FILES = {"MANIFEST.json", "README.md", ".gitattributes", "LICENSE"}
MAX_CORPUS_BYTES = 15 * 1024 * 1024
MAX_FILE_BYTES = 3 * 1024 * 1024
BANNED_PHRASES = ("engineered " + "by", "generated " + "by")


def _load_manifest() -> dict[str, object]:
    assert MANIFEST_PATH.is_file(), "MANIFEST.json is missing"
    data = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
    assert data.get("schema") == 1
    assert isinstance(data.get("files"), list)
    return data


def _check_text_clean(raw: bytes, label: str) -> None:
    lowered_utf8 = raw.decode("utf-8", errors="ignore").lower()
    lowered_latin1 = raw.decode("latin-1", errors="ignore").lower()
    for phrase in BANNED_PHRASES:
        assert phrase not in lowered_utf8, f"{label} contains banned phrase {phrase!r}"
        assert phrase not in lowered_latin1, f"{label} contains banned phrase {phrase!r}"


def test_manifest_paths_hashes_and_coverage() -> None:
    manifest = _load_manifest()
    entries = manifest["files"]
    assert isinstance(entries, list)

    seen_paths: set[str] = set()
    total_bytes = 0
    for entry in entries:
        rel_path = entry["path"]
        assert isinstance(rel_path, str)
        assert rel_path not in seen_paths, f"Duplicate manifest entry: {rel_path}"
        seen_paths.add(rel_path)

        assert entry["format"] in VALID_FORMATS, f"Invalid format {entry['format']!r} on {rel_path}"
        target = FIXTURES_ROOT / rel_path
        assert target.is_file(), f"Missing file listed in manifest: {rel_path}"

        raw = target.read_bytes()
        assert len(raw) == entry["bytes"], f"Byte size mismatch for {rel_path}"
        assert len(raw) <= MAX_FILE_BYTES, f"File exceeds 3 MiB limit: {rel_path}"
        digest = hashlib.sha256(raw).hexdigest()
        assert digest == entry["sha256"], f"SHA-256 mismatch for {rel_path}"
        total_bytes += len(raw)

    assert total_bytes <= MAX_CORPUS_BYTES, f"Corpus total {total_bytes} exceeds 15 MiB"

    disk_files = {
        path.relative_to(FIXTURES_ROOT).as_posix()
        for path in FIXTURES_ROOT.rglob("*")
        if path.is_file()
    }
    expected_disk = seen_paths | EXCLUDED_FILES
    assert disk_files == expected_disk, (
        f"Manifest mismatch: extra={disk_files - expected_disk}, "
        f"missing={expected_disk - disk_files}"
    )


def test_no_watermarks_in_fixtures_or_archives() -> None:
    for path in sorted(FIXTURES_ROOT.rglob("*")):
        if not path.is_file():
            continue
        rel = path.relative_to(FIXTURES_ROOT).as_posix()
        raw = path.read_bytes()
        _check_text_clean(raw, rel)

        if path.suffix.lower() in {".docx", ".pptx", ".xlsx", ".ods", ".epub"}:
            with zipfile.ZipFile(io.BytesIO(raw), "r") as zf:
                for info in zf.infolist():
                    if info.file_size > 8 * 1024 * 1024:
                        continue
                    if "bomb/" in info.filename:
                        continue
                    _check_text_clean(zf.read(info.filename), f"{rel}!{info.filename}")


def test_special_fixture_invariants() -> None:
    gitattributes = (FIXTURES_ROOT / ".gitattributes").read_bytes()
    assert gitattributes == b"* -text\n"

    for rel in ("csv/data-crlf-bom.csv", "text/notes-crlf-bom.md"):
        raw = (FIXTURES_ROOT / rel).read_bytes()
        assert raw.startswith(b"\xef\xbb\xbf"), f"{rel} missing UTF-8 BOM"
        assert b"\r\n" in raw, f"{rel} missing CRLF line endings"

    nfd = (FIXTURES_ROOT / "text" / "malayalam-nfd.txt").read_bytes()
    nfc = (FIXTURES_ROOT / "text" / "malayalam-nfc.txt").read_bytes()
    assert nfd != nfc
    assert unicodedata.normalize("NFC", nfd.decode("utf-8")) == nfc.decode("utf-8")

    honest_bytes = (FIXTURES_ROOT / "docx" / "bomb-honest.docx").read_bytes()
    with zipfile.ZipFile(io.BytesIO(honest_bytes), "r") as zf:
        bomb_info = zf.getinfo("word/bomb/zeros.bin")
        assert bomb_info.file_size == 1024 * 1024 * 1024

    lying_bytes = (FIXTURES_ROOT / "docx" / "bomb-lying.docx").read_bytes()
    with zipfile.ZipFile(io.BytesIO(lying_bytes), "r") as zf:
        lying_info = zf.getinfo("word/bomb/zeros.bin")
        assert lying_info.file_size == 1024
        start = lying_info.header_offset
        local_uncomp = int.from_bytes(lying_bytes[start + 22 : start + 26], "little")
        assert local_uncomp == 1024
        decomp = zlib.decompressobj(-15)
        # Inflate raw compressed stream directly to confirm >= 600 MiB uncompressed payload
        name_len = int.from_bytes(lying_bytes[start + 26 : start + 28], "little")
        extra_len = int.from_bytes(lying_bytes[start + 28 : start + 30], "little")
        payload_start = start + 30 + name_len + extra_len
        compressed = lying_bytes[payload_start : payload_start + lying_info.compress_size]
        inflated_total = 0
        for offset in range(0, len(compressed), 65536):
            inflated_total += len(decomp.decompress(compressed[offset : offset + 65536]))
        inflated_total += len(decomp.flush())
        assert inflated_total >= 600 * 1024 * 1024

    nb = json.loads((FIXTURES_ROOT / "text" / "notebook.ipynb").read_text(encoding="utf-8"))
    code_cells = [c for c in nb["cells"] if c["cell_type"] == "code"]
    md_cells = [c for c in nb["cells"] if c["cell_type"] == "markdown"]
    assert len(code_cells) == 2 and len(md_cells) == 1
    assert all(c["outputs"] for c in code_cells)

    manifest = _load_manifest()
    classify_pdfs = [
        e for e in manifest["files"] if e["path"].startswith("classify/") and e["format"] == "pdf"
    ]
    assert len(classify_pdfs) == 40
    null_entries = [e for e in classify_pdfs if e["expect"]["subject"] is None]
    assert len(null_entries) == 2
    assert all(e["notes"] == "host batch" for e in null_entries)


def test_test_data_is_mit_and_embeds_no_font_program() -> None:
    """All test data is MIT licensed, and no fixture carries a font program.

    Font programs (FontFile, FontFile2, FontFile3) are usually under their own
    licence, so a PDF fixture names its font without embedding it, and the
    raster fixtures use Pillow's bundled Aileron (CC0).
    """
    repo = FIXTURES_ROOT.parent.parent
    for folder in (FIXTURES_ROOT, repo / "crates" / "lucubra-mcp" / "tests" / "fixtures"):
        licence = (folder / "LICENSE").read_text(encoding="utf-8")
        assert licence.startswith("MIT License"), f"{folder} needs the MIT LICENSE file"
    for pdf in sorted(FIXTURES_ROOT.rglob("*.pdf")):
        raw = pdf.read_bytes()
        for key in (b"/FontFile ", b"/FontFile2", b"/FontFile3"):
            assert key not in raw, f"{pdf.name} embeds a font program ({key!r})"
    for gen in sorted((repo / "scripts" / "fixtures").glob("*.py")):
        src = gen.read_text(encoding="utf-8")
        assert "Windows\Fonts" not in src and ".ttc" not in src, f"{gen.name} uses a system font"
