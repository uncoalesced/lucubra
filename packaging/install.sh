#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
#
# Install Lucubra with uv on macOS and Linux.
#
#   sh install.sh                          install the latest pre-release from PyPI
#   sh install.sh --version 1.0.0b1        pin a version (PEP 440)
#   sh install.sh --wheel-url URL          install from a GitHub release wheel instead of PyPI
#
# Installs uv first when it is missing (official Astral installer, which may edit
# your shell profile as documented by uv). This script itself never edits PATH.
# Safe to run again: a second run upgrades or reinstalls in place.
set -eu

wheel_url=""
version=""

usage() {
    sed -n '5,9p' "$0" | sed 's/^# \{0,1\}//'
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --wheel-url)
            [ "$#" -ge 2 ] || { echo "error: --wheel-url needs a value" >&2; exit 2; }
            wheel_url="$2"
            shift 2
            ;;
        --version)
            [ "$#" -ge 2 ] || { echo "error: --version needs a value" >&2; exit 2; }
            version="$2"
            shift 2
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            echo "error: unknown option: $1" >&2
            exit 2
            ;;
    esac
done

if ! command -v uv >/dev/null 2>&1; then
    echo "uv not found; installing it with the official installer (https://astral.sh/uv)."
    if command -v curl >/dev/null 2>&1; then
        curl -LsSf https://astral.sh/uv/install.sh | sh
    elif command -v wget >/dev/null 2>&1; then
        wget -qO- https://astral.sh/uv/install.sh | sh
    else
        echo "error: need curl or wget to install uv" >&2
        exit 1
    fi
    # The uv installer puts uv here; make it visible to this process only.
    PATH="${XDG_BIN_HOME:-$HOME/.local/bin}:$HOME/.cargo/bin:$PATH"
    export PATH
    command -v uv >/dev/null 2>&1 || {
        echo "error: uv was installed but is not on PATH; open a new shell and run this script again" >&2
        exit 1
    }
fi

if [ -n "$wheel_url" ]; then
    echo "Installing Lucubra from $wheel_url"
    uv tool install --force "lucubra @ $wheel_url"
else
    spec="lucubra"
    [ -z "$version" ] || spec="lucubra==$version"
    echo "Installing Lucubra from PyPI ($spec)"
    uv tool install --prerelease allow --upgrade "$spec"
fi

if command -v lucubra >/dev/null 2>&1; then
    lucubra --version
else
    bin_dir="$(uv tool dir --bin)"
    echo "Installed, but $bin_dir is not on your PATH."
    echo "Run 'uv tool update-shell' (uv edits your shell profile) or add that folder to PATH yourself,"
    echo "then open a new shell."
fi

cat <<'MSG'

Next steps:
  1. Run:  lucubra doctor
  2. Connect your AI assistant: https://github.com/uncoalesced/lucubra/blob/main/docs/getting-started.md
MSG
