# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

<#
.SYNOPSIS
Synthetic pen and touch input to 3.0, run inside Windows Sandbox
(docs/rust/evidence.md section 33).

.DESCRIPTION
Started at logon by the .wsb that prepare.ps1 -Scenario pointer writes,
with C:\ugu\in (ugurugu.exe, ugu-doc.exe; read-only) and C:\ugu\out
(results) mapped. Synthetic input (InjectSyntheticPointerInput) goes to
the window under each point, so it runs only here, where the app is the
only window; each frame still checks that the app is in front and under
the point. Frames carry a frame id and time, as a device's do. Pen: rising
pressure, the barrel button held, hovering, a release outside the window
and 1000 samples a second, read back from the recovery file with ugu-doc
strokes, and what pen flags reach a plain window. Touch: two-finger pan,
pinch and turn and a one-finger drag, read from the status bar's zoom and
turn.
Runs in Windows PowerShell 5.1.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$title = 'Synthetic pen and touch'
$in = 'C:\ugu\in'
$out = 'C:\ugu\out'
$installRoot = $in
$logs = Join-Path $out 'logs'
$recovery = Join-Path $env:TEMP 'pointer-recovery'
$app = Join-Path $in 'ugurugu.exe'
$uguDoc = Join-Path $in 'ugu-doc.exe'
$results = New-Object System.Collections.ArrayList

. (Join-Path $PSScriptRoot 'common.ps1')

