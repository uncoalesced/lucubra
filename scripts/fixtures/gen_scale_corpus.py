# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
"""Generate a deterministic 3,000-file (~5 GB) scale test corpus (S36) outside the repo."""

from __future__ import annotations

import argparse
import hashlib
import io
import random
import sys
import zipfile
from pathlib import Path

from _common import FIXED_ZIP_TIME, REPO_ROOT

VOCAB = [
    "enthalpy",
    "entropy",
    "diffraction",
    "eigenvalue",
    "gradient",
    "covariance",
    "stochastic",
    "capacitor",
    "inductance",
    "impedance",
    "catalysis",
    "equilibrium",
    "isotopic",
    "chromatography",
    "membrane",
    "organelle",
    "osmosis",
    "kinematics",
    "momentum",
    "torsion",
    "elasticity",
    "polymer",
    "crystal",
    "lattice",
    "semiconductor",
    "transistor",
    "Fourier",
    "Laplace",
    "matrix",
    "orthogonal",
    "simplex",
    "convexity",
]


def _validate_out_dir(out_arg: str) -> Path:
    out = Path(out_arg).resolve()
    drive = out.drive.upper()
    if drive.startswith("C:") or str(out).upper().startswith("C:\\"):
        raise ValueError(f"Refusing --out on C: drive: {out}")

    protected_roots = [REPO_ROOT.resolve(), (REPO_ROOT.parent / "Lucubra").resolve()]
    for root in protected_roots:
        if out == root or out.is_relative_to(root) or root.is_relative_to(out):
            raise ValueError(f"Refusing --out inside or enclosing repository {root}: {out}")
    return out


def _make_filler_pool(rng: random.Random, pool_size: int = 131072) -> bytes:
    """Create deterministic readable ASCII lines for fast unique-block composition."""
    lines: list[str] = []
    current = 0
    while current < pool_size:
        words = [rng.choice(VOCAB) for _ in range(10)]
        line = " ".join(words) + ".\n"
        lines.append(line)
        current += len(line)
    return "".join(lines).encode("utf-8")


def _unique_payload(pool: bytes, file_idx: int, target_size: int) -> bytes:
    """Assemble `target_size` bytes of deterministic, file-unique readable data."""
    if target_size <= 0:
        return b""
    tag = hashlib.sha256(f"scale-file-{file_idx}".encode()).hexdigest()
    header = f"Study Unit {file_idx:04d} Record {tag}\n".encode()
    if target_size <= len(header):
        return header[:target_size]

    chunks = [header]
    written = len(header)
    pool_len = len(pool)
    block_idx = 0
    while written < target_size:
        offset = ((file_idx * 4099) + (block_idx * 1031)) % pool_len
        block_tag = f"[{file_idx:04d}:{block_idx:04d}] ".encode()
        remaining = target_size - written
        if remaining <= len(block_tag):
            chunks.append(block_tag[:remaining])
            break
        chunks.append(block_tag)
        written += len(block_tag)
        take = min(target_size - written, pool_len - offset, 65536)
        chunks.append(pool[offset : offset + take])
        written += take
        block_idx += 1
    return b"".join(chunks)


def _wrap_pdf(file_idx: int, body_bytes: bytes) -> bytes:
    """Wrap unique text/padding in a minimal valid single-page PDF."""
    stream_text = f"BT /F1 11 Tf 72 720 Td (Scale Fixture {file_idx:04d}) Tj ET\n".encode()
    objs = [
        b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n",
        b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n",
        (
            b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            b"/Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>\nendobj\n"
        ),
        b"4 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n",
        (
            f"5 0 obj\n<< /Length {len(stream_text)} >>\nstream\n".encode()
            + stream_text
            + b"endstream\nendobj\n"
        ),
    ]
    buf = io.BytesIO()
    buf.write(b"%PDF-1.4\n")
    pad_prefix = b"% "
    pad = pad_prefix + body_bytes.replace(b"\n", b"\n% ") + b"\n"
    buf.write(pad)
    offsets: list[int] = []
    for obj in objs:
        offsets.append(buf.tell())
        buf.write(obj)
    xref_pos = buf.tell()
    buf.write(f"xref\n0 {len(objs) + 1}\n0000000000 65535 f \n".encode())
    for off in offsets:
        buf.write(f"{off:010d} 00000 n \n".encode())
    buf.write(
        f"trailer\n<< /Size {len(objs) + 1} /Root 1 0 R >>\nstartxref\n{xref_pos}\n%%EOF\n".encode()
    )
    return buf.getvalue()


