# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""Tests for the worker line protocol."""

import io
import json

import lucubra_workers


def run(lines: str) -> list[dict]:
    out = io.StringIO()
    lucubra_workers.serve(io.StringIO(lines), out)
    return [json.loads(line) for line in out.getvalue().splitlines()]


def test_ping_returns_ok_with_request_id():
    assert run('{"id": "r1", "op": "ping"}\n') == [
        {"id": "r1", "ok": True, "worker": lucubra_workers.__version__}
    ]


def test_unknown_op_is_reported_and_does_not_stop_the_loop():
    responses = run('{"id": "r1", "op": "nope"}\n\n{"id": "r2", "op": "ping"}\n')
    assert responses[0]["ok"] is False
    assert responses[0]["error"]["code"] == "unknown_op"
    assert responses[1] == {"id": "r2", "ok": True, "worker": lucubra_workers.__version__}


def test_invalid_json_and_non_object_are_bad_requests():
    responses = run("not json\n[1, 2]\n")
    assert [r["error"]["code"] for r in responses] == ["bad_request", "bad_request"]
    assert all(r["id"] is None for r in responses)
