# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

<#
.SYNOPSIS
Stages the 3.0 executable with its licenses and packs it with Velopack.

.DESCRIPTION
The package ID, channel and title differ from 2.x (Ugurugu, win, "Ugurugu"),
so 3.0 installs beside 2.2.13 in its own folder, update feed and shortcuts
(docs/rust/decisions.md, m6 2절 4). The staged files must be exactly
the expected ones, so nothing else reaches the installer unnoticed.

.EXAMPLE
tools/package_windows.ps1 -Executable target/release/ugurugu.exe -OutputDirectory artifacts

.EXAMPLE
tools/package_windows.ps1 -Executable target/release/ugurugu.exe -OutputDirectory feed -Version 3.0.1 -ReleaseNotes release-notes/3.0.0.en.md

A later version of the same build, for trying an update from a local feed
(tools/sandbox). Releases take the version and notes from the repository.
#>
param(
    [Parameter(Mandatory)] [string] $Executable,
    [Parameter(Mandatory)] [string] $OutputDirectory,
    [string] $Vpk = 'vpk',
    [string] $Version,
    [string] $ReleaseNotes
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

$manifest = Get-Content (Join-Path $root 'Cargo.toml') -Raw
if ($manifest -notmatch '(?ms)^\[workspace\.package\][^\[]*?^version = "([^"]+)"') {
    throw 'Could not read the workspace version from Cargo.toml'
}
$version = if ($Version) { $Version } else { $Matches[1] }
$notes = if ($ReleaseNotes) { $ReleaseNotes } else { Join-Path $root "release-notes/$version.en.md" }
if (-not (Test-Path $notes)) {
    throw "Missing release notes: $notes"
}

$staging = Join-Path ([IO.Path]::GetTempPath()) "ugurugu-package-$PID"
if (Test-Path $staging) {
    Remove-Item -Recurse -Force $staging
}
New-Item -ItemType Directory -Path $staging | Out-Null
try {
    Copy-Item $Executable (Join-Path $staging 'ugurugu.exe')
    Copy-Item (Join-Path $root 'LICENSE') (Join-Path $staging 'LICENSE.txt')
    python -X utf8 (Join-Path $root 'tools/third_party_notices.py') (Join-Path $staging 'THIRD_PARTY_NOTICES.txt')
    if ($LASTEXITCODE -ne 0) {
        throw "Third-party notices failed with exit code $LASTEXITCODE"
    }

    $expected = @('LICENSE.txt', 'THIRD_PARTY_NOTICES.txt', 'ugurugu.exe')
    $staged = @(Get-ChildItem -Recurse -File $staging | ForEach-Object Name | Sort-Object)
    if (Compare-Object $expected $staged) {
        throw "Staged files differ from the expected list: $($staged -join ', ')"
    }

    New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
    & $Vpk pack `
        --packId Ugurugu3 `
        --packVersion $version `
        --packDir $staging `
        --mainExe ugurugu.exe `
        --packTitle 'Ugurugu 3' `
        --packAuthors nyabi-gh `
        --icon (Join-Path $root 'resources/icons/Ugurugu.ico') `
        --releaseNotes $notes `
        --runtime win10-x64 `
        --channel win-x64 `
        --delta None `
        --noPortable true `
        --outputDir $OutputDirectory
    if ($LASTEXITCODE -ne 0) {
        throw "vpk pack failed with exit code $LASTEXITCODE"
    }
    # Named with its version, as the release page shows it.
    Move-Item -Force `
        (Join-Path $OutputDirectory 'Ugurugu3-win-x64-Setup.exe') `
        (Join-Path $OutputDirectory "Ugurugu-$version-Windows-x64-Setup.exe")
} finally {
    Remove-Item -Recurse -Force $staging
}
