# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

<#
.SYNOPSIS
Prepares a Windows Sandbox run of 3.0: the clean-install acceptance (M6-7),
an install next to 2.2.13 (M6-8) or synthetic pen and touch input (M6-9,
M6-10).

.DESCRIPTION
For the installer runs, packs the executable twice, as 3.0.0 to install and
as 3.0.1 in a local update feed; for the pointer run, copies the executable
and ugu-doc. Then writes a .wsb that maps them read-only into a Sandbox with
the vGPU disabled and runs the scenario's script there at logon:
accept.ps1 (no network; with a document to open and a feed whose package is
damaged), alongside.ps1 (with the network, which the 2.2.13 Setup given by
-Legacy needs for its Visual C++ runtime) or pointer.ps1 (no network). The
results come back in <Work>\out\results.md. Nothing on this PC is installed
or registered.

.EXAMPLE
tools/sandbox/prepare.ps1 -Work $env:TEMP\m6-7
Start-Process $env:TEMP\m6-7\sandbox.wsb

.EXAMPLE
tools/sandbox/prepare.ps1 -Work $env:TEMP\m6-8 -Scenario alongside -Legacy <2.2.13 Setup>
#>
param(
    [Parameter(Mandatory)] [string] $Work,
    [ValidateSet('accept', 'alongside', 'pointer')] [string] $Scenario = 'accept',
    [string] $Legacy,
    [string] $Executable = 'target/release/ugurugu.exe',
    [string] $UguDoc = 'target/release/ugu-doc.exe',
    [string] $Vpk = 'vpk'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)

if ($Scenario -eq 'alongside' -and -not $Legacy) {
    throw 'The alongside scenario needs -Legacy, the 2.2.13 Setup'
}
if (Test-Path $Work) {
    throw "$Work already exists"
}
$in = Join-Path $Work 'in'
$out = Join-Path $Work 'out'
New-Item -ItemType Directory -Path $in, $out | Out-Null
$Work = (Resolve-Path $Work).Path
$in = Join-Path $Work 'in'
$out = Join-Path $Work 'out'

if ($Scenario -ne 'pointer') {
    $package = Join-Path $root 'tools/package_windows.ps1'
    & $package -Executable $Executable -OutputDirectory (Join-Path $in 'install') -Vpk $Vpk
    & $package -Executable $Executable -OutputDirectory (Join-Path $in 'feed') -Vpk $Vpk `
        -Version 3.0.1 -ReleaseNotes (Join-Path $root 'release-notes/3.0.0.en.md')
}

if ($Scenario -eq 'pointer') {
    Copy-Item $Executable, $UguDoc $in
    $networking = 'Disable'
} elseif ($Scenario -eq 'accept') {
    & $UguDoc fixture 1 (Join-Path $in 'drawing.ugurugu')
    if ($LASTEXITCODE -ne 0) {
        throw "ugu-doc fixture failed with exit code $LASTEXITCODE"
    }
    # The same feed with one byte of the package changed, which fails its checksum.
    $broken = Join-Path $in 'feed-broken'
    Copy-Item -Recurse (Join-Path $in 'feed') $broken
    $nupkg = Get-ChildItem $broken -Filter '*.nupkg' | Select-Object -First 1
    $bytes = [IO.File]::ReadAllBytes($nupkg.FullName)
    $bytes[[int]($bytes.Length / 2)] = $bytes[[int]($bytes.Length / 2)] -bxor 0xFF
    [IO.File]::WriteAllBytes($nupkg.FullName, $bytes)
    $networking = 'Disable'
} else {
    New-Item -ItemType Directory -Path (Join-Path $in 'legacy') | Out-Null
    Copy-Item $Legacy (Join-Path $in 'legacy\Ugurugu-Windows-x64-Setup.exe')
    $networking = 'Enable'
}
Copy-Item (Join-Path $PSScriptRoot 'common.ps1'), (Join-Path $PSScriptRoot "$Scenario.ps1") $in

$wsb = @"
<Configuration>
  <VGpu>Disable</VGpu>
  <Networking>$networking</Networking>
  <MappedFolders>
    <MappedFolder>
      <HostFolder>$in</HostFolder>
      <SandboxFolder>C:\ugu\in</SandboxFolder>
      <ReadOnly>true</ReadOnly>
    </MappedFolder>
    <MappedFolder>
      <HostFolder>$out</HostFolder>
      <SandboxFolder>C:\ugu\out</SandboxFolder>
      <ReadOnly>false</ReadOnly>
    </MappedFolder>
  </MappedFolders>
  <LogonCommand>
    <Command>powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\ugu\in\$Scenario.ps1</Command>
  </LogonCommand>
</Configuration>
"@
Set-Content -Path (Join-Path $Work 'sandbox.wsb') -Value $wsb -Encoding utf8
Write-Host "Ready: start $(Join-Path $Work 'sandbox.wsb'); results in $(Join-Path $out 'results.md')"
