# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""Smoke test for an installed Lucubra (M9 clean-install gate).

Runs the installed `lucubra` against a fresh workspace: version, init, doctor,
then an MCP stdio session that lists the tools and checks that the embedded
domain packs load (a programme of "MBBS" selects the medicine pack). Set
LUCUBRA_DATA_DIR to keep app state where you want it; by default it goes to a
temporary folder next to the workspace.

    python scripts/smoke_install.py [--exe PATH] [--expect-version X.Y.Z]
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

EXPECTED_TOOLS = 11


def run(cmd: list[str], env: dict[str, str]) -> str:
    out = subprocess.run(cmd, capture_output=True, text=True, env=env, timeout=120)
    if out.returncode != 0:
        raise SystemExit(f"{' '.join(cmd)} failed ({out.returncode}): {out.stderr.strip()}")
    return out.stdout.strip()


class Mcp:
    def __init__(self, exe: str, workspace: Path, env: dict[str, str]) -> None:
        self.proc = subprocess.Popen(
            [exe, "mcp", "-w", str(workspace)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            env=env,
        )
        self.next_id = 0

    def send(self, method: str, params: dict | None = None, notify: bool = False) -> dict | None:
        msg: dict = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            msg["params"] = params
        if not notify:
            self.next_id += 1
            msg["id"] = self.next_id
        assert self.proc.stdin and self.proc.stdout
        self.proc.stdin.write(json.dumps(msg) + "\n")
        self.proc.stdin.flush()
        if notify:
            return None
        while True:
            line = self.proc.stdout.readline()
            if not line:
                raise SystemExit(f"MCP server closed before answering {method}")
            reply = json.loads(line)
            if reply.get("id") == self.next_id:
                if "error" in reply:
                    raise SystemExit(f"{method} failed: {reply['error']}")
                return reply["result"]

    def tool(self, name: str, args: dict) -> dict:
        result = self.send("tools/call", {"name": name, "arguments": args})
        assert result is not None
        envelope = json.loads(result["content"][0]["text"])
        if not envelope.get("ok", True) or result.get("isError"):
            raise SystemExit(f"{name} returned an error: {envelope}")
        return envelope

    def close(self) -> None:
        if self.proc.stdin:
            self.proc.stdin.close()
        try:
            self.proc.wait(timeout=20)
        except subprocess.TimeoutExpired:
            self.proc.kill()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--exe", default="lucubra", help="lucubra executable (default: on PATH)")
    parser.add_argument("--expect-version", help="fail unless --version prints this version")
    args = parser.parse_args()
    exe = shutil.which(args.exe) or args.exe

    base = Path(tempfile.mkdtemp(prefix="lucubra-smoke-"))
    env = dict(os.environ)
    env.setdefault("LUCUBRA_DATA_DIR", str(base / "appdata"))
    workspace = base / "workspace"
    workspace.mkdir()
    try:
        version = run([exe, "--version"], env)
        print(version)
        if args.expect_version and version != f"lucubra {args.expect_version}":
            raise SystemExit(f"expected lucubra {args.expect_version}, got {version!r}")
        run([exe, "init", "-w", str(workspace)], env)
        print("init ok")
        print(run([exe, "doctor", "-w", str(workspace)], env).splitlines()[0])

        mcp = Mcp(exe, workspace, env)
        try:
            mcp.send(
                "initialize",
                {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "lucubra-smoke", "version": "1"},
                },
            )
            mcp.send("notifications/initialized", notify=True)
            listed = mcp.send("tools/list", {})
            assert listed is not None
            names = sorted(t["name"] for t in listed["tools"])
            if len(names) != EXPECTED_TOOLS:
                raise SystemExit(f"expected {EXPECTED_TOOLS} tools, got {len(names)}: {names}")
            print(f"tools: {len(names)}")
            mcp.tool("lucubra_profile", {"action": "answer", "field": "programme", "value": "MBBS"})
            status = mcp.tool("lucubra_status", {})
            pack = (status["data"].get("domain_pack") or {}).get("id")
            if pack != "medicine":
                raise SystemExit(f"domain packs did not load: domain_pack is {pack!r}")
            print("domain packs: ok (medicine)")
        finally:
            mcp.close()
    finally:
        shutil.rmtree(base, ignore_errors=True)
    print("smoke install ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
