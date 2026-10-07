# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

"""Measures the M0 baselines that need no in-app instrumentation
(docs/rust/m0-evidence.md section 5): UI thread work per input batch and at
pen-up, save, open, export cancel, and memory.

Usage: python baseline_probe.py <stroke|save|open|cancel|memory|playback|export>
       <app.exe> <fixture.ugu> <output-dir> [--repeat N] [--presentmon PATH]
       [--thread NAME] [--no-play-key]

--thread measures the thread with that name instead of the window's: the
3.0 app handles input and draws on its "render" thread. --no-play-key leaves
P alone, which starts playback in 3.0 rather than stopping it.

UI thread work is the thread's CPU cycles (QueryThreadCycleTime) from one
input to the next, so it counts everything the thread did for that input,
including painting, and nothing it waited for. The span is the time from the
input to the end of the thread's first run of work after it.

The app gets a copy of the fixture, its own instance lock and recovery path.
The C++ app starts playing, so P is pressed first. Input goes to the
foreground window; the probe stops if the app is not in front."""
import argparse
import ctypes
import ctypes.wintypes as wt
import datetime
import json
import math
import os
import random
import shutil
import statistics
import subprocess
import sys
import time

user32 = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32
psapi = ctypes.windll.psapi
ctypes.windll.shcore.SetProcessDpiAwareness(2)
for name in ("GetForegroundWindow", "WindowFromPoint", "GetAncestor", "GetWindow", "FindWindowExW"):
    getattr(user32, name).restype = ctypes.c_void_p
user32.WindowFromPoint.argtypes = [wt.POINT]
user32.GetAncestor.argtypes = [ctypes.c_void_p, ctypes.c_uint]
user32.FindWindowExW.argtypes = [ctypes.c_void_p, ctypes.c_void_p, wt.LPCWSTR, wt.LPCWSTR]
user32.PostMessageW.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_void_p]
user32.SendMessageW.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_wchar_p]
user32.SendMessageTimeoutW.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_void_p,
                                       ctypes.c_uint, ctypes.c_uint, ctypes.c_void_p]
user32.InternalGetWindowText.argtypes = [ctypes.c_void_p, ctypes.c_wchar_p, ctypes.c_int]
user32.GetClassNameW.argtypes = [ctypes.c_void_p, ctypes.c_wchar_p, ctypes.c_int]
user32.GetWindowThreadProcessId.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
user32.IsWindow.argtypes = [ctypes.c_void_p]
user32.IsWindowVisible.argtypes = [ctypes.c_void_p]
user32.SetForegroundWindow.argtypes = [ctypes.c_void_p]
user32.SetWindowPos.argtypes = [ctypes.c_void_p, ctypes.c_void_p] + [ctypes.c_int] * 4 + [ctypes.c_uint]
user32.GetClientRect.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
user32.ClientToScreen.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
kernel32.OpenThread.restype = ctypes.c_void_p
kernel32.OpenProcess.restype = ctypes.c_void_p
kernel32.QueryThreadCycleTime.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint64)]
psapi.GetProcessMemoryInfo.argtypes = [ctypes.c_void_p, ctypes.c_void_p, wt.DWORD]


class MOUSEINPUT(ctypes.Structure):
    _fields_ = [("dx", wt.LONG), ("dy", wt.LONG), ("mouseData", wt.DWORD), ("dwFlags", wt.DWORD),
                ("time", wt.DWORD), ("dwExtraInfo", ctypes.c_size_t)]


class KEYBDINPUT(ctypes.Structure):
    _fields_ = [("wVk", wt.WORD), ("wScan", wt.WORD), ("dwFlags", wt.DWORD), ("time", wt.DWORD),
                ("dwExtraInfo", ctypes.c_size_t)]


class _INPUT(ctypes.Union):
    _fields_ = [("mi", MOUSEINPUT), ("ki", KEYBDINPUT)]


class INPUT(ctypes.Structure):
    _fields_ = [("type", wt.DWORD), ("u", _INPUT)]


