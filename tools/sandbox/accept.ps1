# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

<#
.SYNOPSIS
The clean-install acceptance of 3.0, run inside Windows Sandbox (M6-7).

.DESCRIPTION
Started at logon by the .wsb that prepare.ps1 writes, with C:\m6-7\in
(packages, feed, document; read-only) and C:\m6-7\out (results) mapped.
The Sandbox has no network. Each step is recorded in
C:\m6-7\out\results.md as it ends, and the app's logs are copied there.
Dialog buttons are pressed through UI Automation by their English names.
Runs in Windows PowerShell 5.1, which has the .NET UI Automation client.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes

$in = 'C:\m6-7\in'
$out = 'C:\m6-7\out'
$env:UGURUGU_LANGUAGE = 'en'
$installRoot = Join-Path $env:LOCALAPPDATA 'Ugurugu3'
$app = Join-Path $installRoot 'current\ugurugu.exe'
$logs = Join-Path $env:LOCALAPPDATA 'Ugurugu\3\logs'
$association = 'HKCU:\Software\Classes\.ugurugu'
$progId = 'Ugurugu.Document.3'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Ugurugu3'
$shortcuts = @(
    (Join-Path ([Environment]::GetFolderPath('Desktop')) 'Ugurugu 3.lnk'),
    (Join-Path ([Environment]::GetFolderPath('Programs')) 'Ugurugu 3.lnk')
)
$results = New-Object System.Collections.ArrayList

function Text([int[]] $codes) {
    -join ($codes | ForEach-Object { [char] $_ })
}

function Save-Results {
    $lines = @('# M6-7 Windows Sandbox', '', "$(Get-Date -Format s), $([Environment]::OSVersion.VersionString)", '',
        '| Step | Result | Detail |', '|---|---|---|')
    foreach ($row in $results) {
        $lines += "| $($row.Step) | $($row.Result) | $($row.Detail -replace '\|', '/') |"
    }
    [IO.File]::WriteAllLines((Join-Path $out 'results.md'), $lines, (New-Object Text.UTF8Encoding $false))
}

function Step([string] $Name, [scriptblock] $Body) {
    Write-Host "== $Name"
    try {
        $detail = @(& $Body) -join '; '
        [void]$results.Add([pscustomobject]@{ Step = $Name; Result = 'pass'; Detail = $detail })
    } catch {
        [void]$results.Add([pscustomobject]@{ Step = $Name; Result = 'FAIL'; Detail = $_.Exception.Message })
        Get-AppProcess | ForEach-Object { Stop-Process -Id $_.Id -Force }
    }
    Save-Results
}

function Get-DefaultValue([string] $Key) {
    if (-not (Test-Path $Key)) {
        return $null
    }
    (Get-Item $Key).GetValue('')
}

function Get-AppProcess {
    @(Get-Process ugurugu -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -and $_.Path.StartsWith($installRoot, [StringComparison]::OrdinalIgnoreCase) })
}

function Get-InstalledVersion {
    $nuspec = [xml](Get-Content (Join-Path $installRoot 'current\sq.version'))
    $nuspec.package.metadata.version
}

function Wait-Until([scriptblock] $Condition, [int] $Seconds, [string] $What) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        $value = & $Condition
        if ($value) {
            return $value
        }
        Start-Sleep -Milliseconds 500
    }
    throw "Timed out after $Seconds s waiting for $What"
}

function Wait-Window([int] $Seconds = 90) {
    Wait-Until {
        $process = Get-AppProcess | Select-Object -First 1
        if ($process -and $process.MainWindowHandle -ne 0) { $process }
    } $Seconds 'the app window'
}

# egui builds its UI Automation tree once asked, so elements are looked for
# again until they appear.
function Find-Element($Process, [scriptblock] $Match, [int] $Seconds = 30, [string] $What) {
    Wait-Until {
        $Process.Refresh()
        $window = [Windows.Automation.AutomationElement]::FromHandle($Process.MainWindowHandle)
        $all = $window.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition)
        foreach ($element in $all) {
            # Elements without a label have a null name.
            if (& $Match ([string] $element.Current.Name)) {
                return $element
            }
        }
    } $Seconds $What
}

function Press($Process, [string] $Name, [int] $Seconds = 30) {
    $element = Find-Element $Process { param($n) $n -eq $Name } $Seconds "'$Name'"
    $element.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
    Start-Sleep -Milliseconds 500
}

function Close-App($Process) {
    [void]$Process.CloseMainWindow()
    if (-not $Process.WaitForExit(30000)) {
        throw 'The app did not close'
    }
}

function Log-Lines([string] $Pattern) {
    @(Get-ChildItem $logs -Filter 'ugurugu-*.log' -ErrorAction SilentlyContinue |
        Select-String -Pattern $Pattern | ForEach-Object { $_.Line.Trim() })
}

Step 'Install 3.0.0' {
    $setup = Join-Path $in 'install\Ugurugu-Windows-x64-Setup.exe'
    $installer = Start-Process -FilePath $setup -ArgumentList '--silent' -PassThru -Wait
    if ($installer.ExitCode -ne 0) {
        throw "Setup exited with code $($installer.ExitCode)"
    }
    # Setup may start the app once installed.
    Start-Sleep -Seconds 5
    Get-AppProcess | ForEach-Object { Close-App $_ }
    $version = Get-InstalledVersion
    if ($version -ne '3.0.0') {
        throw "Installed version $version"
    }
    $files = @(Get-ChildItem -Recurse -File (Join-Path $installRoot 'current') | ForEach-Object Name | Sort-Object)
    $missing = @($shortcuts | Where-Object { -not (Test-Path $_) })
    if ($missing) {
        throw "Missing shortcuts: $($missing -join ', ')"
    }
    if ((Get-DefaultValue $association) -ne $progId) {
        throw '.ugurugu is not associated'
    }
    $name = (Get-Item $uninstallKey).GetValue('DisplayName')
    "version $version in $installRoot", "files: $($files -join ', ')", "Apps entry '$name'", 'shortcuts on the desktop and in Start'
}

