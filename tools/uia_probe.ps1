# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

# Checks what the Rust M0 app exposes to UI Automation, the interface Narrator
# and other screen readers use (docs/rust/m0-evidence.md section 7): prints the
# tree, types into the first text field (the layer name) and presses the Add
# layer button through InvokePattern. Runs in Windows PowerShell 5.1, which
# has the .NET UI Automation client.
#
# Usage: powershell -File uia_probe.ps1 <ugurugu.exe> <output-dir>

param([string]$Exe, [string]$Out)

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
New-Item -ItemType Directory -Force $Out | Out-Null
$log = Join-Path $Out 'app.log'
$env:UGURUGU_LOG = 'info'
$app = Start-Process -FilePath $Exe -PassThru -RedirectStandardOutput $log -RedirectStandardError (Join-Path $Out 'stderr.log')

function Get-Tree($root) {
    $walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
    $items = @()
    $stack = New-Object System.Collections.Stack
    $stack.Push(@($root, 0))
    while ($stack.Count -gt 0) {
        $entry = $stack.Pop()
        $element = $entry[0]; $depth = $entry[1]
        $current = $element.Current
        $items += [pscustomobject]@{
            Depth = $depth
            Type = $current.ControlType.ProgrammaticName -replace 'ControlType\.', ''
            Name = $current.Name
            Element = $element
        }
        $children = @()
        $child = $walker.GetFirstChild($element)
        while ($child -ne $null) { $children += $child; $child = $walker.GetNextSibling($child) }
        [array]::Reverse($children)
        foreach ($c in $children) { $stack.Push(@($c, $depth + 1)) }
    }
    $items
}

try {
    $window = $null
    for ($i = 0; $i -lt 100 -and $window -eq $null; $i++) {
        Start-Sleep -Milliseconds 100
        $app.Refresh()
        if ($app.MainWindowHandle -ne 0) {
            $window = [System.Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle)
        }
    }
    if ($window -eq $null) { throw 'no window' }
    # The first query only asks the app for its tree; it arrives with the next frame.
    $tree = Get-Tree $window
    for ($i = 0; $i -lt 30 -and ($tree | Where-Object Type -eq 'Edit').Count -eq 0; $i++) {
        Start-Sleep -Milliseconds 200
        $tree = Get-Tree $window
    }
    $tree | ForEach-Object { ('  ' * $_.Depth) + $_.Type + ' "' + $_.Name + '"' } | Set-Content -Encoding utf8 (Join-Path $Out 'tree.txt')
    "elements: $($tree.Count)"
    $tree | Group-Object Type | Sort-Object Count -Descending | ForEach-Object { "  $($_.Name): $($_.Count)" }

    # egui text fields take focus and keys but not ValuePattern.SetValue, so
    # this focuses the field through UI Automation and types, as a screen
    # reader user would.
    $edit = ($tree | Where-Object Type -eq 'Edit' | Select-Object -First 1).Element
    if ($edit -ne $null) {
        Add-Type -AssemblyName System.Windows.Forms
        $edit.SetFocus()
        Start-Sleep -Milliseconds 300
        [System.Windows.Forms.SendKeys]::SendWait('uia')
        Start-Sleep -Milliseconds 500
        $value = $edit.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
        "edit '$($edit.Current.Name)' value after typing: '$($value.Current.Value)'"
    } else { 'no Edit element' }

    # Moving focus away commits the typed name.
    $add = ($tree | Where-Object { $_.Type -eq 'Button' -and $_.Name -eq 'Add layer' } | Select-Object -First 1).Element
    if ($add -ne $null) {
        $add.SetFocus()
        Start-Sleep -Milliseconds 300
        $add.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
        'Add layer invoked'
    } else { 'no Add layer button' }
    Start-Sleep -Milliseconds 500
} finally {
    if (-not $app.HasExited) { $app.CloseMainWindow() | Out-Null; if (-not $app.WaitForExit(5000)) { $app.Kill() } }
}
Get-Content $log | Select-String 'layer renamed|layer added|panicked|ERROR' | Select-Object -Last 3 | ForEach-Object { $_.Line }