Add-Type -ReferencedAssemblies System.Windows.Forms, System.Drawing -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class Synthetic
{
    [StructLayout(LayoutKind.Sequential)]
    public struct POINT { public int X, Y; }

    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }

    [StructLayout(LayoutKind.Sequential)]
    public struct POINTER_INFO
    {
        public uint pointerType, pointerId, frameId, pointerFlags;
        public IntPtr sourceDevice, hwndTarget;
        public POINT ptPixelLocation, ptHimetricLocation, ptPixelLocationRaw, ptHimetricLocationRaw;
        public uint dwTime, historyCount;
        public int InputData;
        public uint dwKeyStates;
        public ulong PerformanceCount;
        public int ButtonChangeType;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct POINTER_PEN_INFO
    {
        public POINTER_INFO pointerInfo;
        public uint penFlags, penMask, pressure, rotation;
        public int tiltX, tiltY;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct POINTER_TOUCH_INFO
    {
        public POINTER_INFO pointerInfo;
        public uint touchFlags, touchMask;
        public RECT rcContact, rcContactRaw;
        public uint orientation, pressure;
    }

    [StructLayout(LayoutKind.Explicit)]
    public struct POINTER_TYPE_INFO
    {
        [FieldOffset(0)] public uint type;
        [FieldOffset(8)] public POINTER_TOUCH_INFO touchInfo;
        [FieldOffset(8)] public POINTER_PEN_INFO penInfo;
    }

    [DllImport("user32.dll", SetLastError = true)] public static extern IntPtr CreateSyntheticPointerDevice(uint type, uint count, uint mode);
    [DllImport("user32.dll", SetLastError = true)] public static extern bool InjectSyntheticPointerInput(IntPtr device, [In] POINTER_TYPE_INFO[] info, uint count);
    [DllImport("user32.dll")] public static extern void DestroySyntheticPointerDevice(IntPtr device);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT point);
    [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr hwnd, uint flags);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hwnd, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hwnd, int command);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr hwnd, System.Text.StringBuilder text, int count);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr hwnd, System.Text.StringBuilder text, int count);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint process);

    public static string Describe(IntPtr hwnd)
    {
        var text = new System.Text.StringBuilder(256);
        var kind = new System.Text.StringBuilder(256);
        uint process;
        GetWindowText(hwnd, text, 256);
        GetClassName(hwnd, kind, 256);
        GetWindowThreadProcessId(hwnd, out process);
        string name = "?";
        try { name = System.Diagnostics.Process.GetProcessById((int)process).ProcessName; } catch (Exception) { }
        return string.Format("'{0}' class {1} process {2} ({3})", text, kind, name, process);
    }
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr context);

    [DllImport("kernel32.dll")] static extern bool QueryPerformanceCounter(out long count);

    static uint frame;

    // Stamps each injection as one frame with its time, as a device does.
    public static bool Inject(IntPtr device, POINTER_TYPE_INFO[] frames)
    {
        long now;
        QueryPerformanceCounter(out now);
        frame++;
        for (int i = 0; i < frames.Length; i++)
        {
            if (frames[i].type == 3)
            {
                frames[i].penInfo.pointerInfo.frameId = frame;
                frames[i].penInfo.pointerInfo.PerformanceCount = (ulong)now;
            }
            else
            {
                frames[i].touchInfo.pointerInfo.frameId = frame;
                frames[i].touchInfo.pointerInfo.PerformanceCount = (ulong)now;
            }
        }
        return InjectSyntheticPointerInput(device, frames, (uint)frames.Length);
    }

    [DllImport("user32.dll")] static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra);

    public static uint ProcessOf(IntPtr hwnd)
    {
        uint process;
        GetWindowThreadProcessId(hwnd, out process);
        return process;
    }

    // Windows are told apart by process: the app's main window is not the
    // only top-level window it has.
    public static bool Under(uint process, int x, int y)
    {
        POINT point; point.X = x; point.Y = y;
        return ProcessOf(GetAncestor(WindowFromPoint(point), 2)) == process;
    }

    public static bool InFront(uint process)
    {
        return ProcessOf(GetForegroundWindow()) == process;
    }

    // Windows lets a process bring a window to the front after a key press.
    public static void Front(IntPtr hwnd)
    {
        keybd_event(0x12, 0, 0, UIntPtr.Zero);
        keybd_event(0x12, 0, 2, UIntPtr.Zero);
        SetForegroundWindow(hwnd);
    }

    public static POINTER_TYPE_INFO Pen(int x, int y, uint flags, uint penFlags, uint pressure)
    {
        var frame = new POINTER_TYPE_INFO();
        frame.type = 3;
        frame.penInfo.pointerInfo.pointerType = 3;
        frame.penInfo.pointerInfo.pointerFlags = flags;
        frame.penInfo.pointerInfo.ptPixelLocation.X = x;
        frame.penInfo.pointerInfo.ptPixelLocation.Y = y;
        frame.penInfo.penFlags = penFlags;
        frame.penInfo.penMask = 1;
        frame.penInfo.pressure = pressure;
        return frame;
    }

    public static POINTER_TYPE_INFO Touch(uint id, int x, int y, uint flags)
    {
        var frame = new POINTER_TYPE_INFO();
        frame.type = 2;
        frame.touchInfo.pointerInfo.pointerType = 2;
        frame.touchInfo.pointerInfo.pointerId = id;
        frame.touchInfo.pointerInfo.pointerFlags = flags;
        frame.touchInfo.pointerInfo.ptPixelLocation.X = x;
        frame.touchInfo.pointerInfo.ptPixelLocation.Y = y;
        frame.touchInfo.touchMask = 1 | 4;
        frame.touchInfo.rcContact.Left = x - 2;
        frame.touchInfo.rcContact.Top = y - 2;
        frame.touchInfo.rcContact.Right = x + 2;
        frame.touchInfo.rcContact.Bottom = y + 2;
        frame.touchInfo.pressure = 32000;
        return frame;
    }
}

// A plain window that reads the pen flags of the frames it gets, to tell
// what Windows delivers from what the app makes of it.
public class PenWindow : System.Windows.Forms.Form
{
    [DllImport("user32.dll")] static extern bool GetPointerPenInfo(uint id, out Synthetic.POINTER_PEN_INFO info);

    public int Frames;
    public uint FirstFlags = 0xFFFFFFFF, LastFlags, LastPressure;

    protected override void WndProc(ref System.Windows.Forms.Message m)
    {
        if (m.Msg == 0x0246 || m.Msg == 0x0245)
        {
            Synthetic.POINTER_PEN_INFO info;
            if (GetPointerPenInfo((uint)((long)m.WParam & 0xFFFF), out info))
            {
                if (Frames == 0) FirstFlags = info.penFlags;
                LastFlags = info.penFlags;
                LastPressure = info.pressure;
                Frames++;
            }
        }
        base.WndProc(ref m);
    }
}
'@

$INRANGE = 0x2; $INCONTACT = 0x4; $FIRSTBUTTON = 0x10; $SECONDBUTTON = 0x20
$PRIMARY = 0x2000; $DOWN = 0x10000; $UPDATE = 0x20000; $UP = 0x40000
$PEN_BARREL = 0x1; $PEN_INVERTED = 0x2; $PEN_ERASER = 0x4