class MEMORY(ctypes.Structure):
    _fields_ = [("cb", wt.DWORD), ("PageFaultCount", wt.DWORD), ("PeakWorkingSetSize", ctypes.c_size_t),
                ("WorkingSetSize", ctypes.c_size_t), ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
                ("QuotaPagedPoolUsage", ctypes.c_size_t), ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
                ("QuotaNonPagedPoolUsage", ctypes.c_size_t), ("PagefileUsage", ctypes.c_size_t),
                ("PeakPagefileUsage", ctypes.c_size_t), ("PrivateUsage", ctypes.c_size_t)]


VK_RETURN, VK_ESCAPE, VK_CONTROL = 0x0D, 0x1B, 0x11
MOUSE_MOVE_ABSOLUTE = 0x1 | 0x8000 | 0x4000
MOUSE_DOWN, MOUSE_UP = 0x2, 0x4


def send(inputs):
    array = (INPUT * len(inputs))(*inputs)
    if user32.SendInput(len(inputs), array, ctypes.sizeof(INPUT)) != len(inputs):
        raise SystemExit("SendInput refused")


def key(vk, up=False):
    return INPUT(type=1, u=_INPUT(ki=KEYBDINPUT(vk, 0, 2 if up else 0, 0, 0)))


def tap(vk):
    send([key(vk), key(vk, True)])


def chord(vk):
    send([key(VK_CONTROL), key(vk), key(vk, True), key(VK_CONTROL, True)])


def mouse_move(x, y):
    left, top = user32.GetSystemMetrics(76), user32.GetSystemMetrics(77)
    width, height = user32.GetSystemMetrics(78), user32.GetSystemMetrics(79)
    dx = int((x - left + 0.5) * 65536 / width)
    dy = int((y - top + 0.5) * 65536 / height)
    return INPUT(type=0, u=_INPUT(mi=MOUSEINPUT(dx, dy, 0, MOUSE_MOVE_ABSOLUTE, 0, 0)))


def mouse_button(flag):
    return INPUT(type=0, u=_INPUT(mi=MOUSEINPUT(0, 0, 0, flag, 0, 0)))


def window_text(hwnd):
    buffer = ctypes.create_unicode_buffer(512)
    user32.InternalGetWindowText(hwnd, buffer, 512)
    return buffer.value


def class_name(hwnd):
    buffer = ctypes.create_unicode_buffer(256)
    user32.GetClassNameW(hwnd, buffer, 256)
    return buffer.value


def window_pid(hwnd):
    pid = wt.DWORD()
    user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
    return pid.value


def visible_windows(pid):
    found = []

    @ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def visit(hwnd, _):
        if window_pid(hwnd) == pid and user32.IsWindowVisible(hwnd):
            found.append(hwnd)
        return True

    user32.EnumWindows(visit, 0)
    return found


def wait_for(condition, timeout, what):
    deadline = time.perf_counter() + timeout
    while time.perf_counter() < deadline:
        value = condition()
        if value:
            return value
        time.sleep(0.002)
    raise SystemExit(f"timed out waiting for {what}")


class THREADENTRY32(ctypes.Structure):
    _fields_ = [("dwSize", wt.DWORD), ("cntUsage", wt.DWORD), ("th32ThreadID", wt.DWORD),
                ("th32OwnerProcessID", wt.DWORD), ("tpBasePri", wt.LONG), ("tpDeltaPri", wt.LONG),
                ("dwFlags", wt.DWORD)]


def thread_named(pid, name):
    """The id of `pid`'s thread whose description is `name`."""
    kernel32.CreateToolhelp32Snapshot.restype = ctypes.c_void_p
    snapshot = kernel32.CreateToolhelp32Snapshot(0x4, 0)
    entry = THREADENTRY32(dwSize=ctypes.sizeof(THREADENTRY32))
    found = None
    more = kernel32.Thread32First(ctypes.c_void_p(snapshot), ctypes.byref(entry))
    while more and found is None:
        if entry.th32OwnerProcessID == pid:
            handle = kernel32.OpenThread(0x0800, False, entry.th32ThreadID)
            text = ctypes.c_wchar_p()
            if handle and kernel32.GetThreadDescription(ctypes.c_void_p(handle), ctypes.byref(text)) >= 0:
                if text.value == name:
                    found = entry.th32ThreadID
                kernel32.LocalFree(text)
            if handle:
                kernel32.CloseHandle(ctypes.c_void_p(handle))
        more = kernel32.Thread32Next(ctypes.c_void_p(snapshot), ctypes.byref(entry))
    kernel32.CloseHandle(ctypes.c_void_p(snapshot))
    if found is None:
        raise SystemExit(f"no thread named {name!r}")
    return found


