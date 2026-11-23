# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
<#
.SYNOPSIS
Install Lucubra with uv on Windows.

.DESCRIPTION
Installs uv first when it is missing (official Astral installer, which may edit your
user PATH as documented by uv), then runs `uv tool install` for Lucubra. This script
itself never edits PATH. Safe to run again: a second run upgrades or reinstalls in place.

.PARAMETER Version
Pin a version in PEP 440 form, for example 1.0.0b1. Default: the latest release,
pre-releases included.

.PARAMETER WheelUrl
Install from a GitHub release wheel URL (or a file:// URL) instead of PyPI.

.EXAMPLE
.\install.ps1

.EXAMPLE
.\install.ps1 -WheelUrl https://github.com/uncoalesced/lucubra/releases/download/v1.0.0-beta.1/<wheel>.whl
#>
[CmdletBinding()]
param(
    [string]$Version,
    [string]$WheelUrl
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Invoke-Uv {
    & uv @args
    if ($LASTEXITCODE -ne 0) {
        throw "uv $($args -join ' ') failed with exit code $LASTEXITCODE"
    }
}

if (-not (Get-Command uv -ErrorAction SilentlyContinue)) {
    Write-Output 'uv not found; installing it with the official installer (https://astral.sh/uv).'
    $installer = Join-Path ([System.IO.Path]::GetTempPath()) 'uv-install.ps1'
    Invoke-WebRequest -UseBasicParsing -Uri 'https://astral.sh/uv/install.ps1' -OutFile $installer
    try {
        & powershell -NoProfile -ExecutionPolicy Bypass -File $installer
        if ($LASTEXITCODE -ne 0) { throw "uv installer failed with exit code $LASTEXITCODE" }
    }
    finally {
        Remove-Item -LiteralPath $installer -ErrorAction SilentlyContinue
    }
    # The uv installer puts uv here; make it visible to this process only.
    $uvBin = Join-Path $env:USERPROFILE '.local\bin'
    $env:Path = "$uvBin;$env:Path"
    if (-not (Get-Command uv -ErrorAction SilentlyContinue)) {
        throw 'uv was installed but is not on PATH; open a new terminal and run this script again.'
    }
}

if ($WheelUrl) {
    Write-Output "Installing Lucubra from $WheelUrl"
    Invoke-Uv tool install --force "lucubra @ $WheelUrl"
}
else {
    $spec = 'lucubra'
    if ($Version) { $spec = "lucubra==$Version" }
    Write-Output "Installing Lucubra from PyPI ($spec)"
    Invoke-Uv tool install --prerelease allow --upgrade $spec
}

if (Get-Command lucubra -ErrorAction SilentlyContinue) {
    & lucubra --version
}
else {
    $binDir = (& uv tool dir --bin)
    Write-Output "Installed, but $binDir is not on your PATH."
    Write-Output "Run 'uv tool update-shell' (uv edits your user PATH) or add that folder yourself, then open a new terminal."
}

Write-Output ''
Write-Output 'Next steps:'
Write-Output '  1. Run:  lucubra doctor'
Write-Output '  2. Connect your AI assistant: https://github.com/uncoalesced/lucubra/blob/main/docs/getting-started.md'
