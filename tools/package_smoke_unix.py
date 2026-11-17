# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

"""Unpacks the macOS or Linux 3.x package, checks its files and signature,
runs the app until it has chosen a GPU and ends it, as
tools/package_smoke.ps1 does for the Windows setup. Neither updates: that was tried by hand
(docs/rust/evidence.md section 129).

    python3 tools/package_smoke_unix.py macos artifacts/Ugurugu-3.7.0-macOS-arm64.dmg WORK
    python3 tools/package_smoke_unix.py linux artifacts/Ugurugu-3.7.0-Linux-x64.AppImage WORK

`--notarized` also asks Gatekeeper to accept the disk image and the app
and checks that both carry a stapled ticket.

The run uses its own settings, recovery and log folders under WORK and does
not check for updates. It ends with SIGTERM: Linux takes that as the end of
the session and writes recovery first (`ugu_platform::session`); macOS
apps are ended by Apple Events, which need the app to be registered with
Launch Services, so there the signal just stops it.
"""

import argparse
import os
import pathlib
import signal
import subprocess
import sys
import time

APP = "Ugurugu 3.app"
MACOS_FILES = [
    "Contents/Info.plist",
    "Contents/MacOS/UpdateMac",
    "Contents/MacOS/sq.version",
    "Contents/MacOS/ugurugu",
    "Contents/Resources/LICENSE.txt",
    "Contents/Resources/THIRD_PARTY_NOTICES.txt",
    "Contents/Resources/Ugurugu.icns",
    "Contents/Resources/sq.version",
    "Contents/_CodeSignature/CodeResources",
]
LINUX_FILES = ["LICENSE.txt", "THIRD_PARTY_NOTICES.txt", "UpdateNix", "sq.version", "ugurugu"]
READY = "selected GPU adapter"


def files_under(folder: pathlib.Path) -> list[str]:
    return sorted(path.relative_to(folder).as_posix() for path in folder.rglob("*") if path.is_file())


def expect(found: list[str], expected: list[str], what: str) -> None:
    if found != sorted(expected):
        raise SystemExit(f"{what} differ from the expected list: {', '.join(found)}")


def check_notarized(path: pathlib.Path, assessment: list[str]) -> None:
    subprocess.run(["spctl", "--assess", "--verbose=2", *assessment, str(path)], check=True)
    subprocess.run(["xcrun", "stapler", "validate", str(path)], check=True)


def unpack_macos(package: pathlib.Path, work: pathlib.Path, notarized: bool) -> list[str]:
    if notarized:
        check_notarized(package, ["--type", "open", "--context", "context:primary-signature"])
    volume = work / "volume"
    subprocess.run(
        ["hdiutil", "attach", "-nobrowse", "-readonly", "-mountpoint", str(volume), str(package)],
        check=True,
        stdout=subprocess.DEVNULL,
    )
    try:
        expect(sorted(path.name for path in volume.iterdir() if not path.name.startswith(".")), ["Applications", APP], "Disk image items")
        if os.readlink(volume / "Applications") != "/Applications":
            raise SystemExit("The disk image's Applications link points elsewhere")
        install = work / "install"
        # As a person dragging it to Applications would copy it.
        subprocess.run(["ditto", str(volume / APP), str(install / APP)], check=True)
    finally:
        subprocess.run(["hdiutil", "detach", str(volume)], check=True, stdout=subprocess.DEVNULL)
    app = install / APP
    # Stapling a notarized bundle adds its ticket as Contents/CodeResources.
    expect(files_under(app), MACOS_FILES + (["Contents/CodeResources"] if notarized else []), "Bundle files")
    subprocess.run(["codesign", "--verify", "--deep", "--strict", "--verbose=2", str(app)], check=True)
    if notarized:
        check_notarized(app, ["--type", "execute"])
    return [str(app / "Contents" / "MacOS" / "ugurugu")]


def unpack_linux(package: pathlib.Path, work: pathlib.Path) -> list[str]:
    subprocess.run([str(package.resolve()), "--appimage-extract"], cwd=work, check=True, stdout=subprocess.DEVNULL)
    root = work / "squashfs-root"
    expect(files_under(root / "usr" / "bin"), LINUX_FILES, "Packaged files")
    # AppRun execs the app, so the signal reaches it. The AppImage runtime
    # around it is not run: it mounts the image with FUSE, which containers
    # lack, and its extract-and-run mode does not pass SIGTERM on.
    return [str(root / "AppRun")]


def wait_until_ready(process: subprocess.Popen, logs: pathlib.Path, seconds: int) -> None:
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise SystemExit(f"The app exited with {process.returncode} while starting")
        for log in logs.glob("ugurugu-*.log"):
            if READY in log.read_text(encoding="utf-8", errors="replace"):
                return
        time.sleep(0.5)
    process.kill()
    raise SystemExit(f"The app chose no GPU within {seconds} seconds")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("system", choices=["macos", "linux"])
    parser.add_argument("package", type=pathlib.Path)
    parser.add_argument("work", type=pathlib.Path)
    parser.add_argument("--start-seconds", type=int, default=90)
    parser.add_argument("--notarized", action="store_true", help="macOS: expect a notarized disk image")
    args = parser.parse_args()
    if args.notarized and args.system != "macos":
        raise SystemExit("--notarized is for macOS")

    work = args.work.resolve()
    work.mkdir(parents=True)
    if args.system == "macos":
        command = unpack_macos(args.package, work, args.notarized)
    else:
        command = unpack_linux(args.package, work)

    logs = work / "logs"
    settings = work / "settings.json"
    settings.write_text('{"format": "ugurugu.settings", "version": 1, "checkForUpdates": false}\n', encoding="utf-8")
    environment = dict(
        os.environ,
        UGURUGU_SETTINGS_PATH=str(settings),
        UGURUGU_RECOVERY_PATH=str(work / "recovery"),
        UGURUGU_LOG_PATH=str(logs),
    )
    print("Running", flush=True)
    process = subprocess.Popen(command, env=environment)
    wait_until_ready(process, logs, args.start_seconds)
    process.send_signal(signal.SIGTERM)
    try:
        status = process.wait(30)
    except subprocess.TimeoutExpired:
        process.kill()
        raise SystemExit("The app did not end on SIGTERM")
    if status not in (0, -signal.SIGTERM):
        raise SystemExit(f"The app ended with {status}")

    for log in sorted(logs.glob("ugurugu-*.log")):
        print(log.read_text(encoding="utf-8", errors="replace"))
    problems = [path.name for path in logs.glob("crash-*")]
    problems += [
        f"{log.name}: {line}"
        for log in logs.glob("ugurugu-*.log")
        for line in log.read_text(encoding="utf-8", errors="replace").splitlines()
        if " ERROR " in line
    ]
    if problems:
        raise SystemExit("The run left crash records or errors: " + "; ".join(problems))
    print("Unpack and run passed")


if __name__ == "__main__":
    sys.exit(main())