class UiThread:
    """CPU cycles of the window's thread, or of the thread named `name`,
    converted with a calibrated rate."""

    def __init__(self, hwnd, name=None):
        thread_id = (thread_named(window_pid(hwnd), name) if name
                     else user32.GetWindowThreadProcessId(hwnd, None))
        self.handle = kernel32.OpenThread(0x0800, False, thread_id)
        if not self.handle:
            raise SystemExit("cannot open the UI thread")
        self.cycles_per_ms = calibrate()

    def cycles(self):
        value = ctypes.c_uint64()
        kernel32.QueryThreadCycleTime(self.handle, ctypes.byref(value))
        return value.value

    def watch(self, until):
        """Polls the thread until `until` (perf_counter seconds) and returns
        (cpu ms, seconds from the start to the end of the first run). A run
        ends at a pause over 2ms, which keeps the idle timer's later ticks out
        of the span."""
        start_time = time.perf_counter()
        first = last = self.cycles()
        last_change = run_end = None
        while time.perf_counter() < until:
            now = self.cycles()
            if now != last:
                moment = time.perf_counter()
                if last_change is None or (run_end is None and moment - last_change <= 0.002):
                    last_change = moment
                elif run_end is None:
                    run_end = last_change
                last = now
        end = run_end or last_change or start_time
        return (last - first) / self.cycles_per_ms, end - start_time

    def quiet(self, quiet_for=0.5, timeout=120.0):
        """Waits until the thread spends under 5% of each 20ms window for
        `quiet_for` seconds. An idle C++ app still runs a timer (about 5ms of
        CPU per second), so the threshold cannot be zero."""
        deadline = time.perf_counter() + timeout
        last, since = self.cycles(), time.perf_counter()
        while time.perf_counter() < deadline:
            time.sleep(0.02)
            now = self.cycles()
            if now - last > self.cycles_per_ms * 1.0:
                since = time.perf_counter()
            last = now
            if time.perf_counter() - since >= quiet_for:
                return
        raise SystemExit("the UI thread never went quiet")

    def idle_rate(self, seconds=2.0):
        """CPU ms per second while nothing is sent: the floor under every
        measured interval."""
        before = self.cycles()
        time.sleep(seconds)
        return (self.cycles() - before) / self.cycles_per_ms / seconds


def calibrate():
    """Cycles per millisecond of this machine's cycle counter, from a busy
    loop on this thread."""
    own = kernel32.GetCurrentThread()
    rates = []
    for _ in range(5):
        before, value = time.perf_counter(), ctypes.c_uint64()
        kernel32.QueryThreadCycleTime(ctypes.c_void_p(own), ctypes.byref(value))
        start = value.value
        while time.perf_counter() - before < 0.1:
            pass
        kernel32.QueryThreadCycleTime(ctypes.c_void_p(own), ctypes.byref(value))
        rates.append((value.value - start) / ((time.perf_counter() - before) * 1000))
    # Preemption only lowers a sample, so the highest is the closest.
    return max(rates)


