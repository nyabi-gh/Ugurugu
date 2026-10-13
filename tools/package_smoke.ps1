# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

<#
.SYNOPSIS
Installs the 3.0 setup, runs the installed app, closes it and uninstalls.

.DESCRIPTION
For CI and clean machines: it writes the current user's .ugurugu
association, Start menu and desktop shortcuts and an Apps entry, and removes
them again. The run uses its own settings, recovery and log folders under
WorkDirectory and does not check for updates.

.EXAMPLE
tools/package_smoke.ps1 -Setup artifacts/Ugurugu-3.0.0-Windows-x64-Setup.exe -WorkDirectory $env:RUNNER_TEMP/smoke
#>
param(
    [Parameter(Mandatory)] [string] $Setup,
    [Parameter(Mandatory)] [string] $WorkDirectory,
    [ValidateRange(5, 300)] [int] $StartSeconds = 90
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$association = 'HKCU:\Software\Classes\.ugurugu'
$progId = 'Ugurugu.Document.3'

function Get-DefaultValue([string] $Key) {
    if (-not (Test-Path $Key)) {
        return $null
    }
    (Get-Item $Key).GetValue('')
}

function Get-InstalledProcess([string] $Root) {
    @(Get-Process ugurugu -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -and $_.Path.StartsWith($Root, [StringComparison]::OrdinalIgnoreCase) })
}

if (Test-Path $WorkDirectory) {
    throw "$WorkDirectory already exists"
}
New-Item -ItemType Directory -Path $WorkDirectory | Out-Null
$WorkDirectory = (Resolve-Path $WorkDirectory).Path
$root = Join-Path $WorkDirectory 'install'
$logs = Join-Path $WorkDirectory 'logs'
if (Get-DefaultValue $association) {
    throw "$association is already set; the smoke test needs a user without it"
}

Write-Host 'Installing'
$installer = Start-Process -FilePath $Setup `
    -ArgumentList @('--silent', '--installto', "`"$root`"", '--log', "`"$WorkDirectory\setup.log`"") `
    -PassThru -Wait
if ($installer.ExitCode -ne 0) {
    throw "Setup exited with code $($installer.ExitCode)"
}
# Setup may start the app once installed; that run is not isolated, so it
# is closed before the isolated one.
Start-Sleep -Seconds 3
foreach ($process in Get-InstalledProcess $root) {
    Write-Host "Closing the app Setup started (process $($process.Id))"
    [void]$process.CloseMainWindow()
    if (-not $process.WaitForExit(30000)) {
        throw 'The app Setup started did not close'
    }
}

$current = Join-Path $root 'current'
$app = Join-Path $current 'ugurugu.exe'
if (-not (Test-Path (Join-Path $root 'Update.exe'))) {
    throw 'Update.exe is missing'
}
$installed = @(Get-ChildItem -Recurse -File $current | ForEach-Object Name | Sort-Object)
$expected = @('LICENSE.txt', 'sq.version', 'THIRD_PARTY_NOTICES.txt', 'ugurugu.exe')
if (Compare-Object $expected $installed) {
    throw "Installed files differ from the expected list: $($installed -join ', ')"
}

if ((Get-DefaultValue $association) -ne $progId) {
    throw "The install hook did not associate .ugurugu"
}
$command = Get-DefaultValue "HKCU:\Software\Classes\$progId\shell\open\command"
if ($command -ne "`"$app`" `"%1`"") {
    throw "Unexpected open command: $command"
}

Write-Host 'Running'
$settings = Join-Path $WorkDirectory 'settings.json'
Set-Content -Path $settings -Encoding utf8 `
    -Value '{"format": "ugurugu.settings", "version": 1, "checkForUpdates": false}'
$env:UGURUGU_SETTINGS_PATH = $settings
$env:UGURUGU_RECOVERY_PATH = Join-Path $WorkDirectory 'recovery'
$env:UGURUGU_LOG_PATH = $logs
$process = Start-Process -FilePath $app -PassThru
$deadline = (Get-Date).AddSeconds($StartSeconds)
$shown = $false
while (-not $shown -and (Get-Date) -lt $deadline) {
    if ($process.HasExited) {
        throw "The app exited with code $($process.ExitCode) while starting"
    }
    Start-Sleep -Milliseconds 500
    $process.Refresh()
    $log = Get-ChildItem $logs -Filter 'ugurugu-*.log' -ErrorAction SilentlyContinue |
        Select-String -SimpleMatch 'selected GPU adapter' -List
    $shown = $log -and $process.MainWindowHandle -ne 0
}
if (-not $shown) {
    [void]$process.CloseMainWindow()
    throw "The app showed no window within $StartSeconds seconds"
}
[void]$process.CloseMainWindow()
if (-not $process.WaitForExit(30000)) {
    Stop-Process -Id $process.Id -Force
    throw 'The app did not close'
}
if ($process.ExitCode -ne 0) {
    throw "The app exited with code $($process.ExitCode)"
}
Get-ChildItem $logs -Filter 'ugurugu-*.log' | Get-Content | Write-Host
$problems = @(Get-ChildItem $logs -Filter 'crash-*' -ErrorAction SilentlyContinue)
$problems += @(Get-ChildItem $logs -Filter 'ugurugu-*.log' | Select-String -Pattern '\bERROR\b')
if ($problems) {
    throw "The run left crash records or errors: $($problems -join '; ')"
}

Write-Host 'Uninstalling'
$uninstaller = Start-Process -FilePath (Join-Path $root 'Update.exe') `
    -ArgumentList @('uninstall', '--silent') -PassThru -Wait
if ($uninstaller.ExitCode -ne 0) {
    throw "Uninstall exited with code $($uninstaller.ExitCode)"
}
if (Test-Path $app) {
    throw 'The app is still installed'
}
if ((Test-Path $association) -or (Test-Path "HKCU:\Software\Classes\$progId")) {
    throw 'The uninstall hook left the .ugurugu association'
}
Write-Host 'Install, run and uninstall passed'