def _wrap_docx(file_idx: int, body_bytes: bytes) -> bytes:
    """Wrap unique readable content in a minimal valid STORED DOCX archive."""
    text_str = (
        body_bytes.decode("ascii", errors="replace").replace("&", "&amp;").replace("<", "&lt;")
    )
    content_types = (
        b'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        b'<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        b'<Default Extension="rels" '
        b'ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
        b'<Default Extension="xml" ContentType="application/xml"/>'
        b'<Override PartName="/word/document.xml" '
        b"ContentType="
        b'"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
        b"</Types>"
    )
    rels = (
        b'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        b'<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        b'<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/'
        b'officeDocument/2006/relationships/officeDocument" '
        b'Target="word/document.xml"/>'
        b"</Relationships>"
    )
    doc_xml = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">'
        f"<w:body><w:p><w:r><w:t>Scale DOCX {file_idx:04d}\n{text_str}</w:t></w:r></w:p></w:body>"
        "</w:document>"
    ).encode()

    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", compression=zipfile.ZIP_STORED) as zf:
        for name, data in [
            ("[Content_Types].xml", content_types),
            ("_rels/.rels", rels),
            ("word/document.xml", doc_xml),
        ]:
            info = zipfile.ZipInfo(name, date_time=FIXED_ZIP_TIME)
            info.compress_type = zipfile.ZIP_STORED
            info.external_attr = 0o644 << 16
            zf.writestr(info, data)
    return buf.getvalue()


def generate_corpus(out_dir: Path, *, count: int, target_bytes: int, seed: int) -> tuple[int, int]:
    """Generate `count` varied files totalling approximately `target_bytes` under `out_dir`."""
    rng = random.Random(seed)
    pool = _make_filler_pool(rng)
    out_dir.mkdir(parents=True, exist_ok=True)

    weights = [rng.uniform(0.5, 1.5) for _ in range(count)]
    weight_sum = sum(weights)
    extensions = ("txt", "md", "pdf", "docx")
    total_written = 0

    for idx in range(count):
        ext = extensions[idx % len(extensions)]
        subdir = out_dir / f"unit_{idx // 100:02d}"
        subdir.mkdir(parents=True, exist_ok=True)
        desired = max(256, int(target_bytes * (weights[idx] / weight_sum)))
        raw_body = _unique_payload(pool, idx, desired)

        if ext == "txt":
            data = raw_body
        elif ext == "md":
            heading = f"# Note {idx:04d}\n\n".encode()
            data = heading + raw_body[: max(0, desired - len(heading))]
        elif ext == "pdf":
            data = _wrap_pdf(idx, raw_body)
        else:
            data = _wrap_docx(idx, raw_body)

        file_path = subdir / f"doc_{idx:04d}.{ext}"
        file_path.write_bytes(data)
        total_written += len(data)

    return count, total_written


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", required=True, help="Output directory (must not be C: or in repo)")
    parser.add_argument("--count", type=int, default=3000, help="Number of files to generate")
    parser.add_argument(
        "--target-bytes",
        type=int,
        default=5_000_000_000,
        help="Approximate total corpus size in bytes",
    )
    parser.add_argument("--seed", type=int, default=20261114, help="Fixed random seed")
    args = parser.parse_args(argv)

    try:
        out_dir = _validate_out_dir(args.out)
    except ValueError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2

    files, total = generate_corpus(
        out_dir,
        count=args.count,
        target_bytes=args.target_bytes,
        seed=args.seed,
    )
    print(f"generated {files} files, {total} bytes ({total / (1024**3):.2f} GiB) in {out_dir}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
