# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

<#
.SYNOPSIS
3.0 installed next to 2.2.13, run inside Windows Sandbox
(docs/rust/evidence.md section 32).

.DESCRIPTION
Started at logon by the .wsb that prepare.ps1 -Scenario alongside writes,
with C:\ugu\in (3.0 packages, feed, 2.2.13 Setup; read-only) and
C:\ugu\out (results) mapped. The Sandbox has the network, which the 2.2.13
Setup needs for its Visual C++ runtime. Each step checks that the other
version's install, data and association are as they were, and is recorded
in C:\ugu\out\results.md as it ends.
Runs in Windows PowerShell 5.1, which has the .NET UI Automation client.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$title = 'Windows Sandbox, next to 2.2.13'
$in = 'C:\ugu\in'
$out = 'C:\ugu\out'
$env:UGURUGU_LANGUAGE = 'en'
$installRoot = Join-Path $env:LOCALAPPDATA 'Ugurugu3'
$app = Join-Path $installRoot 'current\ugurugu.exe'
$data = Join-Path $env:LOCALAPPDATA 'Ugurugu Data\3'
$logs = Join-Path $data 'logs'
$recovery = Join-Path $data 'recovery'
$settings = Join-Path $env:APPDATA 'Ugurugu\3\settings.json'
$association = 'HKCU:\Software\Classes\.ugurugu'
$progId = 'Ugurugu.Document.3'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Ugurugu3'
$legacyRoot = Join-Path $env:LOCALAPPDATA 'Ugurugu'
$legacyApp = Join-Path $legacyRoot 'current\Ugurugu.exe'
$legacyData = Join-Path $legacyRoot 'Ugurugu'
$legacyKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Ugurugu'
$legacySettings = 'HKCU:\Software\Ugurugu\Ugurugu'
$legacyShortcuts = @(
    (Join-Path ([Environment]::GetFolderPath('Desktop')) 'Ugurugu.lnk'),
    (Join-Path ([Environment]::GetFolderPath('Programs')) 'Ugurugu.lnk')
)
$shortcuts = @(
    (Join-Path ([Environment]::GetFolderPath('Desktop')) 'Ugurugu 3.lnk'),
    (Join-Path ([Environment]::GetFolderPath('Programs')) 'Ugurugu 3.lnk')
)
$results = New-Object System.Collections.ArrayList

. (Join-Path $PSScriptRoot 'common.ps1')

function Stop-Apps {
    @(Get-AppProcess) + @(Get-AppProcess $legacyRoot) | ForEach-Object { Stop-Process -Id $_.Id -Force }
}

# Hashes of the files an install owns, by path under its root.
function Get-Installed([string] $Root) {
    $owned = @(Get-ChildItem -File $Root) + @(Get-ChildItem -Recurse -File (Join-Path $Root 'current'))
    $map = @{}
    foreach ($file in $owned) {
        $map[$file.FullName.Substring($Root.Length)] = (Get-FileHash $file.FullName -Algorithm SHA256).Hash
    }
    $map
}

function Assert-Same($Before, $After, [string] $What) {
    $changed = @($Before.Keys + $After.Keys | Sort-Object -Unique |
        Where-Object { $Before[$_] -ne $After[$_] })
    if ($changed) {
        throw "$What changed: $($changed -join ', ')"
    }
    "$What unchanged ($($Before.Count) files)"
}

function Count-Files([string] $Folder) {
    if (-not (Test-Path $Folder)) {
        return 0
    }
    @(Get-ChildItem -Recurse -File $Folder).Count
}

function Install([string] $Setup, [string] $Root) {
    $installer = Start-Process -FilePath $Setup -ArgumentList '--silent' -PassThru -Wait
    if ($installer.ExitCode -ne 0) {
        throw "Setup exited with code $($installer.ExitCode)"
    }
    # Setup may start the app once installed.
    Start-Sleep -Seconds 5
    Get-AppProcess $Root | ForEach-Object { Close-App $_ }
}

function Uninstall([string] $Root) {
    $uninstaller = Start-Process -FilePath (Join-Path $Root 'Update.exe') `
        -ArgumentList @('uninstall', '--silent') -PassThru -Wait
    if ($uninstaller.ExitCode -ne 0) {
        throw "Uninstall exited with code $($uninstaller.ExitCode)"
    }
}

function Start-Legacy {
    Start-Process -FilePath $legacyApp | Out-Null
    $process = Wait-Window 90 $legacyRoot
    $caption = $process.MainWindowTitle
    Close-App $process
    "2.2.13 ran ('$caption')"
}

function Start-Current {
    Start-Process -FilePath $app | Out-Null
    $process = Wait-Window
    $caption = $process.MainWindowTitle
    Close-App $process
    "3.0 ran ('$caption')"
}

function Get-Ours {
    [pscustomobject]@{
        Recovery = Count-Files $recovery
        Logs = Count-Files $logs
        Settings = Test-Path $settings
    }
}

function Assert-Ours($Before, [string] $When) {
    $now = Get-Ours
    $detail = "3.0 data $When`: recovery $($now.Recovery) files (was $($Before.Recovery)), logs $($now.Logs) (was $($Before.Logs)), settings $($now.Settings)"
    if ($now.Recovery -lt $Before.Recovery -or $now.Logs -lt $Before.Logs -or ($Before.Settings -and -not $now.Settings)) {
        throw "Lost $detail"
    }
    $detail
}

$legacySetup = Join-Path $in 'legacy\Ugurugu-Windows-x64-Setup.exe'

