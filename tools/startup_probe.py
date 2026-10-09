# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

"""Start-up of 2.2.13 and 3.0 with a document (docs/rust/m0-evidence.md
sections 13 and 14): from launching the process to its main window being
visible, and to its first frame on screen (PresentMon, started before the
app). For 3.0 the app's log also splits the time into the steps it logs.

Usage: python startup_probe.py <app.exe> <fixture> <output-dir> <PresentMon.exe> [runs]

Each launch gets a fresh copy of the fixture, its own instance lock and
recovery path. PresentMon needs administrator rights."""
import datetime
import os
import shutil
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import baseline_probe as bp  # noqa: E402

# Log lines of 3.0 that mark a step of start-up, in order.
STEPS = [
    ("starting", "starting"),
    ("window shown", "window shown"),
    ("graphics instance created", "instance"),
    ("window surface created", "surface"),
    ("selected GPU adapter", "adapter"),
    ("GPU device opened", "device"),
    ("ugu_render::present: swap chain", "swap chain"),
    ("ugurugu::render: frame", "first frame drawn"),
]


def log_steps(path, launched):
    """Milliseconds from `launched` (epoch seconds) to each step's first log line."""
    found = {}
    with open(path, encoding="utf-8", errors="replace") as file:
        for line in file:
            for marker, name in STEPS:
                if name not in found and marker in line:
                    stamp = datetime.datetime.strptime(line[:26], "%Y-%m-%dT%H:%M:%S.%f")
                    stamp = stamp.replace(tzinfo=datetime.timezone.utc).timestamp()
                    found[name] = (stamp - launched) * 1000
    return found


def p50(values):
    return sorted(values)[len(values) // 2]


def main():
    exe, fixture, out, presentmon = sys.argv[1:5]
    runs = int(sys.argv[5]) if len(sys.argv) > 5 else 5
    os.makedirs(out, exist_ok=True)
    frequency = bp.qpc_frequency()
    shown, first, steps = [], [], []
    for run in range(runs):
        folder = os.path.join(out, f"run{run}")
        os.makedirs(folder, exist_ok=True)
        document = os.path.join(folder, "document" + os.path.splitext(fixture)[1])
        shutil.copyfile(fixture, document)
        env = dict(os.environ)
        env["UGURUGU_INSTANCE_LOCK_PATH"] = os.path.join(folder, "instance.lock")
        env["UGURUGU_RECOVERY_PATH"] = os.path.join(folder, "recovery.ugu")
        env.setdefault("UGURUGU_LOG", "info,ugurugu=debug,ugu_render=debug,wgpu_core=warn,wgpu_hal=warn")
        env["PATH"] = os.pathsep.join([os.environ.get("UGURUGU_QT_BIN", r"C:\Qt\6.11.2\msvc2022_64\bin"),
                                       os.path.join(os.environ["LOCALAPPDATA"], "Ugurugu", "current"), env["PATH"]])
        csv_path = os.path.join(folder, "presentmon.csv")
        pm = bp.start_presentmon(presentmon, exe, csv_path, 20)
        log_path = os.path.join(folder, "app.log")
        log = open(log_path, "w", encoding="utf-8")
        launched_qpc = bp.qpc()
        launched_wall = time.time()
        launched = time.perf_counter()
        process = subprocess.Popen([exe, document], env=env, stdout=log, stderr=subprocess.STDOUT)
        probe = bp.App.__new__(bp.App)
        probe.process = process
        bp.wait_for(probe.main_window, 60, "the main window")
        shown.append((time.perf_counter() - launched) * 1000)
        time.sleep(8)
        process.kill()
        pm.wait(60)
        log.close()
        frames = [t for t in bp.displayed_frames(csv_path) if t >= launched_qpc]
        first.append((frames[0] - launched_qpc) / frequency * 1000 if frames else float("nan"))
        steps.append(log_steps(log_path, launched_wall))
        detail = ", ".join(f"{name} {ms:.0f}" for name, ms in steps[-1].items())
        print(f"run {run}: window {shown[-1]:.0f} ms, first frame {first[-1]:.0f} ms; {detail}", flush=True)
        time.sleep(2)

    print(f"{os.path.basename(exe)} {os.path.basename(fixture)}: window p50 {p50(shown):.0f} "
          f"(min {min(shown):.0f}, max {max(shown):.0f}) ms, first frame p50 {p50(first):.0f} "
          f"(min {min(first):.0f}, max {max(first):.0f}) ms")
    for _, name in STEPS:
        values = [run[name] for run in steps if name in run]
        if values:
            print(f"  {name}: p50 {p50(values):.0f} (min {min(values):.0f}, max {max(values):.0f}) ms")


if __name__ == "__main__":
    main()
