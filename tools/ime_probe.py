# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

"""Drives a real Windows IME in the Rust M0 app's IME probe with key input and
records what the app reports (docs/rust/m0-evidence.md section 7).

Usage: python ime_probe.py <ko|ja> <ugurugu.exe> <output-dir> [window-x,window-y]

The window position picks the monitor, and so the scale the IME sees.

Input goes to the foreground window, so it stops before sending any when the
app is not in front. Needs the Microsoft Korean or Japanese IME and Pillow."""
import ctypes
import ctypes.wintypes as wt
import json
import os
import re
import subprocess
import sys
import time

from PIL import ImageGrab

user32 = ctypes.windll.user32
ctypes.windll.shcore.SetProcessDpiAwareness(2)
for name in ("GetForegroundWindow", "WindowFromPoint", "GetAncestor"):
    getattr(user32, name).restype = ctypes.c_void_p
user32.WindowFromPoint.argtypes = [wt.POINT]
user32.LoadKeyboardLayoutW.restype = ctypes.c_void_p
user32.PostMessageW.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_void_p]
user32.GetAncestor.argtypes = [ctypes.c_void_p, ctypes.c_uint]

LANGUAGE = sys.argv[1]
EXE = os.path.abspath(sys.argv[2])
OUT = sys.argv[3]
PLACE = [int(value) for value in sys.argv[4].split(",")] if len(sys.argv) > 4 else [100, 100]
os.makedirs(OUT, exist_ok=True)
LOG = os.path.join(OUT, "app.log")


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


assert ctypes.sizeof(INPUT) == 40


def send(inputs):
    array = (INPUT * len(inputs))(*inputs)
    if user32.SendInput(len(inputs), array, ctypes.sizeof(INPUT)) != len(inputs):
        raise SystemExit("SendInput refused")


def key(vk, up=False):
    return INPUT(type=1, u=_INPUT(ki=KEYBDINPUT(vk, 0, 2 if up else 0, 0, 0)))


def tap(vk, pause=0.06):
    check_front()
    send([key(vk), key(vk, True)])
    time.sleep(pause)


def chord(modifier, vk):
    check_front()
    send([key(modifier), key(vk), key(vk, True), key(modifier, True)])
    time.sleep(0.1)


def type_keys(letters):
    for letter in letters:
        tap(ord(letter.upper()))


def click(x, y):
    left, top = user32.GetSystemMetrics(76), user32.GetSystemMetrics(77)
    width, height = user32.GetSystemMetrics(78), user32.GetSystemMetrics(79)
    dx = int((x - left + 0.5) * 65536 / width)
    dy = int((y - top + 0.5) * 65536 / height)
    move = INPUT(type=0, u=_INPUT(mi=MOUSEINPUT(dx, dy, 0, 0x1 | 0x8000 | 0x4000, 0, 0)))
    send([move])
    time.sleep(0.15)
    under = user32.GetAncestor(user32.WindowFromPoint(wt.POINT(int(x), int(y))), 2)
    if under != HWND or user32.GetForegroundWindow() != HWND:
        raise SystemExit("the app is not under the cursor; no click sent")
    send([INPUT(type=0, u=_INPUT(mi=MOUSEINPUT(0, 0, 0, 0x2, 0, 0))),
          INPUT(type=0, u=_INPUT(mi=MOUSEINPUT(0, 0, 0, 0x4, 0, 0)))])
    time.sleep(0.3)


def check_front():
    if user32.GetForegroundWindow() != HWND:
        raise SystemExit("the app lost the foreground; stopping input")


def log_lines():
    with open(LOG, encoding="utf-8", errors="replace") as file:
        return file.read().splitlines()


def last_texts():
    for line in reversed(log_lines()):
        match = re.search(r'line=("(?:[^"\\]|\\.)*") text=("(?:[^"\\]|\\.)*")', line)
        if match and "ime probe text" in line:
            return json.loads(match.group(1)), json.loads(match.group(2))
    return "", ""


def count(event):
    return sum(1 for line in log_lines() if event in line)


def field_rects():
    for line in reversed(log_lines()):
        match = re.search(r"ime probe fields line=\[([^\]]*)\] text=\[([^\]]*)\]", line)
        if match:
            parse = lambda group: [float(value) for value in group.split(",")]
            return parse(match.group(1)), parse(match.group(2))
    raise SystemExit("no field layout logged")


def to_screen(rect):
    origin = wt.POINT(0, 0)
    user32.ClientToScreen(HWND, ctypes.byref(origin))
    return origin.x + (rect[0] + rect[2]) / 2, origin.y + (rect[1] + rect[3]) / 2


def find_window(pid):
    found = []

    @ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def visit(hwnd, _):
        owner = wt.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid and user32.IsWindowVisible(hwnd):
            found.append(hwnd)
        return True

    user32.EnumWindows(visit, 0)
    return found[0] if found else None


def screenshot(name):
    rect = wt.RECT()
    user32.GetWindowRect(HWND, ctypes.byref(rect))
    ImageGrab.grab(bbox=(rect.left, rect.top, rect.right, rect.bottom), all_screens=True).save(
        os.path.join(OUT, name))


