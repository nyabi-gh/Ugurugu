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
flipping the view, the eyedropper's cursor, the docks of a short window,
the save dialog's added extension, the transform box's cursors, no blank
frame when pressing during playback, a tab keeping its drawing while
another is shown, every tab coming back after a crash, Tab moving the
keyboard focus, a file opened from outside going to the open window as
a tab, and the window opening where and as it closed.
"Screen set" photographs every panel, menu and dialog in Korean and English
for reviewing the look, and "Screen set at 100%" in Korean, English and
Japanese with the interface drawn at 100%; the names they press come from
the .ftl files.
in\only.txt, from prepare.ps1 -Only, limits the run to the steps it names.
The screenshots are of the Sandbox's screen.
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
# Names of the steps to run, one a line; all without the file.
$onlyFile = Join-Path $in 'only.txt'
$only = if (Test-Path $onlyFile) { @(Get-Content $onlyFile | Where-Object { $_ }) } else { @() }

. (Join-Path $PSScriptRoot 'common.ps1')
Add-Type -AssemblyName System.Drawing, System.Windows.Forms

Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
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
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hwnd, int command);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
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

    [DllImport("user32.dll")] static extern int GetSystemMetrics(int index);

    // An absolute move, as a real mouse sends it.
    public static void Glide(int x, int y)
    {
        int dx = (int)((x * 65535L) / (GetSystemMetrics(0) - 1));
        int dy = (int)((y * 65535L) / (GetSystemMetrics(1) - 1));
        mouse_event(0x8001, dx, dy, 0, UIntPtr.Zero);
    }

    public static void Down() { mouse_event(0x2, 0, 0, 0, UIntPtr.Zero); }
    public static void Up() { mouse_event(0x4, 0, 0, 0, UIntPtr.Zero); }
    public static void RightClick()
    {
        mouse_event(0x8, 0, 0, 0, UIntPtr.Zero);
        mouse_event(0x10, 0, 0, 0, UIntPtr.Zero);
    }

    // A drag through the points, in steps of a few pixels every few ms.
    public static void Drag(int[] xs, int[] ys)
    {
        Glide(xs[0], ys[0]);
        System.Threading.Thread.Sleep(50);
        Down();
        for (int i = 1; i < xs.Length; i++)
        {
            int steps = Math.Max(1, (int)(Math.Sqrt(Math.Pow(xs[i] - xs[i - 1], 2) + Math.Pow(ys[i] - ys[i - 1], 2)) / 4));
            for (int s = 1; s <= steps; s++)
            {
                Glide(xs[i - 1] + (xs[i] - xs[i - 1]) * s / steps, ys[i - 1] + (ys[i] - ys[i - 1]) * s / steps);
                System.Threading.Thread.Sleep(4);
            }
        }
        System.Threading.Thread.Sleep(50);
        Up();
    }

    // Dark pixels in the screen rectangle, as fast as the screen can be read.
    static int Dark(System.Drawing.Bitmap bitmap, System.Drawing.Graphics graphics, int x, int y)
    {
        graphics.CopyFromScreen(x, y, 0, 0, bitmap.Size);
        var area = new System.Drawing.Rectangle(0, 0, bitmap.Width, bitmap.Height);
        var data = bitmap.LockBits(area, System.Drawing.Imaging.ImageLockMode.ReadOnly, System.Drawing.Imaging.PixelFormat.Format32bppArgb);
        var bytes = new byte[data.Stride * data.Height];
        Marshal.Copy(data.Scan0, bytes, 0, bytes.Length);
        bitmap.UnlockBits(data);
        int dark = 0;
        for (int i = 0; i < bytes.Length; i += 4)
            if (bytes[i] + bytes[i + 1] + bytes[i + 2] < 3 * 96) dark++;
        return dark;
    }

    // Drags from (x0, y0) to (x1, y1) in `moves` moves about 1 ms apart, as
    // a 1000 Hz mouse reports, holds, and returns the ms from the last move
    // until the rectangle shows dark pixels, or -1 after `timeoutMs`.
    public static long DragAndWait(int x0, int y0, int x1, int y1, int moves, int x, int y, int w, int h, int timeoutMs)
    {
        var bitmap = new System.Drawing.Bitmap(w, h, System.Drawing.Imaging.PixelFormat.Format32bppArgb);
        var graphics = System.Drawing.Graphics.FromImage(bitmap);
        Glide(x0, y0);
        System.Threading.Thread.Sleep(50);
        Down();
        var tick = System.Diagnostics.Stopwatch.StartNew();
        for (int i = 1; i <= moves; i++)
        {
            Glide(x0 + (x1 - x0) * i / moves, y0 + (y1 - y0) * i / moves);
            while (tick.Elapsed.TotalMilliseconds < i) { }
        }
        var clock = System.Diagnostics.Stopwatch.StartNew();
        long shown = -1;
        while (clock.ElapsedMilliseconds < timeoutMs)
        {
            if (Dark(bitmap, graphics, x, y) > 0)
            {
                shown = clock.ElapsedMilliseconds;
                break;
            }
        }
        System.Threading.Thread.Sleep(100);
        Up();
        graphics.Dispose();
        bitmap.Dispose();
        return shown;
    }

    public static int DarkIn(int x, int y, int w, int h)
    {
        using (var bitmap = new System.Drawing.Bitmap(w, h, System.Drawing.Imaging.PixelFormat.Format32bppArgb))
        using (var graphics = System.Drawing.Graphics.FromImage(bitmap))
            return Dark(bitmap, graphics, x, y);
    }

    // Counts of dark pixels in the rectangle while playing: before, then
    // after pressing at (px, py) and holding for the given time.
    public static string PressAndWatch(int px, int py, int x, int y, int w, int h, int before, int holdMs)
    {
        var bitmap = new System.Drawing.Bitmap(w, h, System.Drawing.Imaging.PixelFormat.Format32bppArgb);
        var graphics = System.Drawing.Graphics.FromImage(bitmap);
        var counts = new System.Collections.Generic.List<string>();
        for (int i = 0; i < before; i++) counts.Add(Dark(bitmap, graphics, x, y).ToString());
        counts.Add("|");
        Glide(px, py);
        System.Threading.Thread.Sleep(30);
        var clock = System.Diagnostics.Stopwatch.StartNew();
        Down();
        while (clock.ElapsedMilliseconds < holdMs)
            counts.Add(Dark(bitmap, graphics, x, y).ToString());
        Up();
        graphics.Dispose();
        bitmap.Dispose();
        return string.Join(" ", counts);
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

# The interface's words in $Language, by key, from its .ftl file.
function Read-Words([string] $Language) {
    $words = @{}
    foreach ($line in [IO.File]::ReadAllLines((Join-Path $in "$Language.ftl"), [Text.Encoding]::UTF8)) {
        if ($line -match '^([a-z0-9-]+) = (.*)$') {
            $words[$Matches[1]] = $Matches[2]
        }
    }
    $words
}

function Start-App([int] $Width = 1600, [int] $Height = 1000, [string] $Language = 'en') {
    $env:UGURUGU_LANGUAGE = $Language
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
    # Read when the window is ready, by its name in $Language.
    [void](Get-Value $process (Read-Words $Language)['canvas-zoom-percent'])
    Start-Sleep -Seconds 1
    $process
}

function Get-Rect($Process) {
    $rect = New-Object Native+RECT
    [void][Native]::GetWindowRect($Process.MainWindowHandle, [ref]$rect)
    $rect
}

function Format-Rect($Rect) {
    "$($Rect.Left),$($Rect.Top),$($Rect.Right),$($Rect.Bottom)"
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

# Types $Text into the field named $Name and presses Enter.
function Set-Field($Process, [string] $Name, [string] $Text) {
    $element = Wait-Until {
        $root = [Windows.Automation.AutomationElement]::FromHandle($Process.MainWindowHandle)
        $match = New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty, $Name)
        foreach ($found in $root.FindAll([Windows.Automation.TreeScope]::Descendants, $match)) {
            $pattern = $null
            if ($found.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
                return , $found
            }
        }
    } 15 "a field named '$Name'"
    $box = $element.Current.BoundingRectangle
    [Native]::Glide([int]($box.X + $box.Width / 2), [int]($box.Y + $box.Height / 2))
    [Native]::Down()
    [Native]::Up()
    Start-Sleep -Milliseconds 300
    [Windows.Forms.SendKeys]::SendWait('^a')
    [Windows.Forms.SendKeys]::SendWait($Text)
    [Windows.Forms.SendKeys]::SendWait('{ENTER}')
    Start-Sleep -Milliseconds 500
}

function Get-Value($Process, [string] $Name) {
    $pattern = Find-Pattern $Process $Name @([Windows.Automation.RangeValuePattern]::Pattern,
        [Windows.Automation.ValuePattern]::Pattern)
    if ($pattern -is [Windows.Automation.ValuePattern]) {
        return [double]($pattern.Current.Value -replace '[^0-9.\-]', '')
    }
    [double]$pattern.Current.Value
}

# The document's edges on the screen: from the middle of the window along a
# row and a column until the colour changes. Run on an empty document.
function Find-Paper($Process, [switch] $Below) {
    $rect = Get-Rect $Process
    [Native]::MoveTo($rect.Right - 5, $rect.Bottom - 5)
    Start-Sleep -Milliseconds 300
    $bounds = [Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bitmap = New-Object Drawing.Bitmap $bounds.Width, $bounds.Height
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($bounds.Location, [Drawing.Point]::Empty, $bounds.Size)
    $cx = [int](($rect.Left + $rect.Right) / 2)
    $cy = [int](($rect.Top + $rect.Bottom) / 2)
    # -Below: the first white below the middle, for a document a bar over
    # the canvas has pushed down.
    if ($Below) {
        $white = { param($y) $c = $bitmap.GetPixel($cx, $y); $c.R -gt 235 -and $c.G -gt 235 -and $c.B -gt 235 }
        while ($cy -lt $rect.Bottom - 1 -and -not (& $white $cy)) { $cy++ }
        # Past the frame's handles along its top edge.
        $cy += 40
    }
    $paper = $bitmap.GetPixel($cx, $cy)
    $same = {
        param($x, $y)
        $c = $bitmap.GetPixel($x, $y)
        [Math]::Abs($c.R - $paper.R) + [Math]::Abs($c.G - $paper.G) + [Math]::Abs($c.B - $paper.B) -lt 30
    }
    $left = $cx; while ($left -gt $rect.Left -and (& $same ($left - 1) $cy)) { $left-- }
    $right = $cx; while ($right -lt $rect.Right -and (& $same ($right + 1) $cy)) { $right++ }
    $top = $cy; while ($top -gt $rect.Top -and (& $same $cx ($top - 1))) { $top-- }
    $bottom = $cy; while ($bottom -lt $rect.Bottom -and (& $same $cx ($bottom + 1))) { $bottom++ }
    $graphics.Dispose()
    $bitmap.Dispose()
    if ($right - $left -lt 200 -or $bottom - $top -lt 200) {
        throw "No document found in the middle of the window ($left, $top, $right, $bottom); $(Save-Screen 'no-document')"
    }
    [pscustomobject]@{ Left = $left; Top = $top; Right = $right; Bottom = $bottom; Colour = "$($paper.R),$($paper.G),$($paper.B)" }
}

# Frames whose dark pixels fall to under half of the fewest while playing,
# or cover over half the area, are a flash.
function Test-Flash([string] $Counts, [int] $Area) {
    $before, $after = $Counts -split ' \| '
    $base = @($before -split ' ' | ForEach-Object { [int]$_ })
    $seen = @($after -split ' ' | ForEach-Object { [int]$_ })
    $low = ($base | Measure-Object -Minimum).Minimum
    $flashes = @($seen | Where-Object { $_ -lt $low / 2 -or $_ -gt $Area / 2 })
    [pscustomobject]@{ Low = $low; Frames = $seen.Count; Flashes = $flashes.Count; Counts = $Counts }
}

# Whether an element named $Name is there now.
function Test-Named($Process, [string] $Name) {
    Start-Sleep -Milliseconds 500
    $root = [Windows.Automation.AutomationElement]::FromHandle($Process.MainWindowHandle)
    $match = New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty, $Name)
    $null -ne $root.FindFirst([Windows.Automation.TreeScope]::Descendants, $match)
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

# At 100% so the buttons under the layer list are on the screen.
Step 'Layer buttons' {
    $env:UGURUGU_UI_SCALE = '100'
    try {
        $process = Start-App
        Invoke-Named $process 'Add layer'
        Invoke-Named $process 'Duplicate layer'
        $copied = Test-Named $process 'Layer 2 copy'
        $shot = Save-Screen 'layer-buttons'
        Invoke-Named $process 'Merge with layer below'
        $merged = -not (Test-Named $process 'Layer 2 copy')
        Invoke-Named $process 'Delete layer'
        $deleted = -not (Test-Named $process 'Layer 2')
    } finally {
        Remove-Item Env:UGURUGU_UI_SCALE
        # The edits leave a project to save or restore.
        Get-AppProcess | ForEach-Object { Stop-Process -Id $_.Id -Force }
        Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
    }
    if (-not ($copied -and $merged -and $deleted)) {
        throw "copied $copied, merged $merged, deleted $deleted; $shot"
    }
    "duplicated, merged down and deleted by name; $shot"
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

Step 'Docks of a short window' {
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

Step 'Save dialog with another extension typed' {
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

Step 'Transform box cursors' {
    # A forced close before leaves a project to restore.
    Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
    $process = Start-App
    $paper = Find-Paper $process
    $w = $paper.Right - $paper.Left
    $h = $paper.Bottom - $paper.Top
    Send-Keys $process 'b'
    [Native]::Drag(@(($paper.Left + [int]($w * 0.3)), ($paper.Left + [int]($w * 0.45))),
        @(($paper.Top + [int]($h * 0.3)), ($paper.Top + [int]($h * 0.4))))
    Send-Keys $process '^a'
    Send-Keys $process '^t'
    Start-Sleep -Milliseconds 500
    $mx = [int](($paper.Left + $paper.Right) / 2)
    $my = [int](($paper.Top + $paper.Bottom) / 2)
    $spots = [ordered]@{
        'top left' = @($paper.Left, $paper.Top, 'sizenwse')
        'top right' = @($paper.Right, $paper.Top, 'sizenesw')
        'bottom right' = @($paper.Right, $paper.Bottom, 'sizenwse')
        'bottom left' = @($paper.Left, $paper.Bottom, 'sizenesw')
        'top' = @($mx, $paper.Top, 'sizens')
        'right' = @($paper.Right, $my, 'sizewe')
        'bottom' = @($mx, $paper.Bottom, 'sizens')
        'left' = @($paper.Left, $my, 'sizewe')
        'inside' = @(($mx + 40), ($my + 40), 'sizeall')
        'outside (turn)' = @(($paper.Left - 30), ($paper.Top - 30), 'arrow')
    }
    $seen = @()
    $wrong = @()
    foreach ($spot in $spots.GetEnumerator()) {
        [Native]::MoveTo($spot.Value[0], $spot.Value[1])
        Start-Sleep -Milliseconds 300
        [Native]::MoveTo($spot.Value[0] + 1, $spot.Value[1])
        Start-Sleep -Milliseconds 300
        $cursor = [Native]::CursorName()
        $seen += "$($spot.Key) $cursor"
        if ($cursor -ne $spot.Value[2]) {
            $wrong += "$($spot.Key): $cursor, not $($spot.Value[2])"
        }
    }
    [Native]::MoveTo($paper.Right, $paper.Bottom)
    Start-Sleep -Milliseconds 300
    $shot = Save-Screen 'transform-box'
    Send-Keys $process '{ESC}'
    Get-AppProcess | ForEach-Object { Stop-Process -Id $_.Id -Force }
    if ($wrong) {
        throw "$($wrong -join ', '); $shot"
    }
    "document $($paper.Left),$($paper.Top)-$($paper.Right),$($paper.Bottom); $($seen -join ', '); $shot"
}

Step 'Pressing during playback keeps the frame shown' {
    # A forced close before leaves a project to restore.
    Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
    $process = Start-App
    $paper = Find-Paper $process
    $w = $paper.Right - $paper.Left
    $h = $paper.Bottom - $paper.Top
    $x = $paper.Left + [int]($w * 0.3)
    $y = $paper.Top + [int]($h * 0.3)
    $rw = [int]($w * 0.4)
    $rh = [int]($h * 0.4)
    Send-Keys $process 'b'
    $xs = @(); $ys = @()
    for ($i = 0; $i -le 8; $i++) {
        $xs += $x + 10 + [int](($rw - 20) * $i / 8)
        $ys += if ($i % 2) { $y + $rh - 10 } else { $y + 10 }
    }
    [Native]::Drag($xs, $ys)
    for ($i = 0; $i -le 8; $i++) {
        $xs[$i] = $x + 10 + [int](($rw - 20) * (8 - $i) / 8) + 7
    }
    [Native]::Drag($xs, $ys)
    $press = @(($paper.Left + [int]($w * 0.1)), ($paper.Top + [int]($h * 0.1)))
    $lines = @()
    $flashed = @()
    foreach ($tool in @(@('pen', 'b', 0), @('wand', 'w', 60))) {
        Send-Keys $process $tool[1]
        [Native]::MoveTo($paper.Right - 5, $paper.Bottom - 5)
        # Playing on after the pen's press, it is stopped for the wand.
        if ($tool[0] -eq 'wand') {
            Send-Keys $process 'p'
            Start-Sleep -Milliseconds 500
        }
        Send-Keys $process 'p'
        Start-Sleep -Milliseconds 1500
        $counts = [Native]::PressAndWatch($press[0] + $tool[2], $press[1], $x, $y, $rw, $rh, 30, 1000)
        $flash = Test-Flash $counts ($rw * $rh)
        if ($flash.Low -lt 100) {
            throw "Too little drawn to watch: $counts"
        }
        # Playback goes on once the stroke or the click is over.
        Start-Sleep -Milliseconds 300
        $frame = Get-Value $process 'Current frame'
        Start-Sleep -Milliseconds 1000
        $later = Get-Value $process 'Current frame'
        $lines += "$($tool[0]): $($flash.Frames) frames after the press, $($flash.Flashes) flashed (fewest dark while playing $($flash.Low)), frame after release $frame -> $later 1 s later: $counts"
        if ($flash.Flashes) {
            $flashed += $tool[0]
        }
        if ($frame -eq $later) {
            $flashed += "$($tool[0]) (playback did not go on)"
        }
        Start-Sleep -Seconds 1
    }
    $shot = Save-Screen 'after-pressing-during-playback'
    Get-AppProcess | ForEach-Object { Stop-Process -Id $_.Id -Force }
    if ($flashed) {
        throw "flashed with $($flashed -join ', '); $($lines -join ' // '); $shot"
    }
    "$($lines -join ' // '); $shot"
}

# Issue #51: a large shape dragged with a fast mouse shows its last size
# soon after the pointer stops (evidence 58).
Step 'A bucket fill during playback plays on' {
    Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
    $process = Start-App
    $paper = Find-Paper $process
    $w = $paper.Right - $paper.Left
    $h = $paper.Bottom - $paper.Top
    # A closed box of pen strokes in the middle.
    $box = @(($paper.Left + [int]($w * 0.3)), ($paper.Top + [int]($h * 0.3)),
        ($paper.Left + [int]($w * 0.7)), ($paper.Top + [int]($h * 0.7)))
    Send-Keys $process 'b'
    [Native]::Drag(@($box[0], $box[2], $box[2], $box[0], $box[0]), @($box[1], $box[1], $box[3], $box[3], ($box[1] - 4)))
    Send-Keys $process 'g'
    [Native]::MoveTo($paper.Right - 5, $paper.Bottom - 5)
    Send-Keys $process 'p'
    Start-Sleep -Milliseconds 1500
    $before = Get-Value $process 'Current frame'
    Start-Sleep -Milliseconds 700
    $playing = (Get-Value $process 'Current frame') -ne $before
    # Click inside the box while it plays.
    [Native]::Drag(@([int](($box[0] + $box[2]) / 2), [int](($box[0] + $box[2]) / 2)), @([int](($box[1] + $box[3]) / 2), [int](($box[1] + $box[3]) / 2)))
    [Native]::MoveTo($paper.Right - 5, $paper.Bottom - 5)
    Start-Sleep -Milliseconds 300
    $frames = @()
    for ($i = 0; $i -lt 6; $i++) {
        $frames += Get-Value $process 'Current frame'
        Start-Sleep -Milliseconds 400
    }
    $shot = Save-Screen 'bucket-during-playback'
    Get-AppProcess | ForEach-Object { Stop-Process -Id $_.Id -Force }
    $moving = @($frames | Select-Object -Unique).Count -gt 1
    $detail = "playing before $playing; frames after the fill $($frames -join ', '); $shot"
    if (-not ($playing -and $moving)) {
        throw $detail
    }
    $detail
}

Step 'A dragged shape keeps up with the pointer' {
    Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
    $process = Start-App
    $paper = Find-Paper $process
    $w = $paper.Right - $paper.Left
    $h = $paper.Bottom - $paper.Top
    Send-Keys $process 'u'
    Invoke-Named $process 'Ellipse'
    $x0 = $paper.Left + [int]($w * 0.05)
    $y0 = $paper.Top + [int]($h * 0.05)
    $x1 = $paper.Left + [int]($w * 0.95)
    $y1 = $paper.Top + [int]($h * 0.95)
    # The far right of the ellipse at its last size, which no smaller one reaches.
    $edge = @(($x1 - 6), ([int](($y0 + $y1) / 2) - 6), 12, 12)
    $before = [Native]::DarkIn($edge[0], $edge[1], $edge[2], $edge[3])
    $times = foreach ($run in 1..3) {
        $ms = [Native]::DragAndWait($x0, $y0, $x1, $y1, 1500, $edge[0], $edge[1], $edge[2], $edge[3], 5000)
        Send-Keys $process '^z'
        Start-Sleep -Milliseconds 500
        $ms
    }
    Get-AppProcess | ForEach-Object { Stop-Process -Id $_.Id -Force }
    $detail = "paper $w x $h, dark before $before, last size shown after (ms): $($times -join ', ')"
    if ($before -gt 0 -or ($times | Where-Object { $_ -lt 0 -or $_ -gt 150 })) {
        throw $detail
    }
    $detail
}

# Issue #24: one round pixel pencil dot of 40 px at 100%, 75% and 300%, to
# compare with how another app shows its round pixel brush.
Step 'Round pixel dots at several zooms' {
    Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
    $process = Start-App
    $paper = Find-Paper $process
    Invoke-Named $process 'Edit'
    Invoke-Named $process 'Pixel mode'
    Start-Sleep -Milliseconds 500
    Send-Keys $process 'b'
    Invoke-Named $process 'Round Pixel Pencil'
    Set-Field $process 'Brush size' '40'
    $size = Get-Value $process 'Brush size'
    $words = Read-Words 'en'
    Set-Field $process $words['canvas-zoom-percent'] '100'
    $paper = Find-Paper $process
    $x = [int](($paper.Left + $paper.Right) / 2)
    $y = [int](($paper.Top + $paper.Bottom) / 2)
    [Native]::Drag(@($x, $x), @($y, $y))
    $shots = foreach ($zoom in 100, 75, 300) {
        Set-Field $process $words['canvas-zoom-percent'] "$zoom"
        $shown = Get-Value $process $words['canvas-zoom-percent']
        [Native]::MoveTo(5, 5)
        Start-Sleep -Milliseconds 300
        "$shown%: $(Save-Screen "round-dot-$zoom")"
    }
    Get-AppProcess | ForEach-Object { Stop-Process -Id $_.Id -Force }
    "brush size $size; $($shots -join '; ')"
}

Step 'Pixel mode from the Edit menu' {
    Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
    $process = Start-App
    Send-Keys $process 'b'
    $before = (Test-Named $process 'Ink Pen'), (Test-Named $process 'Pixel Pencil')
    Invoke-Named $process 'Edit'
    Invoke-Named $process 'Pixel mode'
    Start-Sleep -Milliseconds 500
    $on = (Test-Named $process 'Ink Pen'), (Test-Named $process 'Pixel Pencil')
    $tabs = Test-Named $process 'Airbrush'
    $shot = Save-Screen 'pixel-mode-brushes'
    Send-Keys $process 'g'
    $fillEdge = Test-Named $process 'Antialias fill edge'
    Send-Keys $process '^z'
    Send-Keys $process 'b'
    # The pixel pencil stays chosen, so its tab is shown; the tabs are back.
    $after = (Test-Named $process 'Airbrush'), (Test-Named $process 'Pixel Pencil')
    Get-AppProcess | ForEach-Object { Stop-Process -Id $_.Id -Force }
    $detail = "before: ink $($before[0]), pixel $($before[1]); on: ink $($on[0]), pixel $($on[1]), tabs $tabs, fill edge choice $fillEdge; undone: tabs $($after[0]), pixel $($after[1]); $shot"
    if (-not $before[0] -or $on[0] -or -not $on[1] -or $tabs -or $fillEdge -or -not $after[0]) {
        throw $detail
    }
    $detail
}

Step 'Shapes and the round pixel pencil' {
    Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
    $process = Start-App
    $paper = Find-Paper $process
    $w = $paper.Right - $paper.Left
    $h = $paper.Bottom - $paper.Top
    Send-Keys $process 'u'
    Invoke-Named $process 'Star'
    $points = Test-Named $process 'Points'
    [Native]::Drag(@(($paper.Left + [int]($w * 0.1)), ($paper.Left + [int]($w * 0.3)), ($paper.Left + [int]($w * 0.45))),
        @(($paper.Top + [int]($h * 0.1)), ($paper.Top + [int]($h * 0.3)), ($paper.Top + [int]($h * 0.45))))
    $drawn = Test-Named $process 'Untitled*'
    Invoke-Named $process 'Polygon'
    $sides = Test-Named $process 'Sides'
    [Native]::Drag(@(($paper.Left + [int]($w * 0.55)), ($paper.Left + [int]($w * 0.9))),
        @(($paper.Top + [int]($h * 0.1)), ($paper.Top + [int]($h * 0.45))))
    $shapes = Save-Screen 'shapes'
    Invoke-Named $process 'Edit'
    Invoke-Named $process 'Pixel mode'
    Start-Sleep -Milliseconds 500
    Send-Keys $process 'b'
    Invoke-Named $process 'Round Pixel Pencil'
    [Native]::Drag(@(($paper.Left + [int]($w * 0.2)), ($paper.Left + [int]($w * 0.8))),
        @(($paper.Top + [int]($h * 0.75)), ($paper.Top + [int]($h * 0.8))))
    $round = Save-Screen 'round-pixel-pencil'
    Get-AppProcess | ForEach-Object { Stop-Process -Id $_.Id -Force }
    $detail = "star points control $points, drawn $drawn, polygon sides control $sides; $shapes; $round"
    if (-not ($points -and $drawn -and $sides)) {
        throw $detail
    }
    $detail
}

Step 'Filled shapes' {
    Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
    $process = Start-App
    $paper = Find-Paper $process
    $w = $paper.Right - $paper.Left
    $h = $paper.Bottom - $paper.Top
    Send-Keys $process 'u'
    Invoke-Named $process 'Moon'
    Invoke-Named $process 'Both'
    $moon = @(($paper.Left + [int]($w * 0.05)), ($paper.Top + [int]($h * 0.1)), ($paper.Left + [int]($w * 0.45)), ($paper.Top + [int]($h * 0.9)))
    [Native]::Drag(@($moon[0], $moon[2]), @($moon[1], $moon[3]))
    Invoke-Named $process 'Trapezoid'
    Invoke-Named $process 'Fill'
    $trapezoid = @(($paper.Left + [int]($w * 0.55)), ($paper.Top + [int]($h * 0.2)), ($paper.Left + [int]($w * 0.95)), ($paper.Top + [int]($h * 0.8)))
    [Native]::Drag(@($trapezoid[0], $trapezoid[2]), @($trapezoid[1], $trapezoid[3]))
    Invoke-Named $process 'Star'
    $inner = Test-Named $process 'Inner corners'
    [Native]::MoveTo($paper.Right + 40, $paper.Bottom + 40)
    Start-Sleep -Milliseconds 700
    $shot = Save-Screen 'filled-shapes'
    $bounds = [Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bitmap = New-Object Drawing.Bitmap $bounds.Width, $bounds.Height
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($bounds.Location, [Drawing.Point]::Empty, $bounds.Size)
    $dark = {
        param($x, $y)
        $c = $bitmap.GetPixel([int]$x, [int]$y)
        $c.R + $c.G + $c.B -lt 200
    }
    # Inside the thick left part of the moon, its bite and the trapezoid's middle.
    $moonInside = & $dark ($moon[0] + ($moon[2] - $moon[0]) * 0.15) (($moon[1] + $moon[3]) / 2)
    $bite = & $dark ($moon[0] + ($moon[2] - $moon[0]) * 0.8) (($moon[1] + $moon[3]) / 2)
    $trapezoidInside = & $dark (($trapezoid[0] + $trapezoid[2]) / 2) (($trapezoid[1] + $trapezoid[3]) / 2)
    $graphics.Dispose()
    $bitmap.Dispose()
    Get-AppProcess | ForEach-Object { Stop-Process -Id $_.Id -Force }
    $detail = "moon filled $moonInside, bite empty $(-not $bite), trapezoid filled $trapezoidInside, star inner control $inner; $shot"
    if (-not ($moonInside -and -not $bite -and $trapezoidInside -and $inner)) {
        throw $detail
    }
    $detail
}

# Presses the control whose name starts with the words for $Key, without a
# trailing ellipsis: menu items carry their shortcut after the label.
function Invoke-Word($Process, $Words, [string] $Key) {
    Invoke-Named $Process ($Words[$Key].TrimEnd([char]0x2026)) -Prefix
}

# Turns on View > Animation tools, which start off; the settings file a
# step before saved may have them on already.
function Enable-AnimationTools($Process, [string] $Language = 'en') {
    $words = Read-Words $Language
    Invoke-Word $Process $words 'menu-view'
    $name = $words['view-animation-tools']
    $item = Find-Pattern $Process $name @([Windows.Automation.SelectionItemPattern]::Pattern,
        [Windows.Automation.TogglePattern]::Pattern)
    $on = if ($item -is [Windows.Automation.SelectionItemPattern]) { $item.Current.IsSelected }
        else { $item.Current.ToggleState -eq [Windows.Automation.ToggleState]::On }
    if ($on) {
        Send-Keys $Process '{ESC}'
    } else {
        Invoke-Named $Process $name
    }
}

# Draws a stroke across the middle of the document.
function Draw-Stroke($Process) {
    $paper = Find-Paper $Process
    $w = $paper.Right - $paper.Left
    $h = $paper.Bottom - $paper.Top
    Send-Keys $Process 'b'
    [Native]::Drag(@(($paper.Left + [int]($w * 0.2)), ($paper.Left + [int]($w * 0.5)), ($paper.Left + [int]($w * 0.8))),
        @(($paper.Top + [int]($h * 0.6)), ($paper.Top + [int]($h * 0.3)), ($paper.Top + [int]($h * 0.6))))
    $paper
}

# Where the dark pixels inside $Paper are on the screen now: their middle
# along x and how many, counting every second pixel each way; and how many
# pixels are reddish, as an onion skin's frames before.
function Get-Ink($Process, $Paper) {
    [Native]::MoveTo($Paper.Right + 40, $Paper.Bottom + 40)
    Start-Sleep -Milliseconds 700
    $bounds = [Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bitmap = New-Object Drawing.Bitmap $bounds.Width, $bounds.Height
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($bounds.Location, [Drawing.Point]::Empty, $bounds.Size)
    $sum = 0; $count = 0; $red = 0
    for ($y = $Paper.Top + 2; $y -lt $Paper.Bottom - 2; $y += 2) {
        for ($x = $Paper.Left + 2; $x -lt $Paper.Right - 2; $x += 2) {
            $c = $bitmap.GetPixel($x, $y)
            if ($c.R + $c.G + $c.B -lt 200) { $sum += $x; $count++ }
            elseif ($c.R -gt $c.B + 25 -and $c.R -gt $c.G + 25) { $red++ }
        }
    }
    $graphics.Dispose()
    $bitmap.Dispose()
    $centre = if ($count) { [int]($sum / $count) } else { -1 }
    [pscustomobject]@{ Centre = $centre; Count = $count; Red = $red }
}

Step 'Pixel art exports at its own size' {
    Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
    Remove-Item -Force (Join-Path $env:TEMP 'gui-settings.json') -ErrorAction SilentlyContinue
    $process = Start-App
    $words = Read-Words 'en'
    Invoke-Word $process $words 'menu-edit'
    Invoke-Word $process $words 'pixel-mode'
    Start-Sleep -Milliseconds 500
    $sizes = foreach ($menu in 'file-export-frame', 'export-gif') {
        Invoke-Word $process $words 'menu-file'
        Invoke-Word $process $words $menu
        Start-Sleep -Milliseconds 500
        $value = (Find-Pattern $process $words['export-size'] @([Windows.Automation.ValuePattern]::Pattern)).Current.Value
        $shot = Save-Screen "pixel-$menu"
        Send-Keys $process '{ESC}'
        Start-Sleep -Milliseconds 300
        [pscustomobject]@{ Menu = $menu; Value = $value; Shot = $shot }
    }
    Stop-Apps
    $detail = ($sizes | ForEach-Object { "$($_.Menu): '$($_.Value)' $($_.Shot)" }) -join '; '
    if ($sizes | Where-Object { -not $_.Value.StartsWith($words['export-size-original']) }) {
        throw $detail
    }
    $detail
}

Step 'Symmetry ruler' {
    Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
    $process = Start-App
    $paper = Find-Paper $process
    $w = $paper.Right - $paper.Left
    $h = $paper.Bottom - $paper.Top
    $middle = $paper.Left + [int]($w / 2)
    # Dark pixels right of the ruler's line, which is left out.
    $right = {
        [Native]::MoveTo($paper.Right + 40, $paper.Bottom + 40)
        Start-Sleep -Milliseconds 700
        $bounds = [Windows.Forms.Screen]::PrimaryScreen.Bounds
        $bitmap = New-Object Drawing.Bitmap $bounds.Width, $bounds.Height
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        $graphics.CopyFromScreen($bounds.Location, [Drawing.Point]::Empty, $bounds.Size)
        $count = 0
        for ($y = $paper.Top + 2; $y -lt $paper.Bottom - 2; $y += 2) {
            for ($x = $middle + 15; $x -lt $paper.Right - 2; $x += 2) {
                $c = $bitmap.GetPixel($x, $y)
                if ($c.R + $c.G + $c.B -lt 200) { $count++ }
            }
        }
        $graphics.Dispose()
        $bitmap.Dispose()
        $count
    }
    # The ruler tool: a drag down the middle lays an upright line.
    Send-Keys $process 'r'
    Start-Sleep -Milliseconds 300
    $top = $paper.Top + [int]($h * 0.2)
    [Native]::Drag(@($middle, $middle), @($top, ($paper.Top + [int]($h * 0.6))))
    Start-Sleep -Milliseconds 300
    $switch = Find-Pattern $process 'Use the symmetry ruler' @([Windows.Automation.TogglePattern]::Pattern)
    $on = $switch.Current.ToggleState -eq [Windows.Automation.ToggleState]::On
    $placed = Save-Screen 'symmetry-placed'
    $before = & $right
    Send-Keys $process 'b'
    Start-Sleep -Milliseconds 300
    $inBrush = Test-Named $process 'Use the symmetry ruler'
    # On the left half only.
    [Native]::Drag(@(($paper.Left + [int]($w * 0.1)), ($paper.Left + [int]($w * 0.25)), ($paper.Left + [int]($w * 0.4))),
        @(($paper.Top + [int]($h * 0.2)), ($paper.Top + [int]($h * 0.35)), ($paper.Top + [int]($h * 0.25))))
    $after = & $right
    $mirrored = Save-Screen 'symmetry-mirror'
    # From the centre, the ruler tool moves it.
    Send-Keys $process 'r'
    Start-Sleep -Milliseconds 300
    $x = Get-Value $process 'Centre X'
    [Native]::Drag(@($middle, ($middle + [int]($w * 0.2))), @($top, $top))
    Start-Sleep -Milliseconds 300
    $moved = Get-Value $process 'Centre X'
    Set-Field $process 'Number of symmetry lines' '6'
    Send-Keys $process 'b'
    [Native]::Drag(@(($paper.Left + [int]($w * 0.45)), ($paper.Left + [int]($w * 0.48))),
        @(($paper.Top + [int]($h * 0.6)), ($paper.Top + [int]($h * 0.85))))
    $six = Save-Screen 'symmetry-six-lines'
    Get-AppProcess | ForEach-Object { Stop-Process -Id $_.Id -Force }
    $detail = "placed on $on, switch under the brush $inBrush, dark pixels right of the line $before -> $after, centre x $x -> $moved; $placed; $mirrored; $six"
    if (-not $on -or $inBrush -or $before -gt 20 -or $after -lt 50 -or $moved - $x -lt 150) {
        throw $detail
    }
    $detail
}

Step 'Keyframes' {
    Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
    $process = Start-App
    Enable-AnimationTools $process
    $paper = Draw-Stroke $process
    $w = $paper.Right - $paper.Left
    $x = [int](($paper.Left + $paper.Right) / 2)
    $y = [int](($paper.Top + $paper.Bottom) / 2)
    $drawn = Get-Ink $process $paper
    # Frame 1: dragged right with the move tool.
    Send-Keys $process 'v'
    [Native]::Drag(@($x, ($x + [int]($w * 0.12)), ($x + [int]($w * 0.25))), @($y, $y, $y))
    $first = Get-Ink $process $paper
    # Frame 16: dragged back left.
    for ($i = 0; $i -lt 15; $i++) { Send-Keys $process '.' }
    $frame = Get-Value $process 'Current frame'
    [Native]::Drag(@($x, ($x - [int]($w * 0.12)), ($x - [int]($w * 0.25))), @($y, $y, $y))
    $later = Get-Ink $process $paper
    $row = Test-Named $process 'Keys of Layer 1: 2'
    $shot = Save-Screen 'keyframes'
    # Halfway between, on frame 8, the drawing is between the two.
    for ($i = 0; $i -lt 8; $i++) { Send-Keys $process ',' }
    $between = Get-Ink $process $paper
    Get-AppProcess | ForEach-Object { Stop-Process -Id $_.Id -Force }
    Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
    $detail = "ink middle x: drawn $($drawn.Centre), frame 1 $($first.Centre), frame $frame $($later.Centre), frame 8 $($between.Centre); timeline row $row; $shot"
    $moved = $first.Centre - $drawn.Centre
    if ($frame -ne 16 -or -not $row -or $moved -lt $w * 0.2 -or [Math]::Abs($later.Centre - $drawn.Centre) -gt $w * 0.05 -or
        -not ($between.Centre -gt $later.Centre -and $between.Centre -lt $first.Centre)) {
        throw $detail
    }
    $detail
}

Step 'Cels and onion skin' {
    Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
    $process = Start-App
    Enable-AnimationTools $process
    $paper = Draw-Stroke $process
    $w = $paper.Right - $paper.Left
    $h = $paper.Bottom - $paper.Top
    Invoke-Named $process 'Make animation folder'
    for ($i = 0; $i -lt 3; $i++) { Send-Keys $process '.' }
    Send-Keys $process '^+n'
    Send-Keys $process 'b'
    [Native]::Drag(@(($paper.Left + [int]($w * 0.2)), ($paper.Left + [int]($w * 0.8))),
        @(($paper.Top + [int]($h * 0.2)), ($paper.Top + [int]($h * 0.25))))
    $row = Test-Named $process 'Cels of Animation 1, Cel 3 on this frame'
    $plain = Get-Ink $process $paper
    Send-Keys $process 'o'
    Start-Sleep -Milliseconds 800
    $onion = Get-Ink $process $paper
    $shot = Save-Screen 'cels-onion-skin'
    for ($i = 0; $i -lt 3; $i++) { Send-Keys $process ',' }
    $back = Test-Named $process 'Cels of Animation 1, Layer 1 on this frame'
    Get-AppProcess | ForEach-Object { Stop-Process -Id $_.Id -Force }
    Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
    $detail = "cel row on frame 4 $row, back on frame 1 $back; red pixels without onion skin $($plain.Red), with $($onion.Red); $shot"
    if (-not ($row -and $back) -or $onion.Red -le $plain.Red + 20) {
        throw $detail
    }
    $detail
}

Step 'Key opacity, pivot and onion settings' {
    Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
    $process = Start-App
    Enable-AnimationTools $process
    $paper = Draw-Stroke $process
    $drawn = Get-Ink $process $paper
    Send-Keys $process 'v'
    Send-Keys $process 'k'
    $pivot = Test-Named $process 'Pivot X'
    Set-Field $process 'Key opacity' '30'
    $faded = Get-Ink $process $paper
    $shot = Save-Screen 'key-opacity-pivot'
    $root = [Windows.Automation.AutomationElement]::FromHandle($process.MainWindowHandle)
    $button = $root.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition) |
        Where-Object { ([string]$_.Current.Name) -like '*Onion skin' } | Select-Object -First 1
    $rect = $button.Current.BoundingRectangle
    [Native]::MoveTo([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
    Start-Sleep -Milliseconds 300
    [Native]::RightClick()
    Start-Sleep -Milliseconds 500
    $popup = Test-Named $process 'Frames before'
    $settings = Save-Screen 'onion-settings'
    Get-AppProcess | ForEach-Object { Stop-Process -Id $_.Id -Force }
    Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
    $detail = "dark pixels drawn $($drawn.Count), at 30% $($faded.Count); pivot field $pivot; onion settings $popup; $shot; $settings"
    if (-not ($pivot -and $popup) -or $drawn.Count -eq 0 -or $faded.Count -gt $drawn.Count / 4) {
        throw $detail
    }
    $detail
}

# At 100% so the glyphs are seen at their real size: the move tool, keys,
# onion skin, animation folder and new cel.
Step 'Animation icons at 100%' {
    $env:UGURUGU_UI_SCALE = '100'
    try {
        Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
        $process = Start-App
        Enable-AnimationTools $process
        [void](Draw-Stroke $process)
        Invoke-Named $process 'Make animation folder'
        Send-Keys $process 'v'
        Start-Sleep -Milliseconds 500
        $shot = Save-Screen 'animation-icons'
    } finally {
        Remove-Item Env:UGURUGU_UI_SCALE
        Get-AppProcess | ForEach-Object { Stop-Process -Id $_.Id -Force }
        Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
    }
    "move tool, key buttons, timeline and layer list; $shot"
}

Step 'Canvas size by dragging the frame' {
    Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
    $process = Start-App
    $words = Read-Words 'en'
    Invoke-Word $process $words 'menu-edit'
    Invoke-Word $process $words 'edit-canvas-size'
    $width = Get-Value $process $words['canvas-size-width']
    $height = Get-Value $process $words['canvas-size-height']
    $paper = Find-Paper $process -Below
    $my = [int](($paper.Top + $paper.Bottom) / 2)
    [Native]::MoveTo($paper.Right, $my)
    Start-Sleep -Milliseconds 300
    [Native]::MoveTo($paper.Right + 1, $my)
    Start-Sleep -Milliseconds 300
    $cursor = [Native]::CursorName()
    # A quarter of the document's width further right.
    $by = [int](($paper.Right - $paper.Left) / 4)
    [Native]::Drag(@($paper.Right, ($paper.Right + $by)), @($my, $my))
    [Native]::MoveTo($paper.Right + $by + 60, $paper.Bottom + 60)
    Start-Sleep -Milliseconds 700
    $dragged = Get-Value $process $words['canvas-size-width']
    $shot = Save-Screen 'canvas-size-frame'
    $bounds = [Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bitmap = New-Object Drawing.Bitmap $bounds.Width, $bounds.Height
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($bounds.Location, [Drawing.Point]::Empty, $bounds.Size)
    $added = $bitmap.GetPixel($paper.Right + [int]($by / 2), $my)
    $graphics.Dispose()
    $bitmap.Dispose()
    $paperShown = "$($added.R),$($added.G),$($added.B)" -eq $paper.Colour
    # The window is in front from the drag; Send-Keys taps Alt first, which
    # opens the menu bar.
    [Windows.Forms.SendKeys]::SendWait('{ENTER}')
    Start-Sleep -Seconds 1
    $after = Find-Paper $process
    $applied = Save-Screen 'canvas-size-applied'
    Get-AppProcess | ForEach-Object { Stop-Process -Id $_.Id -Force }
    Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
    $wanted = $width * 1.25
    $ratio = ($after.Right - $after.Left) / ($after.Bottom - $after.Top)
    $detail = "edge cursor $cursor; width $width -> $dragged (about $([int]$wanted)), height $height; " +
        "added area in the paper colour $paperShown; applied ratio $([Math]::Round($ratio, 3)) " +
        "(wanted $([Math]::Round($dragged / $height, 3))); $shot; $applied"
    $close = { param($a, $b, $share) [Math]::Abs($a - $b) -le $b * $share }
    if (-not ($cursor -eq 'sizewe' -and (& $close $dragged $wanted 0.03) -and $paperShown -and
            (& $close $ratio ($dragged / $height) 0.02))) {
        throw $detail
    }
    $detail
}

# Photographs every panel, menu and dialog in each of $Languages; the files
# are named with $Tag first.
function Invoke-ScreenSet([string[]] $Languages, [string] $Tag = '') {
    $taken = @()
    $missed = @()
    foreach ($language in $Languages) {
        $words = Read-Words $language
        # Each shot starts from a fresh window, so one that fails leaves
        # nothing open for the next. Settings open with Ctrl+,, and their
        # pages are pressed by the whole name: "Files" in Korean and Japanese
        # starts the File menu's name too.
        $shots = [ordered]@{
            'main' = { param($p) }
            'menu-file' = { param($p) Invoke-Word $p $words 'menu-file' }
            'menu-edit' = { param($p) Invoke-Word $p $words 'menu-edit' }
            'menu-view' = { param($p) Invoke-Word $p $words 'menu-view' }
            'menu-tools' = { param($p) Invoke-Word $p $words 'menu-tools' }
            'menu-window' = { param($p) Invoke-Word $p $words 'menu-window' }
            'menu-help' = { param($p) Invoke-Word $p $words 'menu-help' }
            'tool-eraser' = { param($p) Send-Keys $p 'e' }
            'tool-select' = { param($p) Send-Keys $p 'l' }
            'tool-wand' = { param($p) Send-Keys $p 'w' }
            'tool-fill' = { param($p) Send-Keys $p 'g' }
            'tool-text' = { param($p) Send-Keys $p 't' }
            'tool-eyedropper' = { param($p) Send-Keys $p 'i' }
            'tool-shape' = { param($p) Send-Keys $p 'u' }
            'tool-symmetry' = { param($p) Send-Keys $p 'r' }
            'tool-move-timeline' = {
                param($p)
                Enable-AnimationTools $p $language
                $paper = Draw-Stroke $p
                Send-Keys $p 'v'
                $x = [int](($paper.Left + $paper.Right) / 2)
                $y = [int](($paper.Top + $paper.Bottom) / 2)
                [Native]::Drag(@($x, ($x + 60)), @($y, ($y + 20)))
                Invoke-Word $p $words 'menu-view'
                Invoke-Word $p $words 'animation-folder-make'
                Send-Keys $p 'o'
            }
            'pixel-mode' = { param($p) Invoke-Word $p $words 'menu-edit'; Invoke-Word $p $words 'pixel-mode' }
            'settings-general' = { param($p) Send-Keys $p '^,' }
            'settings-drawing' = { param($p) Send-Keys $p '^,'; Invoke-Named $p $words['settings-drawing'] }
            'settings-files' = { param($p) Send-Keys $p '^,'; Invoke-Named $p $words['settings-files'] }
            'settings-shortcuts' = { param($p) Send-Keys $p '^,'; Invoke-Named $p $words['settings-shortcuts'] }
            'settings-about' = { param($p) Invoke-Word $p $words 'menu-help'; Invoke-Word $p $words 'help-about' }
            'new-document' = { param($p) Send-Keys $p '^n' }
            'image-size' = { param($p) Invoke-Word $p $words 'menu-edit'; Invoke-Word $p $words 'edit-image-size' }
            'canvas-size' = { param($p) Invoke-Word $p $words 'menu-edit'; Invoke-Word $p $words 'edit-canvas-size' }
            'export-gif' = { param($p) Invoke-Word $p $words 'menu-file'; Invoke-Word $p $words 'export-gif' }
            'export-webp' = { param($p) Invoke-Word $p $words 'menu-file'; Invoke-Word $p $words 'export-webp' }
            'check-updates' = { param($p) Invoke-Word $p $words 'menu-help'; Invoke-Word $p $words 'help-check-updates'; Start-Sleep -Seconds 2 }
            'help' = { param($p) Send-Keys $p '{F1}'; Start-Sleep -Seconds 1 }
            'unsaved-changes' = { param($p) [void](Draw-Stroke $p); Send-Keys $p '^w' }
            'transform-box' = { param($p) [void](Draw-Stroke $p); Send-Keys $p '^a'; Send-Keys $p '^t' }
            'restyle' = { param($p) [void](Draw-Stroke $p); Send-Keys $p '^a'; Invoke-Word $p $words 'menu-edit'; Invoke-Word $p $words 'restyle-selected' }
            'text-placed' = {
                param($p)
                $paper = Find-Paper $p
                Send-Keys $p 't'
                [Native]::Drag(@(($paper.Left + 80), ($paper.Left + 80)), @(($paper.Top + 80), ($paper.Top + 80)))
            }
        }
        foreach ($shot in $shots.GetEnumerator()) {
            Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
            Remove-Item -Force (Join-Path $env:TEMP 'gui-settings.json') -ErrorAction SilentlyContinue
            try {
                $process = Start-App -Language $language
                & $shot.Value $process
                Start-Sleep -Milliseconds 700
                $taken += Save-Screen "$Tag$language-$($shot.Key)"
            } catch {
                $missed += "$Tag$language-$($shot.Key): $($_.Exception.Message)"
            }
            Stop-Apps
            Start-Sleep -Milliseconds 500
        }
        # Work left by a window closed by force is offered back at the next
        # start.
        Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
        try {
            $process = Start-App -Language $language
            [void](Draw-Stroke $process)
            Start-Sleep -Seconds 5
            Stop-Apps
            Start-Sleep -Seconds 1
            $process = Start-App -Language $language
            [void](Find-Element $process { param($n) $n -eq $words['recovery-title'] } 20 'the recovery notice')
            $taken += Save-Screen "$Tag$language-recovery"
        } catch {
            $missed += "$Tag$language-recovery: $($_.Exception.Message)"
        }
        Stop-Apps
    }
    if ($missed) {
        throw "missed $($missed.Count): $($missed -join ' // '); took $($taken.Count)"
    }
    "took $($taken.Count): $($taken -join ', ')"
}

Step 'Screen set' { Invoke-ScreenSet @('ko', 'en') }

# The Sandbox's display takes the host's scale and cannot be changed, so the
# app draws its interface at 100% itself (UGURUGU_UI_SCALE): the pixels of a
# 100% display. Text is laid out in points, so each language overflows or not
# alike at any scale.
Step 'Screen set at 100%' {
    $env:UGURUGU_UI_SCALE = '100'
    try {
        Invoke-ScreenSet @('ko', 'en', 'ja') '100-'
    } finally {
        Remove-Item Env:UGURUGU_UI_SCALE
    }
}

Step 'Colour triangle' {
    Stop-Apps
    Remove-Item -Force (Join-Path $env:TEMP 'gui-settings.json') -ErrorAction SilentlyContinue
    $taken = @()
    # At 100%, so the whole colour panel fits the window.
    $env:UGURUGU_UI_SCALE = '100'
    foreach ($language in @('ko', 'en')) {
        $words = Read-Words $language
        $process = Start-App -Language $language
        Invoke-Word $process $words 'color-field-triangle'
        Start-Sleep -Milliseconds 700
        $taken += Save-Screen "$language-color-triangle"
        Stop-Apps
        Start-Sleep -Milliseconds 500
    }
    Remove-Item Env:UGURUGU_UI_SCALE
    "took $($taken -join ', ')"
}

Step 'Every tab comes back after a crash' {
    $recovery = Join-Path $env:TEMP 'gui-recovery'
    Stop-Apps
    Remove-Item -Recurse -Force $recovery -ErrorAction SilentlyContinue
    $process = Start-App
    [void](Draw-Stroke $process)
    # A second tab, at the size offered, with a stroke of its own.
    Send-Keys $process '^n'
    Send-Keys $process '{ENTER}'
    Start-Sleep -Seconds 1
    [void](Draw-Stroke $process)
    # Written 3 s after editing pauses, the tab not shown too.
    Start-Sleep -Seconds 6
    $written = @(Get-ChildItem -Path $recovery -Recurse -Filter '*.ugurugu' |
        ForEach-Object { Split-Path -Leaf $_.DirectoryName } | Sort-Object -Unique)
    Stop-Process -Id $process.Id -Force
    Start-Sleep -Seconds 1
    $crashed = @(Get-ChildItem $recovery -Directory | ForEach-Object { $_.Name })
    $again = Start-App
    # Each document is asked about in turn.
    foreach ($document in 1..2) {
        Press $again 'Recover' 20
        Start-Sleep -Seconds 1
    }
    # Written again here 3 s after.
    Start-Sleep -Seconds 6
    $own = @(Get-ChildItem $recovery -Directory | Where-Object { $_.Name -notin $crashed })
    $tabs = @(foreach ($folder in $own) {
            Get-ChildItem $folder.FullName -Directory | Where-Object { Get-ChildItem $_.FullName -Filter '*.ugurugu' }
        })
    $left = @(Get-ChildItem $recovery -Directory | Where-Object { $_.Name -in $crashed }).Count
    $shot = Save-Screen 'tabs-recovered'
    Stop-Apps
    $detail = "written before the crash: $($written -join ', '); recovered and written again: $($tabs.Count); crashed folders left: $left; $shot"
    if ($written.Count -ne 2 -or $tabs.Count -ne 2 -or $left -ne 0) {
        throw $detail
    }
    $detail
}

Step 'A tab keeps its drawing while another is shown' {
    Stop-Apps
    Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
    $process = Start-App
    $paper = Draw-Stroke $process
    Start-Sleep -Seconds 1
    $area = @($paper.Left, $paper.Top, ($paper.Right - $paper.Left), ($paper.Bottom - $paper.Top))
    $drawn = [Native]::DarkIn($area[0], $area[1], $area[2], $area[3])
    Send-Keys $process '^n'
    Send-Keys $process '{ENTER}'
    Start-Sleep -Seconds 1
    $empty = [Native]::DarkIn($area[0], $area[1], $area[2], $area[3])
    Send-Keys $process '^+{TAB}'
    Start-Sleep -Seconds 1
    $back = [Native]::DarkIn($area[0], $area[1], $area[2], $area[3])
    $shot = Save-Screen 'tab-back'
    Stop-Apps
    $detail = "dark pixels: drawn $drawn, new tab $empty, back $back; $shot"
    # The new tab's canvas may be another size, with the dark surround in
    # the area, so it only has to look different.
    if ($drawn -lt 100 -or $empty -eq $drawn -or [Math]::Abs($back - $drawn) * 50 -gt $drawn) {
        throw $detail
    }
    $detail
}

Step 'Tab moves the keyboard focus through the controls' {
    Stop-Apps
    Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
    Remove-Item -Force (Join-Path $env:TEMP 'gui-settings.json') -ErrorAction SilentlyContinue
    $process = Start-App
    $walks = @()
    foreach ($place in @('main', 'settings')) {
        if ($place -eq 'settings') {
            Send-Keys $process '^,'
            Start-Sleep -Seconds 1
        }
        # Send-Keys taps Alt to take the front, which puts the focus on the
        # menu bar; Escape leaves it there, and settings, which Escape would
        # close, are in front already. Then Tab alone is sent.
        if ($place -eq 'main') {
            Send-Keys $process '{ESC}'
        }
        $names = @()
        foreach ($press in 1..14) {
            [Windows.Forms.SendKeys]::SendWait('{TAB}')
            Start-Sleep -Milliseconds 300
            $names += [string][Windows.Automation.AutomationElement]::FocusedElement.Current.Name
        }
        $walks += [pscustomobject]@{ Place = $place; Names = $names }
    }
    $shot = Save-Screen 'tab-focus'
    Stop-Apps
    $detail = ($walks | ForEach-Object { "$($_.Place): $($_.Names -join ' > ')" }) -join ' // '
    foreach ($walk in $walks) {
        if (@($walk.Names | Where-Object { $_ } | Sort-Object -Unique).Count -lt 5) {
            throw "focus stays put; $detail; $shot"
        }
    }
    "$detail; $shot"
}

Step 'A file opened from outside becomes a tab of the open window' {
    Stop-Apps
    Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
    $document = Join-Path $env:TEMP 'handed.ugurugu'
    & (Join-Path $in 'ugu-doc.exe') fixture 1 $document | Out-Null
    $process = Start-App
    [void][Native]::ShowWindow($process.MainWindowHandle, 6)
    Start-Sleep -Seconds 1
    $launch = Start-Process -FilePath $app -ArgumentList "`"$document`"" -PassThru
    $exited = $launch.WaitForExit(10000)
    Start-Sleep -Seconds 1
    $windows = @(Get-AppProcess).Count
    $restored = -not [Native]::IsIconic($process.MainWindowHandle)
    $front = [Native]::GetForegroundWindow() -eq $process.MainWindowHandle
    $tab = [bool](Find-Element $process { param($n) $n -like 'handed*' } 15 'the new tab')
    $shot = Save-Screen 'handed-over'
    Stop-Apps
    $detail = "launcher exited: $exited (code $(if ($exited) { $launch.ExitCode })); windows: $windows; restored: $restored; in front: $front; tab: $tab; $shot"
    if (-not $exited -or $launch.ExitCode -ne 0 -or $windows -ne 1 -or -not $restored -or -not $front -or -not $tab) {
        throw $detail
    }
    $detail
}

# What the app spends while its drawing wobbles on screen, minimized and
# under another window (docs/rust/evidence.md section 65).
Step 'CPU while the window is hidden' {
    Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
    $process = Start-App
    $null = Draw-Stroke $process
    Start-Sleep -Seconds 2
    $spent = {
        $process.Refresh()
        $before = $process.TotalProcessorTime
        Start-Sleep -Seconds 5
        $process.Refresh()
        [int](($process.TotalProcessorTime - $before).TotalMilliseconds)
    }
    $shown = & $spent
    [void][Native]::ShowWindow($process.MainWindowHandle, 6)
    Start-Sleep -Seconds 1
    $minimized = & $spent
    [void][Native]::ShowWindow($process.MainWindowHandle, 9)
    Start-Sleep -Seconds 1
    $cover = New-Object Windows.Forms.Form
    $cover.WindowState = 'Maximized'
    $cover.TopMost = $true
    $cover.Show()
    Start-Sleep -Seconds 1
    $covered = & $spent
    $cover.Close()
    Get-AppProcess | ForEach-Object { Stop-Process -Id $_.Id -Force }
    Remove-Item -Recurse -Force (Join-Path $env:TEMP 'gui-recovery') -ErrorAction SilentlyContinue
    "CPU ms in 5 s while wobbling: shown $shown, minimized $minimized, under a maximized window $covered"
}

Step 'The window opens where it closed' {
    Stop-Apps
    Remove-Item -Force (Join-Path $env:TEMP 'gui-settings.json') -ErrorAction SilentlyContinue
    $process = Start-App
    [void][Native]::SetWindowPos($process.MainWindowHandle, [IntPtr]::Zero, 300, 200, 1100, 700, 4)
    Start-Sleep -Seconds 1
    Close-App $process
    $reopen = {
        Start-Process -FilePath $app | Out-Null
        $process = Wait-Window
        [void](Get-Value $process (Read-Words 'en')['canvas-zoom-percent'])
        Start-Sleep -Seconds 1
        $process
    }
    $process = & $reopen
    $first = Format-Rect (Get-Rect $process)
    [void][Native]::ShowWindow($process.MainWindowHandle, 3)
    Start-Sleep -Seconds 1
    Close-App $process
    $process = & $reopen
    $maximized = [Native]::IsZoomed($process.MainWindowHandle)
    [void][Native]::ShowWindow($process.MainWindowHandle, 9)
    Start-Sleep -Seconds 1
    $restored = Format-Rect (Get-Rect $process)
    $shot = Save-Screen 'window-place'
    Close-App $process
    $expected = '300,200,1400,900'
    $detail = "reopened at $first; maximized again: $maximized; restored to $restored; $shot"
    if ($first -ne $expected -or -not $maximized -or $restored -ne $expected) {
        throw $detail
    }
    $detail
}

if (Test-Path $logs) {
    Write-Host "Logs in $logs"
}
Write-Host "Done: $(Join-Path $out 'results.md')"
