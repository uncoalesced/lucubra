# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
# /// script
# requires-python = ">=3.10"
# dependencies = [
#   "reportlab>=5,<6",
# ]
# ///
"""Generate classification corpus fixtures (S03) under tests/fixtures/classify/."""

from __future__ import annotations

import io
import json
import sys

from _common import FIXTURES_ROOT, write_bytes, write_text_lf

SUBJECTS: list[dict[str, object]] = [
    {
        "slug": "stat201",
        "title": "Probability and Inference",
        "course_code": "STAT201",
        "keywords": ["bayes", "variance", "poisson", "estimator", "quantile"],
    },
    {
        "slug": "cs302",
        "title": "Algorithms and Complexity",
        "course_code": "CS302",
        "keywords": ["dijkstra", "amortized", "recurrence", "bipartite", "npcomplete"],
    },
    {
        "slug": "econ104",
        "title": "Macroeconomic Policy",
        "course_code": "ECON104",
        "keywords": ["inflation", "keynesian", "liquidity", "multiplier", "gdp"],
    },
    {
        "slug": "chem210",
        "title": "Organic Reaction Kinetics",
        "course_code": "CHEM210",
        "keywords": ["nucleophile", "enantiomer", "enthalpy", "carbocation", "esterification"],
    },
]


def _file_specs() -> list[dict[str, object]]:
    """Return deterministic specifications for profile.json and the 40 classify/ PDFs."""
    specs: list[dict[str, object]] = []
    # 38 clear files: 9 for stat201, 9 for cs302, 10 for econ104, 10 for chem210.
    coded_counts = {"stat201": 9, "cs302": 9, "econ104": 10, "chem210": 10}
    doc_num = 1
    for subj in SUBJECTS:
        slug = str(subj["slug"])
        code = str(subj["course_code"])
        title = str(subj["title"])
        kws = list(subj["keywords"])  # type: ignore[arg-type]
        count = coded_counts[slug]
        for idx in range(count):
            signal = idx % 3
            if signal == 0:
                # Signal in filename
                filename = f"{slug}-note-{idx + 1:02d}.pdf"
                pdf_title = f"{title} Summary {idx + 1}"
                first_line = f"Lecture summary covering {kws[0]} and {kws[1]}."
            elif signal == 1:
                # Signal in PDF Title metadata
                filename = f"handout-{doc_num:02d}.pdf"
                pdf_title = f"{code} {title} Handout"
                first_line = f"Topic overview of {kws[2]} and {kws[3]}."
            else:
                # Signal in first line of text
                filename = f"reading-{doc_num:02d}.pdf"
                pdf_title = f"Study Reading {doc_num}"
                first_line = f"{code}: Analytical notes on {kws[4]} and {kws[0]}."

            body_lines = [
                first_line,
                (
                    f"Core concepts examine {kws[1]}, {kws[2]}, and {kws[3]} "
                    f"in structured problem sets."
                ),
                (
                    f"Worked derivations verify how {kws[4]} behaves under standard "
                    "boundary assumptions."
                ),
            ]
            specs.append(
                {
                    "rel_path": f"classify/{filename}",
                    "pdf_title": pdf_title,
                    "lines": body_lines,
                    "subject": slug,
                    "course_code": code,
                    "notes": (
                        f"Clear {code} fixture "
                        f"(signal={('filename', 'title', 'first_line')[signal]})"
                    ),
                }
            )
            doc_num += 1

    # 2 ambiguous files: no course code anywhere, keywords split evenly (2 + 2).
    ambiguous = [
        {
            "rel_path": "classify/ambiguous-01.pdf",
            "pdf_title": "Mixed Interdisciplinary Seminar Notes",
            "lines": [
                "Interdisciplinary methods combine bayes estimation with variance bounds.",
                "Graph models apply dijkstra traversal alongside amortized cost accounting.",
                "Both quantitative frameworks compare discrete and continuous error bounds.",
            ],
            "subject": None,
            "course_code": None,
            "notes": "host batch",
        },
        {
            "rel_path": "classify/ambiguous-02.pdf",
            "pdf_title": "Applied Quantitative Workshop Sheet",
            "lines": [
                "Industrial output reports track inflation alongside liquidity constraints.",
                "Process calorimetry tracks enthalpy shifts across nucleophile pathways.",
                "Workshop exercises balance thermodynamic tables with aggregate indices.",
            ],
            "subject": None,
            "course_code": None,
            "notes": "host batch",
        },
    ]
    specs.extend(ambiguous)
    return specs


def manifest_entries() -> list[dict[str, object]]:
    """Return ADR 0011 manifest descriptors for classify/profile.json and 40 PDFs."""
    entries: list[dict[str, object]] = [
        {
            "path": "classify/profile.json",
            "format": "unknown",
            "stub": False,
            "generator": "scripts/fixtures/gen_classify.py",
            "scenarios": ["S03"],
            "expect": {},
            "notes": "S03 subject profile configuration (four subjects, five keywords each)",
        }
    ]
    for spec in _file_specs():
        entries.append(
            {
                "path": spec["rel_path"],
                "format": "pdf",
                "stub": False,
                "generator": "scripts/fixtures/gen_classify.py",
                "scenarios": ["S03"],
                "expect": {
                    "outcome": "extracted",
                    "pages": 1,
                    "needs_vision": [],
                    "subject": spec["subject"],
                    "course_code": spec["course_code"],
                },
                "notes": spec["notes"],
            }
        )
    return entries


def _build_one_page_pdf(title: str, lines: list[str]) -> bytes:
    from reportlab.pdfgen import canvas

    buf = io.BytesIO()
    c = canvas.Canvas(buf, invariant=1)
    c.setAuthor("fixture")
    c.setCreator("fixture")
    c.setSubject("classification fixture")
    c.setTitle(title)
    c._doc.info.producer = "ReportLab"
    c.setFont("Helvetica", 11)
    y = 730
    for line in lines:
        c.drawString(64, y, line)
        y -= 22
    c.showPage()
    c.save()
    return buf.getvalue()


def generate() -> None:
    """Write classify/profile.json and all 40 classification PDFs."""
    profile = {"subjects": SUBJECTS}
    write_text_lf(
        FIXTURES_ROOT / "classify" / "profile.json",
        json.dumps(profile, indent=2) + "\n",
    )
    for spec in _file_specs():
        pdf_bytes = _build_one_page_pdf(
            str(spec["pdf_title"]),
            list(spec["lines"]),  # type: ignore[arg-type]
        )
        write_bytes(FIXTURES_ROOT / str(spec["rel_path"]), pdf_bytes)


def main() -> int:
    generate()
    return 0


if __name__ == "__main__":
    sys.exit(main())
