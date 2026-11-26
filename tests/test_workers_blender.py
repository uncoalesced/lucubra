# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""Blender scene helper, run against a fake `bpy` (the real one exists only inside Blender)."""

import json
import sys
import types

from lucubra_workers import blender_extract


def ns(**kw):
    return types.SimpleNamespace(**kw)


def fake_bpy():
    steel = ns(name="Steel")
    mesh = ns(
        name="Gear",
        type="MESH",
        material_slots=[ns(material=steel), ns(material=None)],
        modifiers=[ns(type="BEVEL")],
        data=ns(vertices=[0] * 8, polygons=[0] * 6),
    )
    lamp = ns(name="Lamp", type="LIGHT", material_slots=[], modifiers=[], data=None)
    return ns(
        data=ns(objects=[mesh, lamp], materials=[steel]),
        context=ns(scene=ns(name="Gearbox")),
    )


def test_main_writes_objects_materials_and_counts(tmp_path, monkeypatch):
    out = tmp_path / "scene.json"
    monkeypatch.setitem(sys.modules, "bpy", fake_bpy())
    monkeypatch.setattr(sys, "argv", ["blender", "--", str(out)])
    assert blender_extract.main() == 0
    payload = json.loads(out.read_text(encoding="utf-8"))
    assert payload["scene"] == "Gearbox"
    assert payload["materials"] == ["Steel"]
    gear, lamp = payload["objects"]
    assert gear == {
        "name": "Gear",
        "type": "MESH",
        "materials": ["Steel"],
        "modifiers": ["BEVEL"],
        "vertices": 8,
        "polygons": 6,
    }
    assert "vertices" not in lamp


def test_main_without_separator_refuses(monkeypatch):
    monkeypatch.setattr(sys, "argv", ["blender"])
    assert blender_extract.main() == 2