Step 'Install 2.2.13' {
    Install $legacySetup $legacyRoot
    $version = Get-InstalledVersion $legacyRoot
    $missing = @($legacyShortcuts | Where-Object { -not (Test-Path $_) })
    if ($missing) {
        throw "Missing shortcuts: $($missing -join ', ')"
    }
    $name = (Get-Item $legacyKey).GetValue('DisplayName')
    Start-Legacy
    "version $version in $legacyRoot", "Apps entry '$name'", "2.2.13 settings in the registry: $(Test-Path $legacySettings)", "2.2.13 data: $(Count-Files $legacyData) files"
}

$legacy = Get-Installed $legacyRoot

Step 'Install 3.0.0 next to 2.2.13' {
    Install (Join-Path $in 'install\Ugurugu-Windows-x64-Setup.exe') $installRoot
    $version = Get-InstalledVersion
    if ($version -ne '3.0.0') {
        throw "Installed version $version"
    }
    $missing = @($shortcuts + $legacyShortcuts | Where-Object { -not (Test-Path $_) })
    if ($missing) {
        throw "Missing shortcuts: $($missing -join ', ')"
    }
    if (-not (Test-Path $legacyKey) -or -not (Test-Path $uninstallKey)) {
        throw 'Both Apps entries should be there'
    }
    if ((Get-DefaultValue $association) -ne $progId) {
        throw '.ugurugu is not associated with 3.0'
    }
    Assert-Same $legacy (Get-Installed $legacyRoot) '2.2.13 install'
    "3.0.0 in $installRoot", 'both Apps entries and all four shortcuts', ".ugu association: '$(Get-DefaultValue 'HKCU:\Software\Classes\.ugu')'"
    Start-Current
    Start-Legacy
}

Step 'Update 3.0 to 3.0.1' {
    $env:UGURUGU_UPDATE_FEED = Join-Path $in 'feed'
    try {
        Start-Process -FilePath $app | Out-Null
        $process = Wait-Window
        # The automatic check runs two seconds after start.
        Press $process 'Update' 60
        if (-not $process.WaitForExit(60000)) {
            throw 'The app did not close to update'
        }
    } finally {
        Remove-Item Env:UGURUGU_UPDATE_FEED
    }
    $restarted = Wait-Window 120
    $version = Get-InstalledVersion
    Close-App $restarted
    if ($version -ne '3.0.1') {
        throw "Installed version after the update: $version"
    }
    "updated to $version", (Assert-Same $legacy (Get-Installed $legacyRoot) '2.2.13 install')
}

Step 'Leave 3.0 work for recovery' {
    Start-Process -FilePath $app | Out-Null
    $process = Wait-Window
    Press $process 'Add layer'
    $written = Wait-Until { Log-Lines 'work written for recovery.*ok=true' | Select-Object -Last 1 } 30 'the recovery write'
    Stop-Process -Id $process.Id -Force
    $process.WaitForExit()
    $ours = Get-Ours
    if ($ours.Recovery -eq 0) {
        throw "No recovery files in $recovery"
    }
    $written, "recovery $($ours.Recovery) files, logs $($ours.Logs), settings $($ours.Settings)"
}

$ours = Get-Ours

Step 'Run 2.2.13 while 3.0 has work to recover' {
    Start-Legacy
    Assert-Ours $ours 'after 2.2.13 ran'
}

Step 'Uninstall 2.2.13' {
    Uninstall $legacyRoot
    $left = @()
    if (Test-Path $legacyApp) { $left += 'the 2.2.13 app' }
    $left += @($legacyShortcuts | Where-Object { Test-Path $_ })
    if (Test-Path $legacyKey) { $left += 'the 2.2.13 Apps entry' }
    $kept = @(Get-ChildItem -Force $legacyRoot -ErrorAction SilentlyContinue | ForEach-Object Name) -join ', '
    $kept = "left in $legacyRoot`: $(if ($kept) { $kept } else { 'nothing' })"
    if ($left) {
        throw "Left after uninstalling: $($left -join ', '); $kept"
    }
    if (-not (Test-Path $app) -or (Get-DefaultValue $association) -ne $progId) {
        throw "3.0 install or association went with 2.2.13; $kept"
    }
    try {
        Assert-Ours $ours 'after 2.2.13 was uninstalled'
    } catch {
        throw "$($_.Exception.Message); $kept"
    }
    $kept
}

Step 'Install 2.2.13 again with 3.0 data there' {
    $before = Get-Ours
    Install $legacySetup $legacyRoot
    Start-Legacy
    Assert-Ours $before 'after 2.2.13 was installed'
}

$legacy = Get-Installed $legacyRoot

Step 'Uninstall 3.0 next to 2.2.13' {
    Uninstall $installRoot
    if ((Test-Path $app) -or (Test-Path $uninstallKey) -or (Test-Path $association)) {
        throw '3.0 install, Apps entry or association left'
    }
    $missing = @($legacyShortcuts | Where-Object { -not (Test-Path $_) })
    if ($missing -or -not (Test-Path $legacyKey)) {
        throw "2.2.13 lost shortcuts or its Apps entry: $($missing -join ', ')"
    }
    (Assert-Same $legacy (Get-Installed $legacyRoot) '2.2.13 install'), (Start-Legacy)
}

if (Test-Path $logs) {
    Copy-Item -Recurse $logs (Join-Path $out 'logs')
}
if (Test-Path $legacyData) {
    Copy-Item -Recurse $legacyData (Join-Path $out 'legacy-data')
}
Write-Host "Done: $(Join-Path $out 'results.md')"