[void][Synthetic]::SetProcessDpiAwarenessContext([IntPtr](-4))

# A failed step leaves the app running for the next; destroying the
# device lifts whatever it still holds down.
function Stop-Apps {
}

$script:window = [IntPtr]::Zero
$script:held = @()

function Start-Probe {
    New-Item -ItemType Directory -Force -Path $recovery | Out-Null
    $settings = Join-Path $env:TEMP 'pointer-settings.json'
    Set-Content -Path $settings -Encoding ascii -Value '{"format": "ugurugu.settings", "version": 1, "checkForUpdates": false}'
    $env:UGURUGU_LANGUAGE = 'en'
    $env:UGURUGU_SETTINGS_PATH = $settings
    $env:UGURUGU_RECOVERY_PATH = $recovery
    $env:UGURUGU_LOG_PATH = $logs
    $env:UGURUGU_LOG = 'info,ugurugu=debug,ugu_win::pointer=trace'
    # A Windows window (rgnupdt.exe) opens a few seconds after the Sandbox
    # logs on and takes the front; the first strokes went to it.
    $deadline = (Get-Date).AddSeconds(30)
    while ((Get-Date) -lt $deadline -and -not (Get-Process rgnupdt -ErrorAction SilentlyContinue)) {
        Start-Sleep -Milliseconds 500
    }
    Get-Process rgnupdt -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Process -FilePath $app | Out-Null
    $process = Wait-Window
    $script:window = $process.MainWindowHandle
    $script:owner = [uint32]$process.Id
    # Without a place of its own the window opens maximized on a screen too
    # small for 1600 x 1000; restored first, it takes the size given.
    [void][Synthetic]::ShowWindow($script:window, 9)
    [void][Synthetic]::SetWindowPos($script:window, [IntPtr]::Zero, 50, 50, 1600, 1000, 4)
    [Synthetic]::Front($script:window)
    [void](Get-Value 'Canvas zoom percentage')
    Start-Sleep -Seconds 1
    $rect = New-Object Synthetic+RECT
    [void][Synthetic]::GetWindowRect($script:window, [ref]$rect)
    $script:rect = $rect
    $process
}

function Lift([int] $Kind) {
    foreach ($frame in $script:held) {
        if ($Kind -eq 3) {
            $at = $frame.penInfo.pointerInfo.ptPixelLocation
            $up = [Synthetic]::Pen($at.X, $at.Y, $UP, 0, 0)
        } else {
            $at = $frame.touchInfo.pointerInfo.ptPixelLocation
            $up = [Synthetic]::Touch($frame.touchInfo.pointerInfo.pointerId, $at.X, $at.Y, $UP)
        }
        [void][Synthetic]::Inject($script:device, [Synthetic+POINTER_TYPE_INFO[]]@($up))
    }
    $script:held = @()
}

