# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

# Checks what the Rust app exposes to UI Automation, the interface Narrator
# and other screen readers use (docs/rust/m0-evidence.md section 7): prints the
# tree, renames the top layer with F2 from its focused row and presses the Add
# layer button through InvokePattern. Then, from the keyboard alone (M5-14):
# F10 and Alt+E reach the menus, the settings dialog takes the focus and gives
# it back on Escape, and arrows widen the left panel dock. Runs in Windows
# PowerShell 5.1, which has the .NET UI Automation client.
#
# Usage: powershell -File uia_probe.ps1 <ugurugu.exe> <output-dir>

param([string]$Exe, [string]$Out)

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
New-Item -ItemType Directory -Force $Out | Out-Null
$log = Join-Path $Out 'app.log'
$env:UGURUGU_LOG = 'info'
# Elements are found by their English names.
$env:UGURUGU_LANGUAGE = 'en'
# Its own settings and recovery, which the dock step changes.
$env:UGURUGU_SETTINGS_PATH = Join-Path $Out 'settings.json'
$env:UGURUGU_RECOVERY_PATH = Join-Path $Out 'recovery'
Remove-Item -ErrorAction SilentlyContinue $env:UGURUGU_SETTINGS_PATH
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
    for ($i = 0; $i -lt 30 -and ($tree | Where-Object Name -eq 'Add layer').Count -eq 0; $i++) {
        Start-Sleep -Milliseconds 200
        $tree = Get-Tree $window
    }
    $tree | ForEach-Object { ('  ' * $_.Depth) + $_.Type + ' "' + $_.Name + '"' } | Set-Content -Encoding utf8 (Join-Path $Out 'tree.txt')
    "elements: $($tree.Count)"
    $tree | Group-Object Type | Sort-Object Count -Descending | ForEach-Object { "  $($_.Name): $($_.Count)" }
    # Controls a screen reader would announce without a name.
    $unnamed = @($tree | Where-Object { $_.Name -eq '' -and $_.Type -in 'Button', 'CheckBox', 'ComboBox', 'Spinner', 'Slider', 'Edit' })
    "unnamed controls: $($unnamed.Count)" + $(if ($unnamed) { ' (' + (($unnamed | ForEach-Object Type) -join ', ') + ')' })

    # A layer is renamed in its row, as in 2.2.13: focus the row through UI
    # Automation, press F2, type and press Enter, as a keyboard user would.
    # egui text fields take keys but not ValuePattern.SetValue.
    Add-Type -AssemblyName System.Windows.Forms
    Add-Type 'using System; using System.Runtime.InteropServices; public static class Front { [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow(); }'
    $layers = [array]::IndexOf(@($tree | ForEach-Object Name), 'Layers')
    # Rows are the buttons that toggle (selected), unlike the dock's close button.
    $row = ($tree | Select-Object -Skip $layers | Where-Object {
        $_.Type -eq 'Button' -and $_.Element.GetSupportedPatterns() -contains [System.Windows.Automation.TogglePattern]::Pattern
    } | Select-Object -First 1).Element
    if ($row -ne $null) {
        $before = $row.Current.Name
        $row.SetFocus()
        Start-Sleep -Milliseconds 300
        # SendKeys types into whatever is in front.
        if ([Front]::GetForegroundWindow() -ne $app.MainWindowHandle) { throw 'the app is not in front' }
        [System.Windows.Forms.SendKeys]::SendWait('{F2}')
        Start-Sleep -Milliseconds 300
        $edit = (Get-Tree $window | Where-Object Type -eq 'Edit' | Select-Object -First 1).Element
        if ($edit -eq $null) { "no rename field after F2 on '$before'" } else {
            if ([Front]::GetForegroundWindow() -ne $app.MainWindowHandle) { throw 'the app is not in front' }
            [System.Windows.Forms.SendKeys]::SendWait('^auia{ENTER}')
            Start-Sleep -Milliseconds 500
            $renamed = Get-Tree $window | Where-Object { $_.Type -eq 'Button' -and $_.Name -eq 'uia' }
            "row '$before' after F2, typing and Enter: " + $(if ($renamed) { "renamed to 'uia'" } else { 'not renamed' })
        }
    } else { 'no layer row' }

    $add = ($tree | Where-Object { $_.Type -eq 'Button' -and $_.Name -eq 'Add layer' } | Select-Object -First 1).Element
    if ($add -ne $null) {
        $add.SetFocus()
        Start-Sleep -Milliseconds 300
        $add.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
        'Add layer invoked'
    } else { 'no Add layer button' }
    Start-Sleep -Milliseconds 500

    # Keyboard alone. SendKeys goes to whatever is in front, so check first.
    function Send($keys) {
        if ([Front]::GetForegroundWindow() -ne $app.MainWindowHandle) { throw 'the app is not in front' }
        [System.Windows.Forms.SendKeys]::SendWait($keys)
        Start-Sleep -Milliseconds 300
    }
    function Focused { [System.Windows.Automation.AutomationElement]::FocusedElement.Current }
    function Tab-To($prefix) {
        for ($i = 0; $i -lt 150; $i++) {
            if ((Focused).Name.StartsWith($prefix)) { return $true }
            Send '{TAB}'
        }
        $false
    }
    Send '{ESC}'
    Send '{F10}'
    "F10 focuses: '$((Focused).Name)'"
    Send '%e'
    if (Tab-To 'Settings') {
        Send ' '
        Start-Sleep -Milliseconds 300
        $dialog = @(Get-Tree $window | Where-Object Name -eq 'Restore defaults').Count -gt 0
        "settings dialog open: $dialog, focus in it: '$((Focused).Name)'"
        Send '{ESC}'
        $closed = @(Get-Tree $window | Where-Object Name -eq 'Restore defaults').Count -eq 0
        "after Escape closed: $closed, focus back on: '$((Focused).Name)'"
    } else { 'Tab did not reach Settings in the Edit menu' }
    if (Tab-To 'Width of the left panel dock') {
        $before = (Focused).BoundingRectangle.X
        Send '{RIGHT}{RIGHT}'
        $after = (Focused).BoundingRectangle.X
        "left dock edge moved by Right twice: $($after - $before) px, focus: '$((Focused).Name)'"
    } else { 'Tab did not reach the left dock edge' }
} finally {
    if (-not $app.HasExited) { $app.CloseMainWindow() | Out-Null; if (-not $app.WaitForExit(5000)) { $app.Kill() } }
}
Get-Content $log | Select-String 'layer renamed|layer added|panicked|ERROR' | Select-Object -Last 3 | ForEach-Object { $_.Line }
