# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

"""Kills the 3.0 app at bad moments and checks what is left, for M5-15
(docs/rust/m5-plan.md, plan 8.2 and 9):

- save: kills it 0-60ms after Ctrl+S. The document must still read, as the
  old or the new version; a temporary file left beside it must go with the
  next save there.
- export: kills it during a GIF export. No file may appear at the chosen
  path, nor a temporary one beside it.
- recover: kills it once the edits have been written for recovery, starts it
  again and answers the question with Enter (Recover). The recovered work
  must hold every edit.
- windows: two windows share the recovery root; one is killed. The other's
  folder stays, and a new start offers only the killed window's work.

Usage: python kill_probe.py <ugurugu.exe> <ugu-doc.exe> <fixture.ugurugu>
       <output-dir> [--repeat N]"""
import argparse
import glob
import json
import os
import random
import shutil
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import baseline_probe as probe  # noqa: E402

VK_RETURN, VK_TAB = 0x0D, 0x09


OPEN = []


class Window:
    """The app on `document`, with the recovery root and settings in `root`."""

    def __init__(self, exe, document, root):
        env = dict(os.environ)
        env["UGURUGU_RECOVERY_PATH"] = os.path.join(root, "recovery")
        env["UGURUGU_SETTINGS_PATH"] = os.path.join(root, "settings.json")
        env["UGURUGU_LOG_PATH"] = os.path.join(root, "logs")
        env["UGURUGU_LANGUAGE"] = "en"
        env["UGURUGU_LOG"] = "info"
        self.log = open(os.path.join(root, f"app-{time.time():.3f}.log"), "w", encoding="utf-8")
        self.process = subprocess.Popen([exe] + ([document] if document else []), env=env, stdout=self.log,
                                        stderr=subprocess.STDOUT)
        OPEN.append(self)
        self.hwnd = probe.wait_for(self.main_window, 60, "the main window")
        time.sleep(1.5)
        probe.user32.SetWindowPos(self.hwnd, None, 0, 0, 2560, 1600, 0x0004)
        # Escape would answer the recovery question asked at start.
        self.front(escape=document is not None)

    def main_window(self):
        found = [hwnd for hwnd in probe.visible_windows(self.process.pid)
                 if probe.window_text(hwnd).endswith(" - Ugurugu")]
        return found[0] if found else None

    def title(self):
        return probe.window_text(self.hwnd)

    def front(self, escape=True):
        probe.send([probe.key(0x12), probe.key(0x12, True)])
        probe.user32.SetForegroundWindow(self.hwnd)
        time.sleep(0.4)
        # Alt alone goes to the menus; Escape leaves them.
        if escape:
            probe.tap(0x1B)
            time.sleep(0.2)
        front = probe.user32.GetForegroundWindow()
        if probe.window_pid(front) != self.process.pid:
            raise SystemExit("the app is not in front")

    def canvas_point(self, fx, fy):
        return probe.App.canvas_point(self, fx, fy)

    def under_cursor(self, x, y):
        return probe.App.under_cursor(self, x, y)

    def check_front(self):
        front = probe.user32.GetForegroundWindow()
        if probe.window_pid(front) != self.process.pid:
            raise SystemExit("the app lost the foreground")

    def stroke(self, fx=0.5, fy=0.5):
        x, y = self.canvas_point(fx, fy)
        probe.send([probe.mouse_move(x, y)])
        time.sleep(0.05)
        self.check_front()
        probe.send([probe.mouse_button(probe.MOUSE_DOWN)])
        for step in range(1, 16):
            probe.send([probe.mouse_move(x + step * 6, y + (step % 4) * 5)])
            time.sleep(0.02)
        probe.send([probe.mouse_button(probe.MOUSE_UP)])
        time.sleep(0.4)

    def kill(self):
        self.process.kill()
        self.process.wait(10)
        if not self.log.closed:
            self.log.close()

    def close(self):
        if self.process.poll() is None:
            self.kill()


def operations(ugu_doc, path):
    """The document's operation count, or the error when it does not read."""
    result = subprocess.run([ugu_doc, "info", path], capture_output=True, text=True)
    if result.returncode != 0:
        return None, result.stderr.strip() or result.stdout.strip()
    first = result.stdout.splitlines()[0]
    return int(first.split(" operations")[0].rsplit(" ", 1)[1]), None


def temporaries(folder):
    return sorted(os.path.basename(path) for path in glob.glob(os.path.join(folder, ".*.tmp"))
                  + glob.glob(os.path.join(folder, "*.tmp")))


def fresh_root(out, name):
    root = os.path.join(out, name)
    shutil.rmtree(root, ignore_errors=True)
    os.makedirs(root)
    return root


def kill_while_saving(args, out):
    rows = []
    for index in range(args.repeat):
        root = fresh_root(out, f"save-{index}")
        document = os.path.join(root, "document.ugurugu")
        shutil.copyfile(args.fixture, document)
        before, _ = operations(args.ugu_doc, document)
        window = Window(args.exe, document, root)
        window.stroke()
        window.check_front()
        delay = random.uniform(0.0, 0.045)
        probe.chord(ord("S"))
        time.sleep(delay)
        window.kill()
        after, error = operations(args.ugu_doc, document)
        left = temporaries(root)
        # The next save there removes what the killed window left.
        again = Window(args.exe, document, root)
        again.stroke(0.6, 0.6)
        again.check_front()
        file_id = probe.file_id_of(document)
        probe.chord(ord("S"))
        probe.wait_for(lambda: probe.file_id_of(document) not in (file_id, None), 60, "the next save")
        time.sleep(0.3)
        again.kill()
        rows.append({"delay_ms": round(delay * 1000, 1), "operations_before": before, "operations_after": after,
                     "error": error, "temporaries": left, "after_next_save": temporaries(root)})
        print(json.dumps(rows[-1]), flush=True)
    return rows