# Frames in contact, and the one that lifts, may leave the window: the window
# holds the pointer while it is down.
function Send([int] $Kind, $Frames, [switch] $OutsideOk) {
    if (-not [Synthetic]::InFront($script:owner)) {
        Lift $Kind
        Save-Screen 'lost'
        throw "The app is not in front: $([Synthetic]::Describe([Synthetic]::GetForegroundWindow()))"
    }
    foreach ($frame in $Frames) {
        $info = if ($Kind -eq 3) { $frame.penInfo.pointerInfo } else { $frame.touchInfo.pointerInfo }
        $pressed = ($info.pointerFlags -band ($INCONTACT -bor $UP)) -ne 0
        if (-not [Synthetic]::Under($script:owner, $info.ptPixelLocation.X, $info.ptPixelLocation.Y) -and -not ($OutsideOk -and $pressed)) {
            Lift $Kind
            throw "The app is not under $($info.ptPixelLocation.X),$($info.ptPixelLocation.Y)"
        }
    }
    if (-not [Synthetic]::Inject($script:device, [Synthetic+POINTER_TYPE_INFO[]]$Frames)) {
        throw "InjectSyntheticPointerInput failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }
    $script:held = @($Frames | Where-Object {
            $flags = if ($Kind -eq 3) { $_.penInfo.pointerInfo.pointerFlags } else { $_.touchInfo.pointerInfo.pointerFlags }
            ($flags -band $INCONTACT) -ne 0
        })
}

$script:written = 0

# The strokes in the newest recovery file. A step that changed the drawing
# waits for the app to write it (three seconds after the last change); one
# that should not have waits as long and expects no new write.
function Get-Strokes([switch] $Changed) {
    if ($Changed) {
        $deadline = (Get-Date).AddSeconds(20)
        while ((Get-Date) -lt $deadline) {
            $sequences = @(Get-ChildItem $logs -Filter '*.log' | Select-String 'work written for recovery.*sequence=(\d+)' |
                ForEach-Object { [int]$_.Matches[0].Groups[1].Value })
            if ($sequences -and ($sequences | Measure-Object -Maximum).Maximum -gt $script:written) {
                $script:written = ($sequences | Measure-Object -Maximum).Maximum
                break
            }
            Start-Sleep -Milliseconds 250
        }
    } else {
        Start-Sleep -Seconds 5
    }
    $file = Get-ChildItem -Recurse -File $recovery -Filter '*.ugurugu' -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime | Select-Object -Last 1
    if (-not $file) {
        return @()
    }
    @(& $uguDoc strokes $file.FullName | Where-Object { $_.Trim() })
}

function Get-Value([string] $Name) {
    $match = New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty, $Name)
    $value = Wait-Until {
        $root = [Windows.Automation.AutomationElement]::FromHandle($script:window)
        foreach ($element in $root.FindAll([Windows.Automation.TreeScope]::Descendants, $match)) {
            $pattern = $null
            if ($element.TryGetCurrentPattern([Windows.Automation.RangeValuePattern]::Pattern, [ref]$pattern)) {
                return [string]$pattern.Current.Value
            }
            if ($element.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
                return $pattern.Current.Value -replace '[^0-9.\-]', ''
            }
        }
    } 10 "the value of '$Name'"
    [double]$value
}

# Sets the check box named $Name and returns whether it is now on.
function Set-Toggle([string] $Name, [bool] $On) {
    $match = New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty, $Name)
    $pattern = Wait-Until {
        $root = [Windows.Automation.AutomationElement]::FromHandle($script:window)
        foreach ($element in $root.FindAll([Windows.Automation.TreeScope]::Descendants, $match)) {
            $found = $null
            if ($element.TryGetCurrentPattern([Windows.Automation.TogglePattern]::Pattern, [ref]$found)) {
                return , $found
            }
        }
    } 10 "a check box named '$Name'"
    if (($pattern.Current.ToggleState -eq [Windows.Automation.ToggleState]::On) -ne $On) {
        $pattern.Toggle()
        Start-Sleep -Milliseconds 500
    }
    $pattern.Current.ToggleState -eq [Windows.Automation.ToggleState]::On
}

function Get-Field([string] $Line, [string] $Name) {
    $words = $Line -split '\s+'
    $words[[array]::IndexOf($words, $Name) + 1]
}

function Pen-Stroke($Points, [uint32] $PenFlags = 0, [int] $Contact = ($INCONTACT -bor $FIRSTBUTTON), [switch] $OutsideOk) {
    $flags = $INRANGE -bor $PRIMARY
    $first = $Points[0]
    Send 3 @([Synthetic]::Pen($first[0], $first[1], $flags -bor $UPDATE, $PenFlags, 0))
    Start-Sleep -Milliseconds 50
    for ($i = 0; $i -lt $Points.Count; $i++) {
        $point = $Points[$i]
        $state = if ($i -eq 0) { $DOWN } else { $UPDATE }
        Send 3 @([Synthetic]::Pen($point[0], $point[1], $flags -bor $Contact -bor $state, $PenFlags, $point[2])) -OutsideOk:$OutsideOk
        Start-Sleep -Milliseconds 8
    }
    $last = $Points[-1]
    Send 3 @([Synthetic]::Pen($last[0], $last[1], $flags -bor $UP, $PenFlags, 0)) -OutsideOk
    $script:held = @()
    Start-Sleep -Milliseconds 50
    if ([Synthetic]::Under($script:owner, $last[0], $last[1])) {
        Send 3 @([Synthetic]::Pen($last[0], $last[1], $UPDATE, $PenFlags, 0))
    }
}

