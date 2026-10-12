# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

"""A long work session by automatic input, for M5-15 (docs/rust/decisions.md):
strokes, undo and redo, a selection moved and applied (3.0 only), saves,
playback and GIF exports, repeated for `--minutes`. Each round records the
work of the thread that handles input per stroke move and pen-up, and the
process's memory, handles and GDI/USER objects; every fifth round its GPU
memory too.

The document grows by three strokes a round, so memory may grow with it. To
tell that from a leak, the saved document is opened afresh at the end and
measured the same way.

Usage: python session_probe.py <app.exe> <fixture> <output-dir> [--minutes M]
       [--thread render] [--no-transform] [--no-play-key]

Uses baseline_probe.py's app control; the app gets its own instance lock,
recovery path and settings. Input goes to the foreground window and stops
if the app is not in front."""
import argparse
import ctypes
import json
import os
import random
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import baseline_probe as probe  # noqa: E402

user32 = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32
kernel32.GetProcessHandleCount.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint32)]
user32.GetGuiResources.argtypes = [ctypes.c_void_p, ctypes.c_uint32]

VK_RETURN = 0x0D
VK_CONTROL = 0x11


def objects(pid):
    handle = kernel32.OpenProcess(0x1000, False, pid)
    count = ctypes.c_uint32()
    kernel32.GetProcessHandleCount(ctypes.c_void_p(handle), ctypes.byref(count))
    gdi = user32.GetGuiResources(ctypes.c_void_p(handle), 0)
    user = user32.GetGuiResources(ctypes.c_void_p(handle), 1)
    kernel32.CloseHandle(ctypes.c_void_p(handle))
    return {"handles": count.value, "gdi": gdi, "user": user}


def log_count(app, text):
    app.log.flush()
    with open(app.log.name, encoding="utf-8", errors="replace") as file:
        return sum(1 for line in file if text in line)


def drag(app, start, end, steps=12):
    probe.send([probe.mouse_move(*start)])
    time.sleep(0.05)
    app.check_front()
    probe.send([probe.mouse_button(probe.MOUSE_DOWN)])
    for step in range(1, steps + 1):
        t = step / steps
        probe.send([probe.mouse_move(start[0] + (end[0] - start[0]) * t, start[1] + (end[1] - start[1]) * t)])
        time.sleep(0.02)
    probe.send([probe.mouse_button(probe.MOUSE_UP)])
    time.sleep(0.2)


def save(app):
    file_id = probe.file_id_of(app.document)
    app.check_front()
    sent = time.perf_counter()
    probe.chord(ord("S"))
    probe.wait_for(lambda: probe.file_id_of(app.document) not in (file_id, None), 120, "the saved file")
    return (time.perf_counter() - sent) * 1000


def export_gif(app, cpp, round_index):
    word = "Exported" if cpp else "exported"
    before = log_count(app, word)
    app.check_front()
    probe.chord(ord("E"))
    if cpp:
        options = probe.wait_for(lambda: (lambda front: front if front and front != app.hwnd
                                          and probe.window_pid(front) == app.process.pid else None)(
            user32.GetForegroundWindow()), 30, "the export options")
        time.sleep(0.3)
        probe.tap(VK_RETURN)
        probe.wait_for(lambda: not user32.IsWindowVisible(ctypes.c_void_p(options)), 30, "the options to close")
    else:
        # The options are a dialog inside the window, focused on the size;
        # Tab goes to Export.
        time.sleep(0.6)
        probe.tap(0x09)
        time.sleep(0.2)
        probe.tap(VK_RETURN)
    target = os.path.join(app.out, f"export-{round_index}.gif")
    sent = probe.choose_in_dialog(app, target)
    probe.wait_for(lambda: log_count(app, word) > before, 600, "the export to finish")
    seconds = time.perf_counter() - sent
    size = os.path.getsize(target) if os.path.exists(target) else None
    if size:
        os.remove(target)
    return seconds, size


def transform(app):
    """Select all, move it by dragging inside, apply, deselect."""
    app.check_front()
    probe.chord(ord("A"))
    time.sleep(0.3)
    probe.chord(ord("T"))
    time.sleep(0.5)
    start = app.canvas_point(0.5, 0.5)
    drag(app, start, (start[0] + 30, start[1] + 20))
    app.check_front()
    probe.tap(VK_RETURN)
    time.sleep(0.5)
    probe.chord(ord("D"))
    time.sleep(0.3)
    probe.tap(ord("B"))
    time.sleep(0.2)


def sample(app, with_gpu):
    row = dict(app.memory(), **objects(app.process.pid))
    if with_gpu:
        row["gpu_dedicated"] = app.gpu_dedicated()
    return row


class LASTINPUTINFO(ctypes.Structure):
    _fields_ = [("cbSize", ctypes.c_uint32), ("dwTime", ctypes.c_uint32)]


def idle_seconds():
    info = LASTINPUTINFO(ctypes.sizeof(LASTINPUTINFO), 0)
    user32.GetLastInputInfo(ctypes.byref(info))
    return (kernel32.GetTickCount() - info.dwTime) / 1000


def take_back(app, quiet_for=60):
    """Someone else used the PC: let go of the mouse, wait until nobody has
    touched it for `quiet_for` seconds, then bring the app back."""
    probe.send([probe.mouse_button(probe.MOUSE_UP)])
    print(json.dumps({"paused": round(time.perf_counter(), 1)}), flush=True)
    while idle_seconds() < quiet_for:
        time.sleep(5)
    probe.send([probe.key(0x12), probe.key(0x12, True)])
    user32.SetForegroundWindow(app.hwnd)
    time.sleep(0.5)
    probe.tap(0x1B)
    time.sleep(0.3)
    app.check_front()


