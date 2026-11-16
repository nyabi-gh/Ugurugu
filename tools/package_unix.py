# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

"""Stages a macOS or Linux 3.x build with its licenses and packs it with
Velopack, as tools/package_windows.ps1 does for Windows.

    python3 tools/package_unix.py macos target/release/ugurugu artifacts
    python3 tools/package_unix.py linux target/release/ugurugu artifacts

The package ID is Ugurugu3 on every OS; the channel is the runtime, so each
OS and architecture has its own list in the one update feed. macOS is
signed ad hoc (docs/rust/decisions.md section 10): Velopack adds its updater
to the bundle and signs it all again, so the signature covers what ships.
`--version` and `--release-notes` make a later version of the same build,
for trying an update from a local feed (`UGURUGU_UPDATE_FEED`).
"""

import argparse
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile

import macos_app

ROOT = pathlib.Path(__file__).resolve().parent.parent
TITLE = "Ugurugu 3"
PACK_ID = "Ugurugu3"
AUTHORS = "nyabi-gh"
TARGETS = {
    "macos": {"osx-arm64": "aarch64-apple-darwin"},
    "linux": {
        "linux-x64": "x86_64-unknown-linux-gnu",
        # For trying the package in an arm64 container; not released.
        "linux-arm64": "aarch64-unknown-linux-gnu",
    },
}
# The names on the release page; Velopack names them by package ID alone.
SHOWN = {
    "osx-arm64": "macOS-arm64",
    "linux-x64": "Linux-x64",
    "linux-arm64": "Linux-arm64",
}


def run(*command: str | pathlib.Path) -> None:
    subprocess.run([str(part) for part in command], check=True)


def notices(target: str, output: pathlib.Path) -> None:
    run(sys.executable, "-X", "utf8", ROOT / "tools" / "third_party_notices.py", target, output)


def expect(found: list[str], expected: list[str], what: str) -> None:
    if sorted(found) != sorted(expected):
        raise SystemExit(f"{what} differ from the expected list: {', '.join(sorted(found))}")


def files_under(folder: pathlib.Path) -> list[str]:
    return [path.relative_to(folder).as_posix() for path in folder.rglob("*") if path.is_file()]


def stage_macos(executable: pathlib.Path, staging: pathlib.Path, version: str, target: str) -> pathlib.Path:
    notices(target, staging / "THIRD_PARTY_NOTICES.txt")
    app = macos_app.make(
        executable,
        staging,
        version,
        {
            "LICENSE.txt": ROOT / "LICENSE",
            "THIRD_PARTY_NOTICES.txt": staging / "THIRD_PARTY_NOTICES.txt",
        },
    )
    expect(
        files_under(app),
        [
            "Contents/Info.plist",
            "Contents/MacOS/ugurugu",
            "Contents/Resources/LICENSE.txt",
            "Contents/Resources/THIRD_PARTY_NOTICES.txt",
            f"Contents/Resources/{macos_app.ICON}",
            "Contents/_CodeSignature/CodeResources",
        ],
        "Bundle files",
    )
    return app


def stage_linux(executable: pathlib.Path, staging: pathlib.Path, target: str) -> pathlib.Path:
    folder = staging / "app"
    folder.mkdir()
    shutil.copy2(executable, folder / "ugurugu")
    shutil.copy2(ROOT / "LICENSE", folder / "LICENSE.txt")
    notices(target, folder / "THIRD_PARTY_NOTICES.txt")
    expect(files_under(folder), ["LICENSE.txt", "THIRD_PARTY_NOTICES.txt", "ugurugu"], "Staged files")
    return folder


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("system", choices=TARGETS)
    parser.add_argument("executable", type=pathlib.Path)
    parser.add_argument("output", type=pathlib.Path)
    parser.add_argument("--runtime", help="Velopack runtime; the first of the system's by default")
    parser.add_argument("--vpk", default="vpk")
    parser.add_argument("--version")
    parser.add_argument("--release-notes", type=pathlib.Path)
    args = parser.parse_args()

    runtimes = TARGETS[args.system]
    runtime = args.runtime or next(iter(runtimes))
    if runtime not in runtimes:
        raise SystemExit(f"{args.system} runtimes: {', '.join(runtimes)}")
    version = args.version or macos_app.version()
    release_notes = args.release_notes or ROOT / "release-notes" / f"{version}.en.md"
    if not release_notes.is_file():
        raise SystemExit(f"Missing release notes: {release_notes}")

    args.output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="ugurugu-package-") as folder:
        staging = pathlib.Path(folder)
        # vpk packs for the OS it runs on unless a directive names another.
        directive = ["[linux]"] if args.system == "linux" and sys.platform != "linux" else []
        pack = [
            args.vpk,
            *directive,
            "pack",
            "--packId", PACK_ID,
            "--packVersion", version,
            "--packTitle", TITLE,
            "--packAuthors", AUTHORS,
            "--mainExe", "ugurugu",
            "--releaseNotes", release_notes,
            "--runtime", runtime,
            "--channel", runtime,
            "--delta", "None",
            "--outputDir", args.output,
        ]
        if args.system == "macos":
            app = stage_macos(args.executable, staging, version, runtimes[runtime])
            pack += [
                "--packDir", app,
                # Only the bundle is released: a downloaded `.pkg` that is
                # not signed asks for the same Open Anyway and then for an
                # administrator, and installs the app as root, where the
                # updater cannot replace it (hypothesis).
                "--noInst",
                "--signAppIdentity", "-",
                "--signEntitlements", ROOT / "resources" / "macos" / "Ugurugu.entitlements",
            ]
        else:
            app = stage_linux(args.executable, staging, runtimes[runtime])
            pack += [
                "--packDir", app,
                "--icon", ROOT / "resources" / "icons" / "Ugurugu.png",
                "--categories", "Graphics;2DGraphics;",
            ]
        run(*pack)

    shown = SHOWN[runtime]
    if args.system == "macos":
        installer = args.output / f"{PACK_ID}-{runtime}-Portable.zip"
        renamed = args.output / f"Ugurugu-{version}-{shown}.zip"
    else:
        installer = args.output / f"{PACK_ID}-{runtime}.AppImage"
        renamed = args.output / f"Ugurugu-{version}-{shown}.AppImage"
    os.replace(installer, renamed)
    print(renamed)


if __name__ == "__main__":
    main()