VK_RETURN, VK_BACK, VK_SPACE, VK_CONTROL, VK_HANGUL, VK_HANJA, VK_MENU = 0x0D, 0x08, 0x20, 0x11, 0x15, 0x19, 0x12
VK_IME_ON = 0x16
WM_INPUTLANGCHANGEREQUEST = 0x50


def korean(results, line_rect, text_rect):
    # Find the IME mode: one consonant key shows a jamo in Hangul mode.
    click(*to_screen(line_rect))
    type_keys("r")
    time.sleep(0.3)
    line, _ = last_texts()
    tap(VK_BACK)
    if line == "r":
        tap(VK_HANGUL, 0.3)
    results["start mode"] = "alphanumeric" if line == "r" else f"hangul ({line!r})"

    # 1. Compose and commit with Enter in a single-line field.
    enter_before = count("canvas Enter")
    type_keys("gksrmf")
    tap(VK_RETURN, 0.4)
    results["1 line after Enter"] = last_texts()[0]
    results["1 canvas Enter from text Enter"] = count("canvas Enter") - enter_before

    # 2-4. Multi-line: erase within a syllable, final consonant moving, Hanja.
    click(*to_screen(text_rect))
    type_keys("dkssudgktpdy")
    # Backspace takes the vowel off the composing syllable, leaving its ㅇ.
    tap(VK_BACK)
    type_keys("y")
    tap(VK_RETURN)
    type_keys("rkqtdl")
    tap(VK_RETURN)
    type_keys("gks")
    tap(VK_HANJA, 0.8)
    screenshot("candidates.png")
    type_keys("1")
    time.sleep(0.3)
    tap(VK_RETURN, 0.4)
    results["2-4 text"] = last_texts()[1]
    results["2-4 canvas Enter from text"] = count("canvas Enter") - enter_before


def japanese(results, line_rect, text_rect):
    # Switch this window to the Japanese keyboard and turn the IME on.
    hkl = user32.LoadKeyboardLayoutW("00000411", 0)
    user32.PostMessageW(HWND, WM_INPUTLANGCHANGEREQUEST, 0, hkl)
    time.sleep(0.5)
    click(*to_screen(line_rect))
    tap(VK_IME_ON, 0.3)
    type_keys("a")
    time.sleep(0.3)
    results["start mode"] = repr(last_texts()[0])
    tap(VK_BACK)
    tap(VK_BACK)

    # 1. Romaji to kana, convert with Space, commit with Enter.
    enter_before = count("canvas Enter")
    type_keys("nihongo")
    tap(VK_SPACE, 0.4)
    tap(VK_RETURN, 0.4)
    results["1 line after Enter"] = last_texts()[0]
    results["1 canvas Enter from text Enter"] = count("canvas Enter") - enter_before

    # 2. A phrase converted in segments; 3. the candidate list.
    click(*to_screen(text_rect))
    type_keys("watashihagakuseidesu")
    tap(VK_SPACE, 0.6)
    tap(VK_RETURN, 0.4)
    tap(VK_RETURN, 0.3)
    type_keys("kanji")
    tap(VK_SPACE, 0.4)
    tap(VK_SPACE, 0.8)
    screenshot("candidates.png")
    tap(VK_RETURN, 0.4)
    results["2-3 text"] = last_texts()[1]
    results["2-3 canvas Enter from text"] = count("canvas Enter") - enter_before

env = dict(os.environ, UGURUGU_LOG="info,ugurugu::ime_probe=debug")
log_file = open(LOG, "w", encoding="utf-8")
app = subprocess.Popen([EXE], env=env, stdout=log_file, stderr=subprocess.STDOUT)
HWND = None
results = {}
try:
    for _ in range(100):
        time.sleep(0.1)
        HWND = find_window(app.pid)
        if HWND:
            break
    assert HWND, "no window"
    time.sleep(1.5)
    user32.SetWindowPos(HWND, 0, PLACE[0], PLACE[1], 1600, 1000, 0x0004)
    send([key(VK_MENU), key(VK_MENU, True)])
    user32.SetForegroundWindow(HWND)
    time.sleep(1.0)
    check_front()
    line_rect, text_rect = field_rects()

    (korean if LANGUAGE == "ko" else japanese)(results, line_rect, text_rect)

    # 5. Shortcuts reach the canvas once text editing has ended.
    client = wt.RECT()
    user32.GetClientRect(HWND, ctypes.byref(client))
    origin = wt.POINT(0, 0)
    user32.ClientToScreen(HWND, ctypes.byref(origin))
    click(origin.x + 200, origin.y + 200)
    enter_before, undo_before = count("canvas Enter"), count("canvas Ctrl+Z")
    tap(VK_RETURN, 0.3)
    chord(VK_CONTROL, ord("Z"))
    time.sleep(0.3)
    results["5 canvas Enter"] = count("canvas Enter") - enter_before
    results["5 canvas Ctrl+Z"] = count("canvas Ctrl+Z") - undo_before
    results["5 text after shortcuts"] = last_texts()[1]
    screenshot("final.png")
finally:
    if HWND:
        user32.PostMessageW(HWND, 0x0010, 0, 0)
    try:
        app.wait(5)
    except subprocess.TimeoutExpired:
        app.kill()
    log_file.close()

with open(os.path.join(OUT, "results.txt"), "w", encoding="utf-8") as file:
    for name, value in results.items():
        print(f"{name}: {value!r}", file=file)