def one_round(app, args, index, t, moves, ups):
    row = {"round": index, "t_s": round(t, 1)}
    for stroke in range(4):
        fx = 0.3 + 0.4 * random.random()
        fy = 0.3 + 0.4 * random.random()

        def record(kind, cpu, span):
            (moves if kind == "move" else ups).append({"t_s": round(t, 1), "cpu_ms": cpu, "span_ms": span})

        probe.draw_stroke(app, app.canvas_point(fx, fy), 50 + 30 * random.random(), 30, record)
    app.check_front()
    for vk in (ord("Z"), ord("Z"), ord("Y")):
        probe.chord(vk)
        time.sleep(0.15)
    if not args.no_transform:
        transform(app)
    app.thread.quiet(0.5)
    row["save_ms"] = round(save(app), 1)
    if index % 3 == 1:
        app.check_front()
        probe.tap(ord("P"))
        time.sleep(3.0)
        app.check_front()
        probe.tap(ord("P"))
        time.sleep(0.3)
    if index % 6 == 5:
        seconds, size = export_gif(app, args.cpp, index)
        row["export_s"] = round(seconds, 2)
        row["export_bytes"] = size
    app.thread.quiet(0.5, timeout=300)
    return row


def session(app, args, log):
    rounds, moves, ups = [], [], []
    started = time.perf_counter()
    paused = 0.0
    index = 0
    while time.perf_counter() - started - paused < args.minutes * 60:
        t = time.perf_counter() - started - paused
        try:
            row = one_round(app, args, index, t, moves, ups)
        except SystemExit as stop:
            if "foreground" not in str(stop) and "front" not in str(stop):
                raise
            before = time.perf_counter()
            take_back(app)
            paused += time.perf_counter() - before
            continue
        row.update(sample(app, index % 5 == 0))
        this = [move for move in moves if move["t_s"] == row["t_s"]]
        row["move_cpu_ms"] = probe.summary([move["cpu_ms"] for move in this])
        row["up_cpu_ms"] = probe.summary([up["cpu_ms"] for up in ups if up["t_s"] == row["t_s"]])
        rounds.append(row)
        log.write(json.dumps(row) + "\n")
        log.flush()
        print(json.dumps(row), flush=True)
        index += 1
    return rounds, moves, ups


def window(rows, key, start, end):
    return probe.summary([row[key] for row in rows if start <= row["t_s"] < end])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("exe")
    parser.add_argument("fixture")
    parser.add_argument("out")
    parser.add_argument("--minutes", type=float, default=60.0)
    parser.add_argument("--thread")
    parser.add_argument("--no-transform", action="store_true")
    parser.add_argument("--no-play-key", action="store_true")
    parser.add_argument("--cpp", action="store_true", help="the app is 2.2.13")
    args = parser.parse_args()
    out = os.path.abspath(args.out)
    os.makedirs(out, exist_ok=True)
    os.environ["UGURUGU_SETTINGS_PATH"] = os.path.join(out, "settings.json")
    os.environ["UGURUGU_LOG_PATH"] = os.path.join(out, "logs")
    random.seed(11)
    app = probe.App.__new__(probe.App)
    try:
        app.__init__(os.path.abspath(args.exe), os.path.abspath(args.fixture), out,
                     play_key=not args.no_play_key, thread=args.thread)
        opened = sample(app, True)
        with open(os.path.join(out, "rounds.jsonl"), "w", encoding="utf-8") as log:
            rounds, moves, ups = session(app, args, log)
        final = sample(app, True)
    finally:
        app.close()
    # The same document opened afresh: what the content alone holds.
    fresh_out = os.path.join(out, "fresh")
    os.makedirs(fresh_out, exist_ok=True)
    fresh = probe.App.__new__(probe.App)
    try:
        fresh.__init__(os.path.abspath(args.exe), app.document, fresh_out, play_key=not args.no_play_key,
                       thread=args.thread)
        # Played once, as the session did, so its frames are cached the same way.
        fresh.check_front()
        probe.tap(ord("P"))
        time.sleep(3.0)
        probe.tap(ord("P"))
        fresh.thread.quiet(0.5, timeout=300)
        reopened = sample(fresh, True)
    finally:
        fresh.close()
    end = rounds[-1]["t_s"] if rounds else 0
    tenth = max(60.0, end / 6)
    result = {
        "rounds": len(rounds),
        "minutes": round(end / 60, 1),
        "opened": opened,
        "final": final,
        "fresh_open_of_final_document": reopened,
        "move_cpu_ms_first": window(moves, "cpu_ms", 0, tenth),
        "move_cpu_ms_last": window(moves, "cpu_ms", end - tenth, end + 1),
        "move_span_ms_first": window(moves, "span_ms", 0, tenth),
        "move_span_ms_last": window(moves, "span_ms", end - tenth, end + 1),
        "up_cpu_ms_first": window(ups, "cpu_ms", 0, tenth),
        "up_cpu_ms_last": window(ups, "cpu_ms", end - tenth, end + 1),
        "save_ms": probe.summary([row["save_ms"] for row in rounds]),
        "export_s": probe.summary([row["export_s"] for row in rounds if "export_s" in row]),
        "document_bytes": os.path.getsize(app.document),
    }
    with open(os.path.join(out, "session.json"), "w", encoding="utf-8") as file:
        json.dump({"result": result, "rounds": rounds, "moves": moves, "ups": ups}, file, indent=1)
    print(json.dumps(result, indent=1))


if __name__ == "__main__":
    sys.exit(main())