def kill_while_exporting(args, out):
    rows = []
    for index in range(args.repeat):
        root = fresh_root(out, f"export-{index}")
        document = os.path.join(root, "document.ugurugu")
        shutil.copyfile(args.fixture, document)
        window = Window(args.exe, document, root)
        window.check_front()
        probe.chord(ord("E"))
        time.sleep(0.6)
        probe.tap(VK_TAB)
        time.sleep(0.2)
        probe.tap(VK_RETURN)
        target = os.path.join(root, "animation.gif")
        probe.choose_in_dialog(window, target)
        delay = random.uniform(args.kill_from, args.kill_to)
        time.sleep(delay)
        window.kill()
        rows.append({"delay_s": round(delay, 2), "target_exists": os.path.exists(target),
                     "temporaries": temporaries(root)})
        print(json.dumps(rows[-1]), flush=True)
    return rows


def kill_and_recover(args, out):
    rows = []
    for index in range(args.repeat):
        root = fresh_root(out, f"recover-{index}")
        document = os.path.join(root, "document.ugurugu")
        shutil.copyfile(args.fixture, document)
        before, _ = operations(args.ugu_doc, document)
        window = Window(args.exe, document, root)
        strokes = 2 + index % 3
        for stroke in range(strokes):
            window.stroke(0.4 + 0.05 * stroke, 0.45)
        # Written 3s after editing pauses.
        time.sleep(4.5)
        window.kill()
        folders = [path for path in glob.glob(os.path.join(root, "recovery", "*")) if os.path.isdir(path)]
        written = []
        for folder in folders:
            for generation in glob.glob(os.path.join(folder, "*.ugurugu")):
                written.append(operations(args.ugu_doc, generation)[0])
        again = Window(args.exe, None, root)
        time.sleep(1.0)
        again.check_front()
        probe.tap(VK_RETURN)
        recovered_title = probe.wait_for(lambda: (lambda title: title if "recovered" in title else None)(
            again.title()), 20, "the recovered title")
        # Saved under the suggested name to count what came back.
        probe.chord(ord("S"))
        saved = os.path.join(root, "document-recovered.ugurugu")
        dialog = probe.wait_for(lambda: probe.file_dialog(again), 20, "the save dialog")
        time.sleep(0.4)
        probe.user32.SendMessageW(probe.ctypes.c_void_p(probe.dialog_edit(dialog)), 0x000C, 0, saved)
        time.sleep(0.2)
        probe.tap(VK_RETURN)
        probe.wait_for(lambda: os.path.exists(saved), 20, "the recovered document")
        time.sleep(0.5)
        recovered, error = operations(args.ugu_doc, saved)
        again.kill()
        rows.append({"strokes": strokes, "operations_before": before, "generations": sorted(written),
                     "title": recovered_title, "recovered_operations": recovered, "error": error})
        print(json.dumps(rows[-1]), flush=True)
    return rows


def two_windows(args, out):
    root = fresh_root(out, "windows")
    first_document = os.path.join(root, "first.ugurugu")
    second_document = os.path.join(root, "second.ugurugu")
    shutil.copyfile(args.fixture, first_document)
    shutil.copyfile(args.fixture, second_document)
    first = Window(args.exe, first_document, root)
    first.stroke()
    second = Window(args.exe, second_document, root)
    second.stroke(0.6, 0.6)
    time.sleep(4.5)
    folders_before = sorted(os.listdir(os.path.join(root, "recovery")))
    first.kill()
    third = Window(args.exe, None, root)
    time.sleep(1.0)
    offered = probe.window_text(third.hwnd)
    third.check_front()
    probe.tap(VK_RETURN)
    title = probe.wait_for(lambda: (lambda title: title if "recovered" in title else None)(third.title()),
                           20, "the recovered title")
    time.sleep(0.5)
    # Nothing more is offered: Escape would answer a second question.
    second_alive = second.process.poll() is None
    folders_after = sorted(os.listdir(os.path.join(root, "recovery")))
    third.kill()
    second.kill()
    row = {"folders_before_kill": folders_before, "folders_after_recovery": folders_after,
           "title_before": offered, "recovered_title": title, "second_still_running": second_alive}
    print(json.dumps(row), flush=True)
    return [row]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("exe")
    parser.add_argument("ugu_doc")
    parser.add_argument("fixture")
    parser.add_argument("out")
    parser.add_argument("--repeat", type=int, default=10)
    parser.add_argument("--only", choices=["save", "export", "recover", "windows"])
    parser.add_argument("--kill-from", type=float, default=1.5,
                        help="earliest kill during an export, in seconds after choosing the file")
    parser.add_argument("--kill-to", type=float, default=4.0)
    args = parser.parse_args()
    args.exe, args.ugu_doc, args.fixture = map(os.path.abspath, (args.exe, args.ugu_doc, args.fixture))
    out = os.path.abspath(args.out)
    os.makedirs(out, exist_ok=True)
    random.seed(5)
    result = {}
    try:
        for name, run in [("save", kill_while_saving), ("export", kill_while_exporting),
                          ("recover", kill_and_recover), ("windows", two_windows)]:
            if args.only in (None, name):
                result[name] = run(args, out)
    finally:
        for window in OPEN:
            window.close()
    with open(os.path.join(out, "kill.json"), "w", encoding="utf-8") as file:
        json.dump(result, file, indent=1)


if __name__ == "__main__":
    sys.exit(main())
