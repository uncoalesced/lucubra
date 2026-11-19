# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""Python dependency licence check.

Walks the runtime dependencies of an installed distribution (extras excluded)
and fails when a dependency declares a licence outside the permissive allow
list, or declares none that can be read. Run inside the project environment.
"""

from __future__ import annotations

import argparse
import re
import sys
from importlib import metadata

ALLOWED = {
    "MIT",
    "MIT-0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "Apache-2.0",
    "ISC",
    "Zlib",
    "Unicode-3.0",
    "MPL-2.0",
    "PSF-2.0",
    "CC0-1.0",
    "0BSD",
    "MIT-CMU",
}
# Distributions whose metadata states the licence as free text, reviewed by
# hand against the licence files in the wheel. The exact declared text is
# pinned: if a release changes it, the check fails again until re-reviewed.
REVIEWED = {
    # pypdfium2 5.15.2 (2026-11-14): Apache-2.0 OR BSD-3-Clause for the
    # bindings; the wheel's BUILD_LICENSES cover PDFium (BSD-3-Clause) and
    # bundled permissive code (freetype FTL, ICU, libjpeg-turbo IJG and BSD,
    # libpng, libtiff, zlib, lcms, openjpeg, harfbuzz, abseil, Boost). The
    # musl and libgcc files apply only to Linux musl wheels.
    "pypdfium2": "BSD-3-Clause, Apache-2.0, dependency licenses",
}
CLASSIFIERS = {
    "MIT License": "MIT",
    "BSD License": "BSD-3-Clause",
    "Apache Software License": "Apache-2.0",
    "ISC License (ISCL)": "ISC",
    "zlib/libpng License": "Zlib",
    "Mozilla Public License 2.0 (MPL 2.0)": "MPL-2.0",
}
NAME = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*")


def expression_allowed(expression: str) -> bool:
    """Evaluate a flat SPDX expression: AND needs every term, OR needs one."""
    terms = [t for t in re.split(r"\s+(?:AND|OR)\s+", expression.replace("(", "").replace(")", ""))]
    terms = [t.strip() for t in terms if t.strip()]
    if not terms:
        return False
    if re.search(r"\sAND\s", expression):
        return all(t in ALLOWED for t in terms)
    return any(t in ALLOWED for t in terms)


def declared_licence(dist: metadata.Distribution) -> str | None:
    meta = dist.metadata
    if meta.get("License-Expression"):
        return meta["License-Expression"]
    found = [
        CLASSIFIERS.get(c.rsplit(" :: ", 1)[-1], c.rsplit(" :: ", 1)[-1])
        for c in meta.get_all("Classifier") or []
        if c.startswith("License ::")
    ]
    if found:
        return " AND ".join(sorted(set(found)))
    licence = (meta.get("License") or "").strip()
    return licence if licence and "\n" not in licence else None


def runtime_requirements(dist: metadata.Distribution) -> list[str]:
    names = []
    for requirement in dist.requires or []:
        if re.search(r"extra\s*==", requirement.partition(";")[2]):
            continue
        match = NAME.match(requirement.strip())
        if match:
            names.append(match.group(0))
    return names


def closure(root: str) -> dict[str, metadata.Distribution]:
    seen: dict[str, metadata.Distribution] = {}
    queue = runtime_requirements(metadata.distribution(root))
    while queue:
        name = queue.pop()
        key = re.sub(r"[-_.]+", "-", name).lower()
        if key in seen:
            continue
        seen[key] = metadata.distribution(name)
        queue.extend(runtime_requirements(seen[key]))
    return seen


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--dist", default="lucubra")
    args = parser.parse_args(argv)
    try:
        dependencies = closure(args.dist)
    except metadata.PackageNotFoundError as exc:
        print(f"licence check error: distribution not installed: {exc.name}")
        return 2
    failures = 0
    for key in sorted(dependencies):
        licence = declared_licence(dependencies[key])
        ok = licence is not None and (expression_allowed(licence) or REVIEWED.get(key) == licence)
        failures += not ok
        print(f"{'ok  ' if ok else 'FAIL'} {key}: {licence or 'no readable licence'}")
    status = "FAILED" if failures else "ok"
    print(
        f"licence check {status}: {len(dependencies)} runtime dependencies, {failures} violations"
    )
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
