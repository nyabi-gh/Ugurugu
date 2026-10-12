# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

<#
.SYNOPSIS
GUI checks of 3.0 that need a real window, run inside Windows Sandbox.

.DESCRIPTION
Started at logon by the .wsb that prepare.ps1 -Scenario gui writes, with
C:\ugu\in (ugurugu.exe; read-only) and C:\ugu\out (results and screenshots)
mapped. Keys and the mouse go to the front window, so this runs only in the
Sandbox, where the app is the only window. Checks: the notice about an
unexpected close, the exe and window icons, the help window, turning and
flipping the view, the eyedropper's cursor, the docks of a short window
(review U3-07) and the save dialog's added extension (review U3-08). The
screenshots are of the Sandbox's screen.
Runs in Windows PowerShell 5.1.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$title = 'GUI checks'
$in = 'C:\ugu\in'
$out = 'C:\ugu\out'
$installRoot = $in
$logs = Join-Path $out 'logs'
$app = Join-Path $in 'ugurugu.exe'
$results = New-Object System.Collections.ArrayList

. (Join-Path $PSScriptRoot 'common.ps1')
Add-Type -AssemblyName System.Drawing, System.Windows.Forms

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class Native
{
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct CURSORINFO { public int cbSize, flags; public IntPtr hCursor; public POINT pt; }

    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hwnd, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool GetCursorInfo(ref CURSORINFO info);
    [DllImport("user32.dll")] public static extern IntPtr LoadCursor(IntPtr instance, IntPtr name);
    [DllImport("user32.dll")] static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] static extern void mouse_event(uint flags, int dx, int dy, int data, UIntPtr extra);

    public static void Front(IntPtr hwnd)
    {
        keybd_event(0x12, 0, 0, UIntPtr.Zero);
        keybd_event(0x12, 0, 2, UIntPtr.Zero);
        SetForegroundWindow(hwnd);
    }

    public static void MoveTo(int x, int y)
    {
        SetCursorPos(x, y);
        mouse_event(0x1, 1, 0, 0, UIntPtr.Zero);
        mouse_event(0x1, -1, 0, 0, UIntPtr.Zero);
    }

    public static void Wheel(int notches)
    {
        mouse_event(0x800, 0, 0, notches * 120, UIntPtr.Zero);
    }

    public static bool CursorIs(int id)
    {
        var info = new CURSORINFO();
        info.cbSize = Marshal.SizeOf(typeof(CURSORINFO));
        GetCursorInfo(ref info);
        return info.hCursor == LoadCursor(IntPtr.Zero, new IntPtr(id));
    }

    delegate bool EnumProc(IntPtr hwnd, IntPtr data);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc proc, IntPtr data);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint process);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr hwnd, System.Text.StringBuilder text, int count);

    // A visible dialog-class top-level window of the process, or zero.
    public static IntPtr Dialog(uint process)
    {
        IntPtr found = IntPtr.Zero;
        EnumWindows((hwnd, data) =>
        {
            uint owner;
            GetWindowThreadProcessId(hwnd, out owner);
            var kind = new System.Text.StringBuilder(64);
            GetClassName(hwnd, kind, 64);
            if (owner == process && IsWindowVisible(hwnd) && kind.ToString() == "#32770")
            {
                found = hwnd;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    public static string CursorName()
    {
        var info = new CURSORINFO();
        info.cbSize = Marshal.SizeOf(typeof(CURSORINFO));
        GetCursorInfo(ref info);
        int[] ids = { 32512, 32513, 32515, 32646, 32642, 32643, 32644, 32645, 32649 };
        string[] names = { "arrow", "ibeam", "cross", "sizeall", "sizenwse", "sizenesw", "sizewe", "sizens", "hand" };
        for (int i = 0; i < ids.Length; i++)
            if (info.hCursor == LoadCursor(IntPtr.Zero, new IntPtr(ids[i]))) return names[i];
        return "custom 0x" + info.hCursor.ToString("x");
    }
}
'@

[void][Native]::SetProcessDpiAwarenessContext([IntPtr](-4))
$script:shots = 0

function Stop-Apps {
    Get-AppProcess | ForEach-Object { Stop-Process -Id $_.Id -Force }
}

function Save-Screen([string] $Name) {
    $script:shots++
    $bounds = [Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bitmap = New-Object Drawing.Bitmap $bounds.Width, $bounds.Height
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($bounds.Location, [Drawing.Point]::Empty, $bounds.Size)
    $file = Join-Path $out ("{0:00}-$Name.png" -f $script:shots)
    $bitmap.Save($file)
    $graphics.Dispose()
    $bitmap.Dispose()
    Split-Path -Leaf $file
}

function Start-App([int] $Width = 1600, [int] $Height = 1000) {
    $env:UGURUGU_LANGUAGE = 'en'
    $env:UGURUGU_SETTINGS_PATH = Join-Path $env:TEMP 'gui-settings.json'
    $env:UGURUGU_RECOVERY_PATH = Join-Path $env:TEMP 'gui-recovery'
    $env:UGURUGU_LOG_PATH = $logs
    if (-not (Test-Path $env:UGURUGU_SETTINGS_PATH)) {
        Set-Content -Path $env:UGURUGU_SETTINGS_PATH -Encoding ascii -Value '{"format": "ugurugu.settings", "version": 1, "checkForUpdates": false}'
    }
    Start-Process -FilePath $app | Out-Null
    $process = Wait-Window
    [void][Native]::SetWindowPos($process.MainWindowHandle, [IntPtr]::Zero, 50, 50, $Width, $Height, 4)
    [Native]::Front($process.MainWindowHandle)
    [void](Get-Value $process 'Canvas zoom percentage')
    Start-Sleep -Seconds 1
    $process
}

function Get-Rect($Process) {
    $rect = New-Object Native+RECT
    [void][Native]::GetWindowRect($Process.MainWindowHandle, [ref]$rect)
    $rect
}

# Labels and tooltips share names with controls, so the first element with
# a usable pattern is taken.
function Find-Pattern($Process, [string] $Name, $Patterns, [switch] $Prefix) {
    Wait-Until {
        $root = [Windows.Automation.AutomationElement]::FromHandle($Process.MainWindowHandle)
        $all = $root.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition)
        foreach ($element in $all) {
            $label = [string]$element.Current.Name
            if (-not ($label -eq $Name -or ($Prefix -and $label.StartsWith($Name)))) {
                continue
            }
            foreach ($pattern in $Patterns) {
                $found = $null
                if ($element.TryGetCurrentPattern($pattern, [ref]$found)) {
                    return , $found
                }
            }
        }
    } 15 "a control named '$Name'"
}

function Invoke-Named($Process, [string] $Name, [switch] $Prefix) {
    $pattern = Find-Pattern $Process $Name @([Windows.Automation.InvokePattern]::Pattern,
        [Windows.Automation.TogglePattern]::Pattern, [Windows.Automation.SelectionItemPattern]::Pattern) -Prefix:$Prefix
    if ($pattern -is [Windows.Automation.InvokePattern]) {
        $pattern.Invoke()
    } elseif ($pattern -is [Windows.Automation.TogglePattern]) {
        $pattern.Toggle()
    } else {
        $pattern.Select()
    }
    Start-Sleep -Milliseconds 500
}

function Get-Value($Process, [string] $Name) {
    $pattern = Find-Pattern $Process $Name @([Windows.Automation.RangeValuePattern]::Pattern)
    [double]$pattern.Current.Value
}

function Send-Keys($Process, [string] $Keys) {
    [Native]::Front($Process.MainWindowHandle)
    Start-Sleep -Milliseconds 200
    [Windows.Forms.SendKeys]::SendWait($Keys)
    Start-Sleep -Milliseconds 500
}

# A Windows window (rgnupdt.exe) opens a few seconds after logon and takes
# the front.
$deadline = (Get-Date).AddSeconds(30)
while ((Get-Date) -lt $deadline -and -not (Get-Process rgnupdt -ErrorAction SilentlyContinue)) {
    Start-Sleep -Milliseconds 500
}
Get-Process rgnupdt -ErrorAction SilentlyContinue | Stop-Process -Force

Step 'Notice about an unexpected close' {
    New-Item -ItemType Directory -Force -Path $logs | Out-Null
    $record = Join-Path $logs 'crash-ugurugu-20261012-000000-1-panic-0.txt'
    Set-Content -Path $record -Encoding ascii -Value 'planted by gui.ps1'
    $process = Start-App
    [void](Find-Element $process { param($n) $n -eq 'Ugurugu closed unexpectedly' } 20 'the notice')
    $shot = Save-Screen 'crash-notice'
    Press $process 'Close'
    Start-Sleep -Seconds 1
    $seen = Test-Path ($record -replace '\.txt$', '.seen.txt')
    Close-App $process
    if (-not $seen) {
        throw "The record was not marked as told about; $shot"
    }
    "notice shown and closed, record marked seen; $shot"
}

Step 'Exe and window icons' {
    $icon = [Drawing.Icon]::ExtractAssociatedIcon($app)
    $bitmap = $icon.ToBitmap()
    $file = Join-Path $out 'exe-icon.png'
    $bitmap.Save($file)
    $process = Start-App
    $shot = Save-Screen 'window-and-taskbar-icon'
    Close-App $process
    "exe icon $($bitmap.Width)x$($bitmap.Height) in exe-icon.png; title bar and taskbar in $shot"
}

Step 'Help window' {
    $process = Start-App
    Send-Keys $process '{F1}'
    [void](Find-Element $process { param($n) $n -eq 'Ugurugu Help' -or $n -eq 'Getting started' } 15 'the help window')
    $shot = Save-Screen 'help-window'
    Close-App $process
    "F1 opened it; $shot"
}

Step 'Turn and flip the view' {
    $process = Start-App
    $turn0 = Get-Value $process 'Canvas rotation angle'
    Invoke-Named $process 'Rotate canvas right'
    $turn1 = Get-Value $process 'Canvas rotation angle'
    Invoke-Named $process 'Flip canvas horizontally'
    $shot = Save-Screen 'turned-and-flipped'
    Invoke-Named $process 'Reset canvas rotation'
    $turn2 = Get-Value $process 'Canvas rotation angle'
    Close-App $process
    if ($turn1 - $turn0 -ne 5 -or $turn2 -ne 0) {
        throw "turn $turn0 -> $turn1 -> $turn2; $shot"
    }
    "turn $turn0 -> $turn1 (right) -> $turn2 (reset); flipped in $shot"
}

Step 'Eyedropper cursor' {
    $process = Start-App
    $rect = Get-Rect $process
    Invoke-Named $process 'Eyedropper'
    [Native]::MoveTo([int](($rect.Left + $rect.Right) / 2), [int](($rect.Top + $rect.Bottom) / 2))
    Start-Sleep -Milliseconds 500
    [Native]::MoveTo([int](($rect.Left + $rect.Right) / 2) + 10, [int](($rect.Top + $rect.Bottom) / 2))
    Start-Sleep -Milliseconds 500
    $cursor = [Native]::CursorName()
    $shot = Save-Screen 'eyedropper-cursor'
    Close-App $process
    if ($cursor -ne 'cross') {
        throw "cursor over the canvas: $cursor; $shot"
    }
    "cursor over the canvas: $cursor"
}

Step 'Docks of a short window (U3-07)' {
    $process = Start-App 1000 520
    $rect = Get-Rect $process
    $before = Save-Screen 'short-window'
    [Native]::MoveTo($rect.Left + 250, $rect.Top + 300)
    Start-Sleep -Milliseconds 300
    [Native]::Wheel(-10)
    Start-Sleep -Milliseconds 800
    $after = Save-Screen 'short-window-left-dock-scrolled'
    [Native]::MoveTo($rect.Right - 150, $rect.Top + 300)
    Start-Sleep -Milliseconds 300
    [Native]::Wheel(-10)
    Start-Sleep -Milliseconds 800
    $right = Save-Screen 'short-window-right-dock-scrolled'
    Close-App $process
    "window 1000x520; $before, $after, $right"
}

Step 'Save dialog with another extension typed (U3-08)' {
    $documents = [Environment]::GetFolderPath('MyDocuments')
    $existing = Join-Path $documents 'probe.png.ugurugu'
    Set-Content -Path $existing -Encoding ascii -Value 'keep'
    $process = Start-App
    Invoke-Named $process 'File'
    [void](Save-Screen 'file-menu')
    Invoke-Named $process 'Save As' -Prefix
    Start-Sleep -Seconds 1
    [void](Save-Screen 'save-as-pressed')
    $owner = [uint32]$process.Id
    $dialog = Wait-Until { $found = [Native]::Dialog($owner); if ($found -ne [IntPtr]::Zero) { $found } } 20 'the save dialog'
    $dialogName = [Windows.Automation.AutomationElement]::FromHandle($dialog).Current.Name
    Start-Sleep -Seconds 2
    [Native]::Front($dialog)
    Start-Sleep -Milliseconds 500
    # The first keys can arrive before the name box has the focus.
    [Windows.Forms.SendKeys]::SendWait('^a')
    Start-Sleep -Milliseconds 500
    [Windows.Forms.SendKeys]::SendWait('probe.png')
    Start-Sleep -Milliseconds 500
    [Windows.Forms.SendKeys]::SendWait('{ENTER}')
    Start-Sleep -Seconds 2
    $shot = Save-Screen 'after-typing-probe-png'
    $next = [Native]::Dialog($owner)
    $open = if ($next -ne [IntPtr]::Zero) { [Windows.Automation.AutomationElement]::FromHandle($next).Current.Name } else { 'none' }
    for ($i = 0; $i -lt 3 -and [Native]::Dialog($owner) -ne [IntPtr]::Zero; $i++) {
        [Native]::Front([Native]::Dialog($owner))
        [Windows.Forms.SendKeys]::SendWait('{ESC}')
        Start-Sleep -Milliseconds 800
    }
    $kept = (Get-Content $existing -Raw).Trim()
    $files = @(Get-ChildItem $documents -Filter '*.ugurugu' | ForEach-Object Name)
    Get-AppProcess | ForEach-Object { Stop-Process -Id $_.Id -Force }
    if ($kept -ne 'keep') {
        throw "probe.png.ugurugu was replaced without asking; files: $($files -join ', '); $shot"
    }
    "dialog '$dialogName'; after 'probe.png': dialog then '$open'; probe.png.ugurugu kept; files: $($files -join ', '); $shot"
}

if (Test-Path $logs) {
    Write-Host "Logs in $logs"
}
Write-Host "Done: $(Join-Path $out 'results.md')"
