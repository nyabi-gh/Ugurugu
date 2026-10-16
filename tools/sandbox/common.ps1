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

# UI Automation on a window that is still starting can time out (backlog
# B3-97). Windows PowerShell gets that as a COMException, not a
# TimeoutException, so it is known by its HRESULT, COR_E_TIMEOUT.
$uiaTimeout = 0x80131505
# What timed out and was asked again in the current step.
$script:retried = New-Object System.Collections.ArrayList

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

function Get-Cause($Exception) {
    while ($Exception.InnerException) { $Exception = $Exception.InnerException }
    $Exception
}

# The message, and for an exception that did not come from a script's own
# `throw` its type and HRESULT.
function Format-Error($Exception) {
    $cause = Get-Cause $Exception
    if ($cause.GetType() -eq [Management.Automation.RuntimeException]) {
        return $Exception.Message
    }
    '{0} [{1} 0x{2:X8}]' -f $Exception.Message, $cause.GetType().FullName, $cause.HResult
}

# A step that passed only because UI Automation was asked again says so, so
# that the retry does not hide a window that stops answering.
function Format-Retried {
    $counts = $script:retried | Group-Object | ForEach-Object {
        if ($_.Count -gt 1) { "$($_.Name) x$($_.Count)" } else { $_.Name }
    }
    "UI Automation timed out and was asked again: $($counts -join ', ')"
}

function Step([string] $Name, [scriptblock] $Body) {
    $chosen = Get-Variable only -ValueOnly -ErrorAction SilentlyContinue
    if ($chosen -and $Name -notin $chosen) {
        return
    }
    Write-Host "== $Name"
    $script:retried.Clear()
    try {
        $detail = @(& $Body) -join '; '
        $result = 'pass'
        if ($script:retried.Count) {
            $result = 'pass after retry'
            $detail = "$detail; $(Format-Retried)"
        }
        [void]$results.Add([pscustomobject]@{ Step = $Name; Result = $result; Detail = $detail })
    } catch {
        $detail = Format-Error $_.Exception
        if ($script:retried.Count) {
            $detail = "$detail; $(Format-Retried)"
        }
        [void]$results.Add([pscustomobject]@{ Step = $Name; Result = 'FAIL'; Detail = $detail })
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
    $timedOut = $null
    while ((Get-Date) -lt $deadline) {
        # A UI Automation timeout is not there yet, not a failure.
        try {
            $value = & $Condition
        } catch {
            $cause = Get-Cause $_.Exception
            if ($cause.HResult -ne $uiaTimeout) { throw }
            $timedOut = $_.Exception
            [void]$script:retried.Add(('{0} ({1} 0x{2:X8})' -f $What, $cause.GetType().Name, $cause.HResult))
            $value = $null
        }
        if ($value) {
            return $value
        }
        Start-Sleep -Milliseconds 500
    }
    if ($timedOut) {
        throw "Timed out after $Seconds s waiting for $What; last: $(Format-Error $timedOut)"
    }
    throw "Timed out after $Seconds s waiting for $What"
}

# A UI Automation read whose answer may be empty or false, asked again only
# while it times out.
function Read-Automation([scriptblock] $Read, [int] $Seconds, [string] $What) {
    (Wait-Until { [pscustomobject]@{ Answer = & $Read } } $Seconds $What).Answer
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
