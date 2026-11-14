# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""Tests for the Python dependency licence check."""

import check_python_licenses as check
import pytest


@pytest.mark.parametrize(
    "expression",
    [
        "MIT",
        "Apache-2.0",
        "MIT OR Apache-2.0",
        "(MIT AND BSD-3-Clause)",
        "LGPL-3.0-only OR MIT",
        "PSF-2.0",
        "BSD-3-Clause AND 0BSD AND MIT AND Zlib AND CC0-1.0",
    ],
)
def test_permissive_expressions_pass(expression):
    assert check.expression_allowed(expression)


@pytest.mark.parametrize(
    "expression",
    ["", "LGPL-3.0-only", "GPL-3.0-or-later", "AGPL-3.0-only", "MIT AND LGPL-3.0-only", "Custom"],
)
def test_other_expressions_fail(expression):
    assert not check.expression_allowed(expression)


def test_reviewed_text_is_pinned():
    text = check.REVIEWED["pypdfium2"]
    assert not check.expression_allowed(text), "free text never passes as an expression"
    assert check.REVIEWED.get("pypdfium2") == text
    assert check.REVIEWED.get("pypdfium2") != text + ", GPL-3.0"


def test_missing_distribution_is_an_error(capsys):
    assert check.main(["--dist", "no-such-distribution-for-this-test"]) == 2
    assert "not installed" in capsys.readouterr().out
