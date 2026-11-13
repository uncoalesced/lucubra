# ADR 0006: IfcOpenShell as an optional extra

- Status: Accepted
- Date: 2026-11-13
- Decision references: D18, D20, SPEC sections 4.3 and 10.2

## Context

IFC files are read with IfcOpenShell through an optional extra, `lucubra[ifc]`. SPEC section 4.3 asked whether its licence is compatible with an Apache-2.0 project when used this way.

## Evidence

- PyPI lists `ifcopenshell` 0.9.0.post1 with the licence expression `LGPL-3.0-or-later`. Its runtime dependencies are shapely, numpy, isodate, python-dateutil, lark, pyparsing and typing-extensions.
- Open CASCADE Technology, which IfcOpenShell uses for geometry, is licensed under LGPL-2.1 with an additional exception published by its maintainers.
- Under section 0 of the LGPL version 3, a program that uses a library's interface without being based on it is an "Application". The obligations in section 4 apply to whoever conveys a "Combined Work".
- The Apache Software Foundation's third-party licence policy permits LGPL components as optional dependencies that are not bundled. It binds only foundation projects and is used here as guidance.

This is engineering licence analysis, not legal advice.

## Decision

1. `ifc` is declared as an optional dependency in package metadata. The installer fetches IfcOpenShell from PyPI under its own licence.
2. IfcOpenShell is imported only inside a worker subprocess. No Lucubra code links against it.
3. IfcOpenShell and Open CASCADE are never included in the Lucubra wheel, the bundle or any installer.
4. The user documentation for the extra names both licences and how to install the extra.

## Consequences

- Lucubra conveys no Combined Work, so it carries no LGPL obligation under this design.
- Any future installer that bundles IfcOpenShell would take on the section 4 obligations of the LGPL and requires a new decision.
- The `ifcopenshell` 0.9.0.post1 `win_amd64` wheel bundles more than 50 Open CASCADE libraries (reported 2026-11-13). PyPI distributes them, not Lucubra, so the decision stands. The Python runtime dependencies of `ifcopenshell` were reported permissive.
