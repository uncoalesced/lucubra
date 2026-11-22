# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""Headless Blender scene extraction helper (SPEC section 10.2, S08)."""

from __future__ import annotations

import json
import sys


def main() -> int:
    if "--" not in sys.argv:
        return 2
    out_path = sys.argv[sys.argv.index("--") + 1]
    import bpy  # type: ignore[import-not-found]

    objects = []
    for obj in bpy.data.objects:
        entry = {
            "name": obj.name,
            "type": obj.type,
            "materials": [slot.material.name for slot in obj.material_slots if slot.material],
            "modifiers": [mod.type for mod in obj.modifiers],
        }
        if obj.type == "MESH" and obj.data is not None:
            entry["vertices"] = len(obj.data.vertices)
            entry["polygons"] = len(obj.data.polygons)
        objects.append(entry)

    payload = {
        "scene": bpy.context.scene.name if bpy.context.scene else "Scene",
        "objects": objects,
        "materials": [mat.name for mat in bpy.data.materials],
    }
    with open(out_path, "w", encoding="utf-8") as handle:
        json.dump(payload, handle)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
