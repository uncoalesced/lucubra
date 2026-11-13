# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""Entry point for `python -m lucubra_workers`."""

import sys

from lucubra_workers import serve

if __name__ == "__main__":
    serve(sys.stdin, sys.stdout)
