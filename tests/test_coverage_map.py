# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""Every module, MCP tool and acceptance scenario has its own test suite.

Rules:
1. Every crate has a `tests/` folder.
2. Every Rust source file has a `#[cfg(test)]` module or is listed in
   COVERED_BY with the integration suite that tests it.
3. Every MCP tool has `crates/lucubra-mcp/tests/tool_<name>.rs`.
4. Every Python worker module has a `tests/test_workers*.py` that imports it.
5. Every scenario S01..S59 has a test function named `sNN_...`, or an entry in
   SCENARIO_TESTS naming an existing test, or an entry in PENDING with the
   phase that builds it. PENDING must be empty at the release candidate.
"""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CRATES = ROOT / "crates"

# Source files tested through an integration suite instead of a unit module.
COVERED_BY = {
    "lucubra-cli/src/main.rs": "lucubra-cli/tests/workspace_cli.rs",
    "lucubra-mcp/src/server.rs": "lucubra-mcp/tests/protocol_and_gates.rs",
    "lucubra-mcp/src/transport.rs": "lucubra-mcp/tests/stdio_transcripts.rs",
    "lucubra-mcp/src/tools/mod.rs": "lucubra-mcp/tests/protocol_and_gates.rs",
    "lucubra-pack/src/ingest.rs": "lucubra-pack/tests/pack.rs",
}
# Files with no logic of their own.
NO_LOGIC = {
    "lucubra-core/src/error.rs",
    "lucubra-ingest/src/lib.rs",
    "lucubra-mcp/src/lib.rs",
    "lucubra-pack/src/lib.rs",
}

# ruff: noqa: E501 (the scenario map lines are paths and names)
# Scenarios whose test predates the sNN_ naming: "<path under crates or tests>::<function>".
SCENARIO_TESTS = {
    "S04": "lucubra-ingest/tests/vision.rs::m2_5_vision_renders_once_and_transcript_clears_pending",
    "S07": "lucubra-ingest/tests/doctor_tier2.rs::m2_8_pending_tier2_requeues_when_tools_appear_and_unsupported_stays_failed",
    "S08": "lucubra-ingest/tests/doctor_tier2.rs::m2_8_pending_tier2_requeues_when_tools_appear_and_unsupported_stays_failed",
    "S09": "lucubra-ingest/tests/doctor_tier2.rs::m2_8_pending_tier2_requeues_when_tools_appear_and_unsupported_stays_failed",
    "S12": "lucubra-mcp/tests/protocol_and_gates.rs::budget_caps_and_wrap_up_triggers_across_free_paid_and_local_tiers",
    "S20": "lucubra-ingest/tests/scan.rs::online_only_files_are_reported_and_not_logged",
    "S30": "tests/test_workers_office.py::test_lying_header_zip_bomb_is_limit_exceeded",
    "S37": "lucubra-mcp/tests/protocol_and_gates.rs::budget_caps_and_wrap_up_triggers_across_free_paid_and_local_tiers",
    "S38": "lucubra-core/tests/fts5.rs::word_tokenizer_keeps_indic_words_whole",
    "S41": "lucubra-companion/tests/security.rs::companion_rejects_without_token_host_or_origin_and_works_with_them",
    "S43": "lucubra-pack/src/build.rs::pack_phone_text_and_skill_front_matter",
    "S48": "lucubra-core/src/organise.rs::organise_proposal_moves_nothing_and_respects_shared_folders",
    "S57": "lucubra-ingest/src/native.rs::native_subtitle_vtt_and_srt_keep_timestamps",
    "S58": "tests/test_audit_headers.py::test_watermark_outside_source_fails",
}

# Scenarios not built yet, with the phase that builds them.
PENDING = {
    "S36": "M10 performance run",
    "S46": "round-4 sessions (study scripts)",
    "S47": "round-4 sessions (teaching styles)",
    "S49": "round-4 grades (term close)",
    "S50": "round-4 grades (target CGPA)",
    "S51": "round-4 practice (past papers)",
    "S52": "round-4 practice (fetch)",
    "S53": "round-4 practice (mock test)",
    "S54": "round-4 sessions (reminders off)",
    "S55": "round-4 sessions (review-day reminders)",
    "S56": "round-4 sessions (preferences file)",
}


def rust_files():
    return sorted(CRATES.glob("*/src/**/*.rs"))


def rel(p: Path) -> str:
    return p.relative_to(CRATES).as_posix()


def resolve(target: str) -> Path:
    base = ROOT if target.startswith("tests/") else CRATES
    return base / target


def test_every_crate_has_a_tests_folder():
    missing = [c.name for c in CRATES.iterdir() if c.is_dir() and not (c / "tests").is_dir()]
    assert not missing, missing


def test_every_source_file_has_a_suite():
    missing = []
    for path in rust_files():
        name = rel(path)
        if name in NO_LOGIC or "#[cfg(test)]" in path.read_text(encoding="utf-8"):
            continue
        if name.startswith("lucubra-mcp/src/tools/") and name != "lucubra-mcp/src/tools/mod.rs":
            continue  # checked by test_every_mcp_tool_has_a_suite
        suite = COVERED_BY.get(name)
        if suite is None or not (CRATES / suite).is_file():
            missing.append(name)
    assert not missing, f"add a #[cfg(test)] module or a COVERED_BY entry: {missing}"


def test_covered_by_entries_are_still_needed():
    stale = [
        name for name in COVERED_BY if "#[cfg(test)]" in (CRATES / name).read_text(encoding="utf-8")
    ]
    assert not stale, f"these now have unit tests; drop them from COVERED_BY: {stale}"


def test_every_mcp_tool_has_a_suite():
    tools = sorted(
        p.stem for p in (CRATES / "lucubra-mcp/src/tools").glob("*.rs") if p.stem != "mod"
    )
    assert len(tools) == 11, tools
    missing = [t for t in tools if not (CRATES / f"lucubra-mcp/tests/tool_{t}.rs").is_file()]
    assert not missing, missing


def test_every_worker_module_has_a_suite():
    sources = "\n".join(p.read_text(encoding="utf-8") for p in ROOT.glob("tests/test_workers*.py"))
    modules = [
        p.stem
        for p in (ROOT / "python/lucubra_workers").glob("*.py")
        if p.stem not in {"__init__", "__main__", "errors"}
    ]
    missing = [
        m
        for m in modules
        if not re.search(rf"lucubra_workers(\.{m}\b| import [\w, ]*\b{m}\b)", sources)
    ]
    assert not missing, missing


def scenario_functions() -> set[str]:
    found = set()
    pattern = re.compile(r"fn (s\d{2}(?:_s\d{2})*)_")
    for path in CRATES.glob("*/**/*.rs"):
        for match in pattern.finditer(path.read_text(encoding="utf-8")):
            found.update(s.upper() for s in match.group(1).split("_"))
    return found


def test_every_scenario_has_a_test_or_a_pending_phase():
    named = scenario_functions()
    missing = []
    for n in range(1, 60):
        sid = f"S{n:02d}"
        if sid in named or sid in PENDING:
            continue
        target = SCENARIO_TESTS.get(sid)
        if target is None:
            missing.append(sid)
            continue
        file, func = target.split("::")
        text = resolve(file).read_text(encoding="utf-8")
        if not re.search(rf"(fn|def) {func}\(", text):
            missing.append(f"{sid} ({target} not found)")
    assert not missing, missing


def test_pending_and_mapped_scenarios_are_not_already_named():
    named = scenario_functions()
    stale = sorted((set(PENDING) | set(SCENARIO_TESTS)) & named)
    assert not stale, f"these now have an sNN_ test; drop them from the maps: {stale}"
