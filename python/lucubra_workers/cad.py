# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""DXF CAD extraction with ezdxf (SPEC section 10.2, ADR 0011)."""

from __future__ import annotations

import os
from collections import defaultdict
from importlib import metadata
from typing import Any

from lucubra_workers.chunking import split_text
from lucubra_workers.errors import ExtractError

MAX_FILE_BYTES = 1 << 30


def _extractor() -> str:
    try:
        return f"ezdxf@{metadata.version('ezdxf')}"
    except metadata.PackageNotFoundError:
        return "ezdxf"


def _entity_text(entity: Any) -> str:
    kind = entity.dxftype()
    if kind == "TEXT":
        return (entity.dxf.text or "").strip()
    if kind == "MTEXT":
        return (entity.plain_text() or "").strip()
    if kind == "DIMENSION":
        raw = (getattr(entity.dxf, "text", "") or "").strip()
        if raw == "<>":
            try:
                return f"{entity.get_measurement():.4g}"
            except Exception:
                return ""
        return raw
    if kind == "ATTRIB":
        tag = (getattr(entity.dxf, "tag", "") or "").strip()
        val = (getattr(entity.dxf, "text", "") or "").strip()
        if tag and val:
            return f"{tag}: {val}"
        return val
    return ""


def extract_dxf(path: str) -> dict[str, Any]:
    """Extract layers, blocks and text/dimension entities from a DXF file."""
    import ezdxf
    from ezdxf.lldxf.const import DXFError

    try:
        size = os.path.getsize(path)
    except OSError as exc:
        raise ExtractError("corrupt", str(exc)) from exc
    if size > MAX_FILE_BYTES:
        raise ExtractError("limit_exceeded", f"{size} bytes is over the input limit")

    try:
        doc = ezdxf.readfile(path)
    except (DXFError, OSError, UnicodeError, ValueError) as exc:
        raise ExtractError("corrupt", f"unreadable DXF: {exc}") from exc

    layers = sorted({layer.dxf.name for layer in doc.layers if layer.dxf.name})
    blocks = sorted(
        {
            block.name
            for block in doc.blocks
            if not block.is_any_layout and not block.name.startswith("*")
        }
    )

    by_layer: dict[str, list[str]] = defaultdict(list)
    entity_count = 0
    for entity in doc.modelspace():
        entity_count += 1
        layer_name = getattr(entity.dxf, "layer", "0") or "0"
        if entity.dxftype() == "INSERT":
            block_name = getattr(entity.dxf, "name", "")
            if block_name:
                by_layer[layer_name].append(f"[block] {block_name}")
            for attrib in getattr(entity, "attribs", ()):
                text = _entity_text(attrib)
                if text:
                    by_layer[getattr(attrib.dxf, "layer", layer_name) or layer_name].append(text)
        else:
            text = _entity_text(entity)
            if text:
                by_layer[layer_name].append(text)

    chunks: list[dict[str, str]] = []
    summary_lines = [
        "Format: DXF drawing",
        f"Layers: {', '.join(layers) if layers else 'none'}",
        f"Blocks: {', '.join(blocks) if blocks else 'none'}",
        f"Modelspace entities: {entity_count}",
    ]
    chunks.append({"ref": "l:summary", "text": "\n".join(summary_lines)})

    for layer_name in sorted(by_layer):
        body = "\n".join(by_layer[layer_name])
        for part, piece in enumerate(split_text(body)):
            ref = f"l:{layer_name}" if part == 0 else f"l:{layer_name}.{part}"
            chunks.append({"ref": ref, "text": piece})

    hints = [name for name in layers if name != "0"] or layers
    return {
        "kind": "dxf",
        "extractor": _extractor(),
        "chunks": chunks,
        "needs_vision": [],
        "units_hint": hints,
        "truncated": False,
        "meta": {"layers": layers, "blocks": blocks, "entities": entity_count},
    }