$process = Start-Probe
$cx = [int](($script:rect.Left + $script:rect.Right) / 2)
$cy = [int](($script:rect.Top + $script:rect.Bottom) / 2)
$script:device = [Synthetic]::CreateSyntheticPointerDevice(3, 1, 1)
$script:before = 0

Step 'Pen device' {
    if ($script:device -eq [IntPtr]::Zero) {
        throw "CreateSyntheticPointerDevice failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }
    "window $($script:rect.Left),$($script:rect.Top)-$($script:rect.Right),$($script:rect.Bottom), centre $cx,$cy"
}

Add-Type -AssemblyName System.Drawing, System.Windows.Forms
$script:shots = 0

# The Sandbox's own screen, never the host's.
function Save-Screen([string] $Name) {
    $script:shots++
    $bounds = [Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bitmap = New-Object Drawing.Bitmap $bounds.Width, $bounds.Height
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($bounds.Location, [Drawing.Point]::Empty, $bounds.Size)
    $bitmap.Save((Join-Path $out ("{0:00}-$Name.png" -f $script:shots)))
    $graphics.Dispose()
    $bitmap.Dispose()
}

Step 'Pressure from 64 to 1024 of 1024' {
    Save-Screen 'before-first-stroke'
    $points = @(0..60 | ForEach-Object { , @(($cx - 300 + $_ * 10), ($cy - 200), (64 + $_ * 16)) })
    Pen-Stroke $points
    $lines = @(Get-Strokes -Changed)
    $new = @($lines | Select-Object -Skip $script:before)
    $script:before = $lines.Count
    if ($new.Count -ne 1 -or $new[0] -notmatch ' paint ') {
        throw "Strokes: $($new -join '; ')"
    }
    $first = [double](Get-Field $new[0] 'first')
    $last = [double](Get-Field $new[0] 'last')
    if ([Math]::Abs($first - 64 / 1024) -gt 0.02 -or [Math]::Abs($last - 1) -gt 0.02) {
        throw "Pressure $first .. ${last}: $($new[0])"
    }
    $new[0]
}

Step 'Pen pressure off draws at full pressure' {
    if (Set-Toggle 'Use tablet pressure' $false) {
        throw 'The check box stayed on'
    }
    $points = @(0..60 | ForEach-Object { , @(($cx - 300 + $_ * 10), ($cy - 150), (64 + $_ * 16)) })
    Pen-Stroke $points
    $on = Set-Toggle 'Use tablet pressure' $true
    $lines = @(Get-Strokes -Changed)
    $new = @($lines | Select-Object -Skip $script:before)
    $script:before = $lines.Count
    if ($new.Count -ne 1 -or $new[0] -notmatch ' paint ') {
        throw "Strokes: $($new -join '; ')"
    }
    $first = [double](Get-Field $new[0] 'first')
    $last = [double](Get-Field $new[0] 'last')
    if ($first -ne 1 -or $last -ne 1 -or -not $on) {
        throw "Pressure $first .. ${last}, back on: ${on}: $($new[0])"
    }
    "$($new[0]); back on: $on"
}

Step 'Barrel held does not draw' {
    $points = @(0..30 | ForEach-Object { , @(($cx - 300 + $_ * 10), $cy, 700) })
    Pen-Stroke $points $PEN_BARREL ($INCONTACT -bor $SECONDBUTTON)
    $lines = @(Get-Strokes)
    $new = @($lines | Select-Object -Skip $script:before)
    $script:before = $lines.Count
    if ($new.Count -ne 0) {
        throw "Strokes: $($new -join '; ')"
    }
    'no stroke'
}

Step 'Hover does not draw' {
    for ($i = 0; $i -lt 60; $i++) {
        Send 3 @([Synthetic]::Pen(($cx - 200 + $i * 5), ($cy + 100), ($INRANGE -bor $PRIMARY -bor $UPDATE), 0, 0))
        Start-Sleep -Milliseconds 16
    }
    Send 3 @([Synthetic]::Pen(($cx + 100), ($cy + 100), $UPDATE, 0, 0))
    $lines = @(Get-Strokes)
    $new = @($lines | Select-Object -Skip $script:before)
    $script:before = $lines.Count
    if ($new.Count -ne 0) {
        throw "Strokes: $($new -join '; ')"
    }
    'no stroke'
}

Step 'Released outside the window' {
    $right = $script:rect.Right
    $step = ($right + 100 - $cx) / 40
    $points = @(0..40 | ForEach-Object { , @([int]($cx + $_ * $step), ($cy + 100), 600) })
    Pen-Stroke $points -OutsideOk
    $lines = @(Get-Strokes -Changed)
    $new = @($lines | Select-Object -Skip $script:before)
    $script:before = $lines.Count
    if ($new.Count -ne 1 -or $new[0] -notmatch ' paint ') {
        throw "Strokes: $($new -join '; ')"
    }
    "up at x $($right + 100), window ends at $right", $new[0]
}

Step 'Many samples at once' {
    $count = 2000
    $flags = $INRANGE -bor $PRIMARY
    Send 3 @([Synthetic]::Pen(($cx - 300), ($cy + 200), ($flags -bor $UPDATE), 0, 0))
    # About as fast as the fastest pens report, one sample a millisecond.
    $watch = [Diagnostics.Stopwatch]::StartNew()
    for ($i = 0; $i -lt $count; $i++) {
        while ($watch.Elapsed.TotalMilliseconds -lt $i) {
        }
        $state = if ($i -eq 0) { $DOWN } else { $UPDATE }
        $x = [int]($cx - 300 + $i * 0.3)
        $y = [int]($cy + 200 + 40 * [Math]::Sin($i / 40))
        try {
            Send 3 @([Synthetic]::Pen($x, $y, ($flags -bor $INCONTACT -bor $FIRSTBUTTON -bor $state), 0, 600))
        } catch {
            throw "sample $i at $x,$y after $([int]$watch.Elapsed.TotalMilliseconds) ms: $($_.Exception.Message)"
        }
    }
    $elapsed = $watch.Elapsed.TotalSeconds
    Send 3 @([Synthetic]::Pen($x, $y, ($flags -bor $UP), 0, 0)) -OutsideOk
    $script:held = @()
    Send 3 @([Synthetic]::Pen($x, $y, $UPDATE, 0, 0))
    $lines = @(Get-Strokes -Changed)
    $new = @($lines | Select-Object -Skip $script:before)
    $script:before = $lines.Count
    if ($new.Count -ne 1) {
        throw "Strokes: $($new -join '; ')"
    }
    "$count samples in $([int]($elapsed * 1000)) ms ($([int]($count / $elapsed))/s)", $new[0]
}

# The app's eraser end is checked by a unit test instead (canvas.rs): the
# pen flags injected here do not reach a window, which this step records.
Step 'Eraser flags in a plain window' {
    $form = New-Object PenWindow
    $form.StartPosition = 'Manual'
    $form.SetBounds(200, 200, 600, 400)
    $form.TopMost = $true
    $form.Show()
    [Windows.Forms.Application]::DoEvents()
    [Synthetic]::Front($form.Handle)
    $app = $script:owner
    $script:owner = [uint32]$PID
    try {
        Start-Sleep -Milliseconds 500
        [Windows.Forms.Application]::DoEvents()
        $flags = $INRANGE -bor $PRIMARY
        $pen = $PEN_INVERTED -bor $PEN_ERASER
        Send 3 @([Synthetic]::Pen(400, 400, ($flags -bor $UPDATE), $pen, 0))
        for ($i = 0; $i -le 20; $i++) {
            $state = if ($i -eq 0) { $DOWN } else { $UPDATE }
            Send 3 @([Synthetic]::Pen((400 + $i * 5), 400, ($flags -bor $INCONTACT -bor $FIRSTBUTTON -bor $state), $pen, 700))
            [Windows.Forms.Application]::DoEvents()
            Start-Sleep -Milliseconds 8
        }
        Send 3 @([Synthetic]::Pen(500, 400, ($flags -bor $UP), $pen, 0))
        $script:held = @()
        [Windows.Forms.Application]::DoEvents()
        "frames $($form.Frames), pen flags on the first frame 0x{0:x}, on the last 0x{1:x} (sent 0x{2:x}), pressure {3}" -f $form.FirstFlags, $form.LastFlags, $pen, $form.LastPressure
    } finally {
        $script:owner = $app
        $form.Close()
        [Synthetic]::Front($script:window)
        Start-Sleep -Milliseconds 500
    }
}

[Synthetic]::DestroySyntheticPointerDevice($script:device)
$script:device = [Synthetic]::CreateSyntheticPointerDevice(2, 2, 1)
$zoomName = 'Canvas zoom percentage'
$turnName = 'Canvas rotation angle'

function Two($A, $B, [int] $State) {
    $flags = $INRANGE -bor $INCONTACT -bor $State
    Send 2 @([Synthetic]::Touch(0, $A[0], $A[1], ($flags -bor $PRIMARY)), [Synthetic]::Touch(1, $B[0], $B[1], $flags))
}

function Gesture([scriptblock] $At) {
    $start = & $At 0.0
    Two $start[0] $start[1] $DOWN
    for ($i = 1; $i -le 30; $i++) {
        $now = & $At ($i / 30)
        Two $now[0] $now[1] $UPDATE
        Start-Sleep -Milliseconds 16
    }
    $end = & $At 1.0
    Send 2 @([Synthetic]::Touch(0, $end[0][0], $end[0][1], ($UP -bor $PRIMARY)), [Synthetic]::Touch(1, $end[1][0], $end[1][1], $UP))
    $script:held = @()
    Start-Sleep -Milliseconds 500
}

Step 'Touch device' {
    if ($script:device -eq [IntPtr]::Zero) {
        throw "CreateSyntheticPointerDevice failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }
    "zoom $(Get-Value $zoomName), turn $(Get-Value $turnName)"
}

Step 'Two-finger pan' {
    $zoom = Get-Value $zoomName; $turn = Get-Value $turnName
    Gesture { param($t) , @(@([int]($cx - 100 + 150 * $t), [int]($cy + 80 * $t)), @([int]($cx + 100 + 150 * $t), [int]($cy + 80 * $t))) }
    $zoom2 = Get-Value $zoomName; $turn2 = Get-Value $turnName
    if ([Math]::Abs($zoom2 - $zoom) -gt 0.5 -or [Math]::Abs($turn2 - $turn) -gt 0.5) {
        throw "zoom $zoom -> $zoom2, turn $turn -> $turn2"
    }
    "zoom $zoom -> $zoom2, turn $turn -> $turn2"
}

Step 'Pinch to twice the distance' {
    $zoom = Get-Value $zoomName
    Gesture { param($t) $half = 100 + 100 * $t; , @(@([int]($cx - $half), $cy), @([int]($cx + $half), $cy)) }
    $zoom2 = Get-Value $zoomName
    if ([Math]::Abs($zoom2 / $zoom - 2) -gt 0.05) {
        throw "zoom $zoom -> $zoom2"
    }
    "zoom $zoom -> $zoom2"
}

Step 'Turn by a quarter' {
    $turn = Get-Value $turnName
    Gesture { param($t) $a = [Math]::PI / 2 * $t; $dx = 150 * [Math]::Cos($a); $dy = 150 * [Math]::Sin($a); , @(@([int]($cx - $dx), [int]($cy - $dy)), @([int]($cx + $dx), [int]($cy + $dy))) }
    $turn2 = Get-Value $turnName
    if ([Math]::Abs([Math]::Abs($turn2 - $turn) - 90) -gt 2) {
        throw "turn $turn -> $turn2"
    }
    "turn $turn -> $turn2"
}

Step 'One finger does not draw' {
    $flags = $INRANGE -bor $INCONTACT
    Send 2 @([Synthetic]::Touch(0, ($cx - 200), ($cy + 150), ($flags -bor $PRIMARY -bor $DOWN)))
    for ($i = 1; $i -le 40; $i++) {
        Send 2 @([Synthetic]::Touch(0, ($cx - 200 + $i * 10), ($cy + 150), ($flags -bor $PRIMARY -bor $UPDATE)))
        Start-Sleep -Milliseconds 16
    }
    Send 2 @([Synthetic]::Touch(0, ($cx + 200), ($cy + 150), ($UP -bor $PRIMARY)))
    $script:held = @()
    $lines = @(Get-Strokes)
    $new = @($lines | Select-Object -Skip $script:before)
    if ($new.Count -ne 0) {
        throw "Strokes: $($new -join '; ')"
    }
    'no stroke'
}

[Synthetic]::DestroySyntheticPointerDevice($script:device)
Get-AppProcess | ForEach-Object { Stop-Process -Id $_.Id -Force }
if (Test-Path $recovery) {
    Copy-Item -Recurse $recovery (Join-Path $out 'recovery')
}
Write-Host "Done: $(Join-Path $out 'results.md')"
