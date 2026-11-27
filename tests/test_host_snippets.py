# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""Host configuration snippets in docs/hosts parse and register lucubra (M9-6)."""

import json
import re
import tomllib
from pathlib import Path

import pytest

HOSTS = Path(__file__).resolve().parent.parent / "docs" / "hosts"
FENCE = re.compile(r"```(json|toml|yaml)\n(.*?)```", re.S)


def snippets():
    for doc in sorted(HOSTS.glob("*.md")):
        for n, m in enumerate(FENCE.finditer(doc.read_text(encoding="utf-8"))):
            yield pytest.param(m.group(1), m.group(2), id=f"{doc.stem}-{n}-{m.group(1)}")


def check_yaml(text: str) -> None:
    # shortcut: structural check only (no PyYAML in the lock); parse for real if YAML grows.
    assert chr(9) not in text, "tab in YAML"
    lines = [line for line in text.splitlines() if line.strip()]
    assert lines[0] == "mcp_servers:", lines[0]
    assert lines[1] == "  lucubra:", lines[1]
    for line in lines:
        indent = len(line) - len(line.lstrip(" "))
        assert indent % 2 == 0, f"odd indent: {line!r}"
        assert line.count('"') % 2 == 0, f"unbalanced quotes: {line!r}"


def names_lucubra(value) -> bool:
    if isinstance(value, dict):
        return "lucubra" in value or any(names_lucubra(v) for v in value.values())
    if isinstance(value, list):
        return any(names_lucubra(v) for v in value)
    return False


def test_every_host_doc_shows_how_to_start_lucubra():
    docs = {doc.stem: doc.read_text(encoding="utf-8") for doc in HOSTS.glob("*.md")}
    assert len(docs) == 11
    missing = [
        name
        for name, text in docs.items()
        if not FENCE.search(text) and not re.search(r"lucubra(\.exe)? mcp", text)
    ]
    assert not missing, missing


@pytest.mark.parametrize(("lang", "text"), list(snippets()))
def test_snippet_parses_and_names_lucubra(lang, text):
    if lang == "yaml":
        check_yaml(text)
        return
    data = json.loads(text) if lang == "json" else tomllib.loads(text)
    is_tool_call = isinstance(data, dict) and str(data.get("name", "")).startswith("lucubra_")
    assert is_tool_call or names_lucubra(data), text
