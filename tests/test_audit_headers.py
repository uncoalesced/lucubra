# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""Tests for the header audit, including planted violations of each kind."""

from pathlib import Path

import audit_headers as audit

RS_HEADER = f"// {audit.SPDX}\n// {audit.PROVENANCE}\n"
PY_HEADER = f"# {audit.SPDX}\n# {audit.PROVENANCE}\n"


def make_clean_tree(root: Path) -> None:
    (root / "src").mkdir()
    (root / "src" / "lib.rs").write_text(RS_HEADER + "\npub fn f() {}\n", encoding="utf-8")
    (root / "tool.py").write_text("#!/usr/bin/env python3\n" + PY_HEADER, encoding="utf-8")
    (root / "config.toml").write_text('name = "x"\n', encoding="utf-8")
    (root / "notes.md").write_text("# Notes\n", encoding="utf-8")
    (root / "blob.bin").write_bytes(b"\0\1\2" + audit.PROVENANCE.encode())


def test_clean_tree_passes(tmp_path):
    make_clean_tree(tmp_path)
    violations, sources, others = audit.audit(tmp_path)
    assert violations == []
    assert (sources, others) == (2, 3)
    assert audit.main(["--root", str(tmp_path)]) == 0


def test_source_without_provenance_fails(tmp_path):
    make_clean_tree(tmp_path)
    (tmp_path / "src" / "bare.rs").write_text(f"// {audit.SPDX}\nfn g() {{}}\n", encoding="utf-8")
    violations, _, _ = audit.audit(tmp_path)
    assert violations == ["src/bare.rs: missing provenance comment"]
    assert audit.main(["--root", str(tmp_path)]) == 1


def test_source_without_spdx_fails(tmp_path):
    make_clean_tree(tmp_path)
    (tmp_path / "x.sh").write_text(f"#!/bin/sh\n# {audit.PROVENANCE}\n", encoding="utf-8")
    assert audit.audit(tmp_path)[0] == ["x.sh: missing SPDX line"]


def test_byte_order_mark_before_headers_passes(tmp_path):
    (tmp_path / "bom.rs").write_bytes(b"\xef\xbb\xbf" + RS_HEADER.encode("utf-8"))
    (tmp_path / "bom.ps1").write_bytes(b"\xef\xbb\xbf" + PY_HEADER.encode("utf-8"))
    assert audit.audit(tmp_path) == ([], 2, 0)


def test_header_below_the_window_fails(tmp_path):
    (tmp_path / "late.py").write_text("\n" * 10 + PY_HEADER, encoding="utf-8")
    assert audit.audit(tmp_path)[0] == [
        "late.py: missing SPDX line",
        "late.py: missing provenance comment",
    ]


def test_repeated_provenance_in_source_fails(tmp_path):
    body = RS_HEADER + f'const S: &str = "{audit.PROVENANCE}";\n'
    (tmp_path / "twice.rs").write_text(body, encoding="utf-8")
    assert audit.audit(tmp_path)[0] == ["twice.rs: provenance text appears 2 times"]


def test_watermark_outside_source_fails(tmp_path):
    make_clean_tree(tmp_path)
    (tmp_path / "doc.md").write_text(f"<!-- {audit.PROVENANCE} -->\n", encoding="utf-8")
    (tmp_path / "ci.yml").write_text(f"# {audit.PROVENANCE.upper()}\n", encoding="utf-8")
    (tmp_path / "out.md").write_text(f"{audit.BANNER}\n", encoding="utf-8")
    assert audit.audit(tmp_path)[0] == [
        "ci.yml: provenance text in a non-source file",
        "doc.md: provenance text in a non-source file",
        "out.md: generated-by banner in a non-source file",
    ]


def make_generated_workspace(root: Path) -> None:
    (root / ".lucubra" / "events").mkdir(parents=True)
    (root / ".lucubra" / "events" / "d-0000000a.jsonl").write_text('{"v":1}\n', encoding="utf-8")
    (root / "AGENTS.md").write_text("# Study workspace\n", encoding="utf-8")
    (root / "subjects" / "maths").mkdir(parents=True)
    (root / "subjects" / "maths" / "agents_maths.md").write_text("# Maths\n", encoding="utf-8")
    (root / "page.png").write_bytes(b"\x89PNG\0" + audit.PROVENANCE.encode())


def test_clean_generated_workspace_passes(tmp_path):
    make_generated_workspace(tmp_path)
    assert audit.audit_generated(tmp_path) == ([], 3)
    assert audit.main(["--generated", str(tmp_path)]) == 0


def test_watermark_in_generated_workspace_fails(tmp_path):
    make_generated_workspace(tmp_path)
    (tmp_path / "subjects" / "maths" / "UNIT.md").write_text(
        f"<!-- {audit.PROVENANCE} -->\n", encoding="utf-8"
    )
    (tmp_path / "AGENTS.md").write_text(f"{audit.BANNER}\n", encoding="utf-8")
    assert audit.audit_generated(tmp_path)[0] == [
        "AGENTS.md: generated-by banner in a generated workspace file",
        "subjects/maths/UNIT.md: provenance text in a generated workspace file",
    ]
    assert audit.main(["--generated", str(tmp_path)]) == 1


def test_skipped_directories_are_not_walked(tmp_path):
    make_clean_tree(tmp_path)
    (tmp_path / "target").mkdir()
    (tmp_path / "target" / "gen.rs").write_text("fn h() {}\n", encoding="utf-8")
    assert audit.audit(tmp_path)[0] == []
