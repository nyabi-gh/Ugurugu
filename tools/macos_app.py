# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

"""Makes Ugurugu 3.app from a macOS build, so that it runs as Finder and the
Dock run it: with its icon, and as the app for `.ugurugu` files.

    python3 tools/macos_app.py target/release/ugurugu target/release
"""

import json
import pathlib
import plistlib
import re
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
NAME = "Ugurugu 3"
EXECUTABLE = "ugurugu"
ICON = "Ugurugu.icns"
# 2.2.13 is dev.ugurugu.app and Ugurugu.app; 3.x installs beside it, as it
# does on Windows.
IDENTIFIER = "dev.ugurugu.app3"
DOCUMENT_TYPE = "dev.ugurugu.document"
DOCUMENT_NAME = "Ugurugu document"
# The languages of the app's own text, so that AppKit's panels follow them.
LOCALIZATIONS = ["en", "ko", "ja", "zh-Hans"]


def version() -> str:
    metadata = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
            cwd=ROOT,
            check=True,
            stdout=subprocess.PIPE,
            encoding="utf-8",
        ).stdout
    )
    versions = [
        package["version"]
        for package in metadata["packages"]
        if package["name"] == "ugurugu"
    ]
    if len(versions) != 1:
        raise SystemExit(f"expected one ugurugu package, found {len(versions)}")
    return versions[0]


def oldest_macos() -> str:
    config = (ROOT / ".cargo" / "config.toml").read_text(encoding="utf-8")
    found = re.search(r'^MACOSX_DEPLOYMENT_TARGET = "([^"]+)"', config, re.MULTILINE)
    if found is None:
        raise SystemExit("no MACOSX_DEPLOYMENT_TARGET in .cargo/config.toml")
    return found.group(1)


def info(app_version: str) -> dict:
    return {
        "CFBundleDevelopmentRegion": "en",
        "CFBundleDisplayName": NAME,
        "CFBundleDocumentTypes": [
            {
                "CFBundleTypeName": DOCUMENT_NAME,
                "CFBundleTypeRole": "Editor",
                "LSHandlerRank": "Owner",
                "LSItemContentTypes": [DOCUMENT_TYPE],
            }
        ],
        "CFBundleExecutable": EXECUTABLE,
        "CFBundleIconFile": ICON,
        "CFBundleIdentifier": IDENTIFIER,
        "CFBundleInfoDictionaryVersion": "6.0",
        "CFBundleLocalizations": LOCALIZATIONS,
        "CFBundleName": NAME,
        "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": app_version,
        "CFBundleVersion": app_version,
        "LSApplicationCategoryType": "public.app-category.graphics-design",
        "LSMinimumSystemVersion": oldest_macos(),
        "NSHighResolutionCapable": True,
        "NSHumanReadableCopyright": (
            "Copyright © 2026 Nyabi (nyabi-gh). Licensed under GPL-3.0-or-later."
        ),
        "UTExportedTypeDeclarations": [
            {
                "UTTypeConformsTo": ["public.data", "public.content"],
                "UTTypeDescription": DOCUMENT_NAME,
                "UTTypeIdentifier": DOCUMENT_TYPE,
                "UTTypeTagSpecification": {"public.filename-extension": ["ugurugu"]},
            }
        ],
    }


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    executable = pathlib.Path(sys.argv[1])
    app = pathlib.Path(sys.argv[2]) / f"{NAME}.app"

    # Not written over: macOS kills a program whose signed file changed in
    # place since the kernel first saw it.
    if app.exists():
        shutil.rmtree(app)
    contents = app / "Contents"
    (contents / "MacOS").mkdir(parents=True)
    (contents / "Resources").mkdir()
    shutil.copy2(executable, contents / "MacOS" / EXECUTABLE)
    shutil.copy2(ROOT / "resources" / "icons" / ICON, contents / "Resources" / ICON)
    with open(contents / "Info.plist", "wb") as plist:
        plistlib.dump(info(version()), plist)
    # The linker signed the executable alone; this also seals Info.plist and
    # the icon, under the bundle's identifier.
    subprocess.run(["codesign", "--force", "--sign", "-", str(app)], check=True)
    print(app)


if __name__ == "__main__":
    main()
