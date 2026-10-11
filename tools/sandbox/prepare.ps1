# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

<#
.SYNOPSIS
Prepares the clean-install acceptance run of 3.0 in Windows Sandbox (M6-7).

.DESCRIPTION
Packs the executable twice, as 3.0.0 to install and as 3.0.1 in a local
update feed, makes a document to open, and writes a .wsb that maps them
read-only into a Sandbox with no network and no GPU (so the app draws with
WARP) and runs accept.ps1 there at logon. The results come back in
<Work>\out\results.md. Nothing on this PC is installed or registered.

.EXAMPLE
tools/sandbox/prepare.ps1 -Work $env:TEMP\m6-7
Start-Process $env:TEMP\m6-7\m6-7.wsb
#>
param(
    [Parameter(Mandatory)] [string] $Work,
    [string] $Executable = 'target/release/ugurugu.exe',
    [string] $UguDoc = 'target/release/ugu-doc.exe',
    [string] $Vpk = 'vpk'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)

if (Test-Path $Work) {
    throw "$Work already exists"
}
$in = Join-Path $Work 'in'
$out = Join-Path $Work 'out'
New-Item -ItemType Directory -Path $in, $out | Out-Null
$Work = (Resolve-Path $Work).Path
$in = Join-Path $Work 'in'
$out = Join-Path $Work 'out'

$package = Join-Path $root 'tools/package_windows.ps1'
& $package -Executable $Executable -OutputDirectory (Join-Path $in 'install') -Vpk $Vpk
& $package -Executable $Executable -OutputDirectory (Join-Path $in 'feed') -Vpk $Vpk `
    -Version 3.0.1 -ReleaseNotes (Join-Path $root 'release-notes/3.0.0.en.md')

& $UguDoc fixture 1 (Join-Path $in 'drawing.ugurugu')
if ($LASTEXITCODE -ne 0) {
    throw "ugu-doc fixture failed with exit code $LASTEXITCODE"
}
Copy-Item (Join-Path $PSScriptRoot 'accept.ps1') $in

$wsb = @"
<Configuration>
  <VGpu>Disable</VGpu>
  <Networking>Disable</Networking>
  <MappedFolders>
    <MappedFolder>
      <HostFolder>$in</HostFolder>
      <SandboxFolder>C:\m6-7\in</SandboxFolder>
      <ReadOnly>true</ReadOnly>
    </MappedFolder>
    <MappedFolder>
      <HostFolder>$out</HostFolder>
      <SandboxFolder>C:\m6-7\out</SandboxFolder>
      <ReadOnly>false</ReadOnly>
    </MappedFolder>
  </MappedFolders>
  <LogonCommand>
    <Command>powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\m6-7\in\accept.ps1</Command>
  </LogonCommand>
</Configuration>
"@
Set-Content -Path (Join-Path $Work 'm6-7.wsb') -Value $wsb -Encoding utf8
Write-Host "Ready: start $(Join-Path $Work 'm6-7.wsb'); results in $(Join-Path $out 'results.md')"
