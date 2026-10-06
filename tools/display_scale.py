# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

"""Reads or sets the primary monitor's display scale, as the Settings app does,
for the M0 DPI checks (docs/rust/m0-evidence.md section 7).

Usage: python display_scale.py [percent]

Without an argument it prints the current scale. The change applies at once
and persists, so set the scale back when done.

Windows exposes this only through undocumented DisplayConfig device info
types (-3 get, -4 set) that count steps from the monitor's recommended scale."""
import ctypes
import ctypes.wintypes as wt
import sys

user32 = ctypes.windll.user32
user32.MonitorFromPoint.restype = ctypes.c_void_p
STEPS = [100, 125, 150, 175, 200, 225, 250, 300, 350, 400, 450, 500]
QDC_ONLY_ACTIVE_PATHS = 2


class LUID(ctypes.Structure):
    _fields_ = [("LowPart", wt.DWORD), ("HighPart", wt.LONG)]


class HEADER(ctypes.Structure):
    _fields_ = [("type", ctypes.c_int), ("size", wt.UINT), ("adapterId", LUID), ("id", wt.UINT)]


class GET_SCALE(ctypes.Structure):
    _fields_ = [("header", HEADER), ("minimum", ctypes.c_int), ("current", ctypes.c_int),
                ("maximum", ctypes.c_int)]


class SET_SCALE(ctypes.Structure):
    _fields_ = [("header", HEADER), ("scale", ctypes.c_int)]


class SOURCE_NAME(ctypes.Structure):
    _fields_ = [("header", HEADER), ("name", wt.WCHAR * 32)]


class PATH(ctypes.Structure):
    # DISPLAYCONFIG_PATH_INFO: source (adapter, id, mode index, flags), target
    # (adapter, id, mode index, technology, rotation, scaling, refresh rate,
    # scanline ordering, available, flags), then the path flags.
    _fields_ = [("sourceAdapter", LUID), ("sourceId", wt.UINT), ("sourceRest", wt.UINT * 2),
                ("targetRest", ctypes.c_byte * 48), ("flags", wt.UINT)]


assert ctypes.sizeof(PATH) == 72


def primary_source():
    paths, modes = wt.UINT(), wt.UINT()
    if user32.GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, ctypes.byref(paths), ctypes.byref(modes)):
        raise SystemExit("GetDisplayConfigBufferSizes failed")
    path_array = (PATH * paths.value)()
    mode_array = (ctypes.c_byte * (64 * modes.value))()
    if user32.QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, ctypes.byref(paths), path_array, ctypes.byref(modes),
                                 mode_array, None):
        raise SystemExit("QueryDisplayConfig failed")
    primary = primary_device_name()
    for path in path_array[:paths.value]:
        name = SOURCE_NAME()
        name.header = HEADER(1, ctypes.sizeof(SOURCE_NAME), path.sourceAdapter, path.sourceId)
        if user32.DisplayConfigGetDeviceInfo(ctypes.byref(name)) == 0 and name.name == primary:
            return path.sourceAdapter, path.sourceId
    raise SystemExit(f"no active path for {primary}")


def primary_device_name():
    class MONITORINFOEX(ctypes.Structure):
        _fields_ = [("cbSize", wt.DWORD), ("rcMonitor", wt.RECT), ("rcWork", wt.RECT), ("dwFlags", wt.DWORD),
                    ("szDevice", wt.WCHAR * 32)]

    info = MONITORINFOEX(cbSize=ctypes.sizeof(MONITORINFOEX))
    monitor = user32.MonitorFromPoint(wt.POINT(0, 0), 1)
    user32.GetMonitorInfoW(ctypes.c_void_p(monitor), ctypes.byref(info))
    return info.szDevice


def scale_info(adapter, source):
    info = GET_SCALE()
    info.header = HEADER(-3, ctypes.sizeof(GET_SCALE), adapter, source)
    if user32.DisplayConfigGetDeviceInfo(ctypes.byref(info)):
        raise SystemExit("reading the scale failed")
    return info


adapter, source = primary_source()
info = scale_info(adapter, source)
recommended = STEPS.index(100) - info.minimum
current = STEPS[recommended + info.current]
if len(sys.argv) < 2:
    print(current)
    sys.exit()
target = int(sys.argv[1])
if target not in STEPS or not info.minimum <= STEPS.index(target) - recommended <= info.maximum:
    raise SystemExit(f"{target}% is not offered for this monitor")
request = SET_SCALE()
request.header = HEADER(-4, ctypes.sizeof(SET_SCALE), adapter, source)
request.scale = STEPS.index(target) - recommended
if user32.DisplayConfigSetDeviceInfo(ctypes.byref(request)):
    raise SystemExit("setting the scale failed")
print(f"{current} -> {STEPS[recommended + scale_info(adapter, source).current]}")