function Start-AndClose {
    $before = @(Log-Lines 'selected GPU adapter').Count
    $started = Start-Process -FilePath $app -PassThru
    # Kept, so the exit code can be read once it ends.
    [void]$started.Handle
    $process = Wait-Window
    $adapter = Wait-Until { $lines = @(Log-Lines 'selected GPU adapter'); if ($lines.Count -gt $before) { $lines[-1] } } 60 'the adapter in the log'
    $title = $process.MainWindowTitle
    Close-App $process
    $started.WaitForExit()
    if ($started.ExitCode -ne 0) {
        throw "Exit code $($started.ExitCode)"
    }
    "title '$title'", $adapter
}

# The Sandbox may still share the host GPU although the .wsb disables it.
Step 'Run' {
    $controllers = @(Get-CimInstance Win32_VideoController | ForEach-Object Name)
    "display adapters: $($controllers -join ', ')"
    Start-AndClose
}

Step 'Run on WARP' {
    $env:UGURUGU_ADAPTER = 'warp'
    try {
        $result = Start-AndClose
    } finally {
        Remove-Item Env:UGURUGU_ADAPTER
    }
    if ($result[-1] -notmatch 'Basic Render Driver|Cpu') {
        throw "Not WARP: $($result[-1])"
    }
    $result
}

Step 'Open by association from a Korean path' {
    $folder = Join-Path ([Environment]::GetFolderPath('Desktop')) (Text 0xD55C, 0xAE00, 0x20, 0xD3F4, 0xB354)
    $name = Text 0xC0C8, 0x20, 0xADF8, 0xB9BC
    New-Item -ItemType Directory -Path $folder | Out-Null
    $document = Join-Path $folder "$name.ugurugu"
    Copy-Item (Join-Path $in 'drawing.ugurugu') $document
    Start-Process -FilePath $document
    $process = Wait-Window
    $title = Wait-Until { $process.Refresh(); if ($process.MainWindowTitle.StartsWith($name)) { $process.MainWindowTitle } } 60 'the document title'
    $opened = Log-Lines 'document opened' | Select-Object -Last 1
    Close-App $process
    if ($opened -notmatch 'ok=true') {
        throw "Not opened: $opened"
    }
    "title '$title'", $opened
}

Step 'Update to 3.0.1 with unsaved changes' {
    $env:UGURUGU_UPDATE_FEED = Join-Path $in 'feed'
    try {
        Start-Process -FilePath $app | Out-Null
        $process = Wait-Window
        Press $process 'Add layer'
        # The automatic check runs two seconds after start.
        Press $process 'Update' 60
        [void](Find-Element $process { param($n) $n -eq 'Unsaved changes' } 120 'the question about unsaved changes')
        Press $process 'Cancel (ESC)'
        Start-Sleep -Seconds 3
        if ($process.HasExited) {
            throw 'The app closed although the question was cancelled'
        }
        Press $process 'Help'
        $item = Find-Element $process { param($n) $n.StartsWith('Check for Updates') } 10 'Check for Updates'
        $item.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
        Press $process "Don't Save (N)" 30
        if (-not $process.WaitForExit(30000)) {
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
    "asked about unsaved changes, cancelled, asked again from Check for Updates, updated to $version and started again"
}

Step 'Check for updates offline' {
    Start-Process -FilePath $app | Out-Null
    $process = Wait-Window
    Press $process 'Help'
    $item = Find-Element $process { param($n) $n.StartsWith('Check for Updates') } 10 'Check for Updates'
    $item.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
    $said = Find-Element $process { param($n) $n.StartsWith('Could not check for updates') } 60 'the failure notice'
    $text = $said.Current.Name -replace '\s+', ' '
    Press $process 'Close'
    Close-App $process
    "notice '$text'", (Log-Lines 'update check failed' | Select-Object -Last 1)
}

Step 'Uninstall' {
    $uninstaller = Start-Process -FilePath (Join-Path $installRoot 'Update.exe') `
        -ArgumentList @('uninstall', '--silent') -PassThru -Wait
    if ($uninstaller.ExitCode -ne 0) {
        throw "Uninstall exited with code $($uninstaller.ExitCode)"
    }
    $left = @()
    if (Test-Path $app) { $left += 'the app' }
    $left += @($shortcuts | Where-Object { Test-Path $_ })
    if ((Test-Path $association) -or (Test-Path "HKCU:\Software\Classes\$progId")) { $left += 'the .ugurugu association' }
    if (Test-Path $uninstallKey) { $left += 'the Apps entry' }
    if ($left) {
        throw "Left after uninstalling: $($left -join ', ')"
    }
    $kept = @(
        (Join-Path $env:LOCALAPPDATA 'Ugurugu'),
        (Join-Path $env:APPDATA 'Ugurugu'),
        $installRoot
    ) | Where-Object { Test-Path $_ } | ForEach-Object {
        "$_ ($(@(Get-ChildItem -Recurse -File $_).Count) files)"
    }
    'app, shortcuts, association and Apps entry removed', "kept: $(if ($kept) { $kept -join ', ' } else { 'nothing' })"
}

if (Test-Path $logs) {
    Copy-Item -Recurse $logs (Join-Path $out 'logs')
}
Write-Host "Done: $(Join-Path $out 'results.md')"