class App:
    def __init__(self, exe, fixture, out, play_key=True, settle=True, thread=None):
        self.out = out
        self.document = os.path.join(out, "document" + os.path.splitext(fixture)[1])
        shutil.copyfile(fixture, self.document)
        env = dict(os.environ)
        env["UGURUGU_INSTANCE_LOCK_PATH"] = os.path.join(out, "instance.lock")
        env["UGURUGU_RECOVERY_PATH"] = os.path.join(out, "recovery.ugu")
        extra = [os.environ.get("UGURUGU_QT_BIN", r"C:\Qt\6.11.2\msvc2022_64\bin"),
                 os.path.join(os.environ["LOCALAPPDATA"], "Ugurugu", "current")]
        env["PATH"] = os.pathsep.join(extra + [env["PATH"]])
        self.log = open(os.path.join(out, "app.log"), "w", encoding="utf-8")
        self.started = time.perf_counter()
        self.process = subprocess.Popen([exe, self.document], env=env, stdout=self.log,
                                        stderr=subprocess.STDOUT)
        self.hwnd = wait_for(self.main_window, 120, "the main window")
        self.shown = time.perf_counter() - self.started
        time.sleep(1.0)
        user32.SetWindowPos(self.hwnd, None, 0, 0, 2560, 1600, 0x0004)
        send([key(0x12), key(0x12, True)])
        user32.SetForegroundWindow(self.hwnd)
        time.sleep(0.5)
        self.check_front()
        self.thread = UiThread(self.hwnd, thread)
        self.idle_cpu_ms_per_s = math.nan
        if not settle:
            return
        if play_key:
            tap(ord("P"))
        self.thread.quiet(1.0)
        self.idle_cpu_ms_per_s = self.thread.idle_rate()

    def log_times(self, event):
        """Wall-clock times (epoch seconds) of the app's log lines containing
        `event`. spdlog stamps local time to the millisecond when the line is
        logged, whenever it is flushed."""
        self.log.flush()
        times = []
        with open(self.log.name, encoding="utf-8", errors="replace") as file:
            for line in file:
                if event in line and line.startswith("["):
                    stamp = datetime.datetime.strptime(line[1:24], "%Y-%m-%d %H:%M:%S.%f")
                    times.append(stamp.timestamp())
        return times

    def main_window(self):
        # 2.2.13 titles "name — Ugurugu", 3.0 "name - Ugurugu".
        windows = [hwnd for hwnd in visible_windows(self.process.pid) if " — " in window_text(hwnd)
                   or window_text(hwnd).startswith("Ugurugu") or window_text(hwnd).endswith(" - Ugurugu")]
        return windows[0] if windows else None

    def check_front(self):
        front = user32.GetForegroundWindow()
        if not front or window_pid(front) != self.process.pid:
            raise SystemExit("the app lost the foreground; stopping input")

    def canvas_point(self, fx, fy):
        rect = wt.RECT()
        user32.GetClientRect(self.hwnd, ctypes.byref(rect))
        origin = wt.POINT(0, 0)
        user32.ClientToScreen(self.hwnd, ctypes.byref(origin))
        return origin.x + rect.right * fx, origin.y + rect.bottom * fy

    def under_cursor(self, x, y):
        under = user32.GetAncestor(user32.WindowFromPoint(wt.POINT(int(x), int(y))), 2)
        return under == self.hwnd

    def memory(self):
        handle = kernel32.OpenProcess(0x1000, False, self.process.pid)
        counters = MEMORY(cb=ctypes.sizeof(MEMORY))
        psapi.GetProcessMemoryInfo(ctypes.c_void_p(handle), ctypes.byref(counters), ctypes.sizeof(MEMORY))
        kernel32.CloseHandle(ctypes.c_void_p(handle))
        return {"working_set": counters.WorkingSetSize, "peak_working_set": counters.PeakWorkingSetSize,
                "private": counters.PrivateUsage, "peak_private": counters.PeakPagefileUsage}

    def gpu_dedicated(self):
        query = (f"(Get-Counter '\\GPU Process Memory(pid_{self.process.pid}_*)\\Dedicated Usage')"
                 ".CounterSamples | Measure-Object CookedValue -Sum | % Sum")
        result = subprocess.run(["powershell", "-NoProfile", "-Command", query], capture_output=True,
                                text=True)
        try:
            return int(float(result.stdout.strip()))
        except ValueError:
            return None

    def close(self):
        if not hasattr(self, "process"):
            return
        if getattr(self, "hwnd", None) and user32.IsWindow(self.hwnd):
            user32.PostMessageW(self.hwnd, 0x0010, 0, None)
            # Answer the unsaved-changes question with "don't save".
            time.sleep(1.0)
            front = user32.GetForegroundWindow()
            if front and window_pid(front) == self.process.pid and front != self.hwnd:
                tap(ord("N"))
        try:
            self.process.wait(15)
        except subprocess.TimeoutExpired:
            self.process.kill()
        self.log.close()


def summary(values):
    if not values:
        return {}
    ordered = sorted(values)

    def percentile(p):
        return ordered[min(len(ordered) - 1, max(0, math.ceil(p * len(ordered)) - 1))]

    return {"n": len(values), "p50": round(percentile(0.5), 3), "p95": round(percentile(0.95), 3),
            "max": round(ordered[-1], 3), "mean": round(statistics.fmean(values), 3)}


