# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""Extraction errors carrying ADR 0011 failure codes."""

from __future__ import annotations


class ExtractError(Exception):
    """An extraction failure with an ADR 0011 code."""

    def __init__(self, code: str, detail: str) -> None:
        super().__init__(detail)
        self.code = code
        self.detail = detail
