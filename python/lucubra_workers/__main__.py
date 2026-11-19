# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""Entry point for `python -m lucubra_workers`."""

import sys

from lucubra_workers import serve
from lucubra_workers.limits import apply_memory_limit

if __name__ == "__main__":
    if hasattr(sys.stdin, "reconfigure"):
        sys.stdin.reconfigure(encoding="utf-8")
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")
    apply_memory_limit()
    serve(sys.stdin, sys.stdout)
