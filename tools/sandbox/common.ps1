# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

<#
.SYNOPSIS
Helpers shared by the Windows Sandbox runs (accept.ps1, alongside.ps1).

.DESCRIPTION
Dot-sourced after the caller sets $title, $out, $results, $installRoot and
$logs and defines Stop-Apps, which a failed step calls.
#>

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes

function Text([int[]] $codes) {
    -join ($codes | ForEach-Object { [char] $_ })
}

function Save-Results {
    $lines = @("# $title", '', "$(Get-Date -Format s), $([Environment]::OSVersion.VersionString)", '',
        '| Step | Result | Detail |', '|---|---|---|')
    foreach ($row in $results) {
        $lines += "| $($row.Step) | $($row.Result) | $($row.Detail -replace '\|', '/') |"
    }
    [IO.File]::WriteAllLines((Join-Path $out 'results.md'), $lines, (New-Object Text.UTF8Encoding $false))
}

function Step([string] $Name, [scriptblock] $Body) {
    $chosen = Get-Variable only -ValueOnly -ErrorAction SilentlyContinue
    if ($chosen -and $Name -notin $chosen) {
        return
    }
    Write-Host "== $Name"
    try {
        $detail = @(& $Body) -join '; '
        [void]$results.Add([pscustomobject]@{ Step = $Name; Result = 'pass'; Detail = $detail })
    } catch {
        [void]$results.Add([pscustomobject]@{ Step = $Name; Result = 'FAIL'; Detail = $_.Exception.Message })
        Stop-Apps
    }
    Save-Results
}

function Get-DefaultValue([string] $Key) {
    if (-not (Test-Path $Key)) {
        return $null
    }
    (Get-Item $Key).GetValue('')
}

# The 2.2.13 root is a prefix of the 3.0 root, so the separator is compared too.
function Get-AppProcess([string] $Root = $installRoot) {
    $prefix = $Root.TrimEnd('\') + '\'
    @(Get-Process ugurugu -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -and $_.Path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) })
}

function Get-InstalledVersion([string] $Root = $installRoot) {
    $nuspec = [xml](Get-Content (Join-Path $Root 'current\sq.version'))
    $nuspec.package.metadata.version
}

function Wait-Until([scriptblock] $Condition, [int] $Seconds, [string] $What) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        # A window still starting can let UI Automation time out, and an
        # element can go while the tree is read; that is not there yet, not
        # a failure.
        try {
            $value = & $Condition
        } catch {
            $cause = $_.Exception
            while ($cause.InnerException) { $cause = $cause.InnerException }
            if ($cause -isnot [TimeoutException] -and $cause.GetType().Name -ne 'ElementNotAvailableException') { throw }
            $value = $null
        }
        if ($value) {
            return $value
        }
        Start-Sleep -Milliseconds 500
    }
    throw "Timed out after $Seconds s waiting for $What"
}

function Wait-Window([int] $Seconds = 90, [string] $Root = $installRoot) {
    Wait-Until {
        $process = Get-AppProcess $Root | Select-Object -First 1
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