def draw_stroke(app, center, radius, steps, record):
    """One curved stroke of `steps` moves with 15-40ms gaps. `record` gets
    (kind, cpu ms, span ms) for each move and for the release."""
    x, y = center
    if not app.under_cursor(x + radius, y):
        raise SystemExit("the canvas is not under the stroke")
    send([mouse_move(x + radius, y)])
    time.sleep(0.05)
    app.check_front()
    send([mouse_button(MOUSE_DOWN)])
    time.sleep(0.05)
    for step in range(1, steps + 1):
        angle = step / steps * math.pi * 1.5
        reach = radius * (1.0 - 0.4 * step / steps)
        gap = random.uniform(0.015, 0.040)
        sent = time.perf_counter()
        send([mouse_move(x + reach * math.cos(angle), y + reach * math.sin(angle))])
        cpu, span = app.thread.watch(sent + gap)
        record("move", cpu, span * 1000)
    app.check_front()
    sent = time.perf_counter()
    send([mouse_button(MOUSE_UP)])
    cpu, span = app.thread.watch(sent + 1.0)
    record("up", cpu, span * 1000)
    app.thread.quiet(0.3)


def measure_strokes(app, args):
    rows = []
    for index in range(args.repeat):
        fx = 0.3 + 0.4 * (index % 5) / 4
        fy = 0.35 + 0.3 * ((index // 5) % 3) / 2
        center = app.canvas_point(fx, fy)
        draw_stroke(app, center, 60, args.steps,
                    lambda kind, cpu, span: rows.append({"stroke": index, "kind": kind, "cpu_ms": cpu,
                                                         "span_ms": span}))
    moves = [row for row in rows if row["kind"] == "move"]
    ups = [row for row in rows if row["kind"] == "up"]
    half = len(ups) // 2
    return rows, {
        "move_cpu_ms": summary([row["cpu_ms"] for row in moves]),
        "move_span_ms": summary([row["span_ms"] for row in moves]),
        "up_cpu_ms": summary([row["cpu_ms"] for row in ups]),
        "up_span_ms": summary([row["span_ms"] for row in ups]),
        "up_cpu_ms_first_half": summary([row["cpu_ms"] for row in ups[:half]]),
        "up_cpu_ms_second_half": summary([row["cpu_ms"] for row in ups[half:]]),
    }


def make_dirty(app):
    """Draws a short stroke so that the title gains its modified mark. Returns
    False when the document refuses it (fixture 4 is at the stroke limit)."""
    title = lambda: window_text(app.hwnd).split(" — ")[0]
    if "*" in title():
        return True
    draw_stroke(app, app.canvas_point(0.5, 0.5), 40, 8, lambda *row: None)
    deadline = time.perf_counter() + 2.0
    while time.perf_counter() < deadline:
        if "*" in title():
            return True
        time.sleep(0.01)
    return False


def measure_save(app, args):
    # QSaveFile renames a finished temporary file over the document, which
    # gives the file a new id: that moment is the save's end, with or without
    # unsaved changes. The title losing its mark comes after, on the UI thread.
    rows = []
    for _ in range(args.repeat):
        dirty = make_dirty(app)
        app.thread.quiet(0.5)
        app.check_front()
        file_id = os.stat(app.document).st_ino
        sent = time.perf_counter()
        chord(ord("S"))
        before = app.thread.cycles()
        wait_for(lambda: os.stat(app.document).st_ino != file_id, 120, "the saved file")
        replaced = time.perf_counter()
        if dirty:
            wait_for(lambda: "*" not in window_text(app.hwnd).split(" — ")[0], 120, "the saved title")
        done = time.perf_counter()
        rows.append({"save_ms": (replaced - sent) * 1000, "title_ms": (done - sent) * 1000 if dirty else None,
                     "ui_cpu_ms": (app.thread.cycles() - before) / app.thread.cycles_per_ms,
                     "bytes": os.path.getsize(app.document)})
        app.thread.quiet(0.5)
    titled = [row["title_ms"] for row in rows if row["title_ms"] is not None]
    return rows, {"save_ms": summary([row["save_ms"] for row in rows]), "title_ms": summary(titled),
                  "ui_cpu_ms": summary([row["ui_cpu_ms"] for row in rows]), "bytes": rows[-1]["bytes"]}


def file_dialog(app):
    front = user32.GetForegroundWindow()
    if front and window_pid(front) == app.process.pid and class_name(front) == "#32770":
        return front
    return None


def dialog_edit(dialog):
    combo_ex = user32.FindWindowExW(ctypes.c_void_p(dialog), None, "ComboBoxEx32", None)
    combo = user32.FindWindowExW(ctypes.c_void_p(combo_ex), None, "ComboBox", None) if combo_ex else None
    edit = user32.FindWindowExW(ctypes.c_void_p(combo), None, "Edit", None) if combo else None
    if not edit:
        # Save dialogs keep the name box under a DUIViewWndClassName tree.
        found = []

        @ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
        def visit(hwnd, _):
            if class_name(hwnd) == "Edit" and user32.IsWindowVisible(hwnd):
                found.append(hwnd)
            return True

        user32.EnumChildWindows(ctypes.c_void_p(dialog), visit, 0)
        edit = found[0] if found else None
    if not edit:
        raise SystemExit("no file name box in the dialog")
    return edit


def choose_in_dialog(app, path):
    dialog = wait_for(lambda: file_dialog(app), 30, "the file dialog")
    time.sleep(0.5)
    user32.SendMessageW(ctypes.c_void_p(dialog_edit(dialog)), 0x000C, 0, path)
    time.sleep(0.2)
    app.check_front()
    sent = time.perf_counter()
    tap(VK_RETURN)
    wait_for(lambda: not user32.IsWindow(ctypes.c_void_p(dialog)), 30, "the dialog to close")
    return sent


def measure_open(app, args):
    # Alternate with a second copy so every open reads the fixture from scratch
    # through the same path.
    other = os.path.join(app.out, "other" + os.path.splitext(app.document)[1])
    shutil.copyfile(app.document, other)
    rows = []
    for index in range(args.repeat):
        target = other if index % 2 == 0 else app.document
        name = os.path.splitext(os.path.basename(target))[0]
        app.check_front()
        chord(ord("O"))
        sent = choose_in_dialog(app, target)
        before = app.thread.cycles()
        wait_for(lambda: window_text(app.hwnd).startswith(name), 300, "the opened title")
        done = time.perf_counter()
        rows.append({"open_ms": (done - sent) * 1000,
                     "ui_cpu_ms": (app.thread.cycles() - before) / app.thread.cycles_per_ms})
        # Opening keeps the playback state, which is paused.
        app.thread.quiet(1.0)
    return rows, {"open_ms": summary([row["open_ms"] for row in rows]),
                  "ui_cpu_ms": summary([row["ui_cpu_ms"] for row in rows])}


def progress_dialog(app):
    # The title is translated, so take the app's only other visible window.
    others = [hwnd for hwnd in visible_windows(app.process.pid) if hwnd != app.hwnd]
    return others[0] if len(others) == 1 else None


def measure_cancel(app, args):
    rows = []
    for index in range(args.repeat):
        app.check_front()
        chord(ord("E"))
        options = wait_for(lambda: (lambda front: front if front and front != app.hwnd
                                    and window_pid(front) == app.process.pid else None)(
            user32.GetForegroundWindow()), 30, "the export options")
        time.sleep(0.3)
        tap(VK_RETURN)
        wait_for(lambda: not user32.IsWindowVisible(ctypes.c_void_p(options)), 30, "the options to close")
        target = os.path.join(app.out, f"export-{index}-{int(time.time() * 1000)}.gif")
        choose_in_dialog(app, target)
        progress = wait_for(lambda: progress_dialog(app), 30, "the export progress")
        time.sleep(random.uniform(0.8, 1.6))
        if not user32.IsWindowVisible(ctypes.c_void_p(progress)):
            raise SystemExit("the export finished before the cancel")
        app.check_front()
        canceled_before = app.log_times("Export canceled")
        sent, sent_wall = time.perf_counter(), time.time()
        tap(VK_ESCAPE)
        wait_for(lambda: not user32.IsWindow(ctypes.c_void_p(progress))
                 or not user32.IsWindowVisible(ctypes.c_void_p(progress)), 120, "the progress to close")
        hidden = time.perf_counter()
        # The dialog hides at once; the export has stopped when the UI thread
        # logs the worker's finish, so that log line's timestamp is the end.
        stopped = wait_for(lambda: app.log_times("Export canceled")[len(canceled_before):], 120,
                           "the canceled export to finish")[0]
        rows.append({"cancel_ms": (stopped - sent_wall) * 1000, "dialog_hidden_ms": (hidden - sent) * 1000,
                     "partial_file": os.path.exists(target)})
        app.thread.quiet(1.0)
    return rows, {"cancel_ms": summary([row["cancel_ms"] for row in rows]),
                  "dialog_hidden_ms": summary([row["dialog_hidden_ms"] for row in rows]),
                  "partial_files": sum(row["partial_file"] for row in rows)}


def measure_memory(app, args):
    rows = [dict(app.memory(), phase="opened, paused", gpu_dedicated=app.gpu_dedicated())]
    app.check_front()
    tap(ord("P"))
    deadline = time.perf_counter() + args.play_seconds
    peak = {}
    while time.perf_counter() < deadline:
        for name, value in app.memory().items():
            peak[name] = max(peak.get(name, 0), value)
        time.sleep(0.25)
    rows.append(dict(peak, phase=f"playing {args.play_seconds}s, maximum", gpu_dedicated=app.gpu_dedicated()))
    tap(ord("P"))
    return rows, {"rows": rows, "window_shown_s": round(app.shown, 3)}


def qpc():
    value = ctypes.c_int64()
    kernel32.QueryPerformanceCounter(ctypes.byref(value))
    return value.value


def qpc_frequency():
    value = ctypes.c_int64()
    kernel32.QueryPerformanceFrequency(ctypes.byref(value))
    return value.value


PRESENTMON_SESSION = "ugurugu-baseline-probe"


def start_presentmon(presentmon, exe, csv_path, seconds):
    """Starts PresentMon before the app, as it misses swap chains created
    before its trace. It stops by itself after `seconds`, which flushes every
    row. Needs administrator rights."""
    process = subprocess.Popen(
        [presentmon, "--process_name", os.path.basename(exe), "--output_file", csv_path, "--qpc_time",
         "--no_track_input", "--timed", str(int(seconds)), "--terminate_after_timed",
         "--stop_existing_session", "--no_console_stats", "--session_name", PRESENTMON_SESSION],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(100):
        time.sleep(0.1)
        if subprocess.run(["logman", "query", PRESENTMON_SESSION, "-ets"], capture_output=True).returncode == 0:
            return process
    process.kill()
    raise SystemExit("PresentMon did not start recording; it needs administrator rights")


def displayed_frames(csv_path):
    """QPC display times of the frames that reached the screen."""
    import csv
    frequency = qpc_frequency()
    times = []
    with open(csv_path, encoding="utf-8-sig", newline="") as file:
        for row in csv.DictReader(file):
            if row["MsUntilDisplayed"] not in ("", "NA"):
                times.append(int(row["TimeInQPC"]) + float(row["MsUntilDisplayed"]) / 1000 * frequency)
    return sorted(times)


def measure_playback(app, args):
    """The C++ app starts playing at launch. Records displayed frames for
    `--play-seconds`: the first pass renders and caches every frame (cold),
    later passes show cached frames (warm)."""
    frequency = qpc_frequency()
    started = app.playback_started
    time.sleep(args.play_seconds)
    app.close()
    app.presentmon.wait(args.play_seconds + 60)
    times = [(t - started) / frequency for t in displayed_frames(app.presentmon_csv) if t >= started]
    intervals = [(b - a) * 1000 for a, b in zip(times, times[1:])]
    rows = [{"t": round(t, 4)} for t in times]
    # The document's frame rate; slower intervals than 1.5 frames are misses.
    frame_ms = 1000 / args.document_fps
    per_second = {}
    for t in times:
        per_second[int(t)] = per_second.get(int(t), 0) + 1
    warm = [ms for t, ms in zip(times[1:], intervals) if t >= args.warm_after]
    cold = [ms for t, ms in zip(times[1:], intervals) if t < args.warm_after]
    return rows, {
        "displayed": len(times),
        "frames_per_second": [per_second.get(second, 0) for second in range(int(args.play_seconds))],
        "cold_interval_ms": summary(cold),
        "warm_interval_ms": summary(warm),
        "warm_misses": sum(1 for ms in warm if ms > frame_ms * 1.5),
        "warm_fps": round(len(warm) / max(1e-9, sum(warm) / 1000), 2) if warm else None,
    }


def measure_export(app, args):
    """Exports a GIF and, meanwhile, measures how long the UI thread takes to
    answer a sent WM_NULL: the delay any click or key would see."""
    rows = []
    app.check_front()
    chord(ord("E"))
    options = wait_for(lambda: (lambda front: front if front and front != app.hwnd
                                and window_pid(front) == app.process.pid else None)(
        user32.GetForegroundWindow()), 30, "the export options")
    time.sleep(0.3)
    tap(VK_RETURN)
    wait_for(lambda: not user32.IsWindowVisible(ctypes.c_void_p(options)), 30, "the options to close")
    target = os.path.join(app.out, f"export-{int(time.time() * 1000)}.gif")
    sent = choose_in_dialog(app, target)
    finished_before = app.log_times("Exported")
    result = ctypes.c_size_t()
    while not app.log_times("Exported")[len(finished_before):]:
        before = time.perf_counter()
        answered = user32.SendMessageTimeoutW(ctypes.c_void_p(app.hwnd), 0, 0, None, 0x0002, 5000,
                                              ctypes.byref(result))
        rtt = (time.perf_counter() - before) * 1000
        rows.append({"t": round(before - sent, 4), "rtt_ms": rtt, "answered": bool(answered)})
        time.sleep(0.02)
        if time.perf_counter() - sent > 600:
            raise SystemExit("the export did not finish in 10 minutes")
    done = app.log_times("Exported")[len(finished_before)]
    return rows, {"export_s": round(done - (time.time() - (time.perf_counter() - sent)), 3),
                  "ui_rtt_ms": summary([row["rtt_ms"] for row in rows]),
                  "unanswered": sum(1 for row in rows if not row["answered"]),
                  "bytes": os.path.getsize(target) if os.path.exists(target) else None}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=["stroke", "save", "open", "cancel", "memory", "playback", "export"])
    parser.add_argument("exe")
    parser.add_argument("fixture")
    parser.add_argument("out")
    parser.add_argument("--repeat", type=int, default=20)
    parser.add_argument("--steps", type=int, default=40)
    parser.add_argument("--play-seconds", type=float, default=60.0)
    parser.add_argument("--presentmon")
    parser.add_argument("--thread", help="measure the thread with this name, not the window's")
    parser.add_argument("--no-play-key", action="store_true", help="do not press P at the start")
    parser.add_argument("--document-fps", type=float, default=25.0)
    parser.add_argument("--warm-after", type=float, default=40.0,
                        help="seconds after which playback counts as warm")
    args = parser.parse_args()
    os.makedirs(args.out, exist_ok=True)
    random.seed(7)
    app = App.__new__(App)
    try:
        if args.mode == "playback":
            if not args.presentmon:
                raise SystemExit("playback needs --presentmon")
            app.presentmon_csv = os.path.join(os.path.abspath(args.out), "presentmon.csv")
            app.presentmon = start_presentmon(args.presentmon, args.exe, app.presentmon_csv,
                                              args.play_seconds + 30)
            app.playback_started = qpc()
            app.__init__(os.path.abspath(args.exe), os.path.abspath(args.fixture), os.path.abspath(args.out),
                         play_key=False, settle=False, thread=args.thread)
        else:
            app.__init__(os.path.abspath(args.exe), os.path.abspath(args.fixture), os.path.abspath(args.out),
                         play_key=not args.no_play_key, thread=args.thread)
        measure = {"stroke": measure_strokes, "save": measure_save, "open": measure_open,
                   "cancel": measure_cancel, "memory": measure_memory, "playback": measure_playback,
                   "export": measure_export}[args.mode]
        rows, result = measure(app, args)
    finally:
        app.close()
    result["cycles_per_ms"] = round(app.thread.cycles_per_ms)
    result["idle_ui_cpu_ms_per_s"] = None if math.isnan(app.idle_cpu_ms_per_s) else round(app.idle_cpu_ms_per_s, 3)
    with open(os.path.join(args.out, f"{args.mode}.json"), "w", encoding="utf-8") as file:
        json.dump({"result": result, "rows": rows}, file, indent=1)
    print(json.dumps(result, indent=1))


if __name__ == "__main__":
    sys.exit(main())
