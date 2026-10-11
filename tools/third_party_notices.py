# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

"""Writes THIRD_PARTY_NOTICES.txt for the 3.0 Windows package.

The crates the app links come from `cargo about` (about.toml). Code and
assets that no crate license covers are added here: the app icon, the
Pretendard JP font the app embeds and libwebp, whose sources libwebp-sys
builds into the executable under libwebp's own license.

    python -X utf8 tools/third_party_notices.py OUTPUT
"""

import json
import pathlib
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
APP = ROOT / "apps" / "ugurugu" / "Cargo.toml"
RULE = "=" * 78

HEADER = """\
Ugurugu 3 - third-party notices

Ugurugu itself is licensed under GPL-3.0-or-later; LICENSE.txt next to this
file holds the GNU GPL version 3 text that grant refers to. Its source is at
https://github.com/nyabi-gh/Ugurugu. The notices below cover software and
assets that the Windows package includes, not Ugurugu's own code.

The app icon artwork is copyright (c) seuppi and is distributed under
GPL-3.0-or-later with the rest of the project.
"""


def cargo(*arguments: str) -> str:
    return subprocess.run(
        ["cargo", *arguments],
        cwd=ROOT,
        check=True,
        stdout=subprocess.PIPE,
        encoding="utf-8",
    ).stdout


def package_folder(name: str) -> pathlib.Path:
    metadata = json.loads(
        cargo(
            "metadata",
            "--locked",
            "--format-version",
            "1",
            "--manifest-path",
            str(APP),
            "--filter-platform",
            "x86_64-pc-windows-msvc",
        )
    )
    folders = [
        pathlib.Path(package["manifest_path"]).parent
        for package in metadata["packages"]
        if package["name"] == name
    ]
    if len(folders) != 1:
        raise SystemExit(f"expected one {name} package, found {len(folders)}")
    return folders[0]


def section(title: str, body: str) -> str:
    return f"{RULE}\n{title}\n{RULE}\n\n{body.strip()}\n"


def read(path: pathlib.Path) -> str:
    return path.read_text(encoding="utf-8").replace("\r\n", "\n")


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    output = pathlib.Path(sys.argv[1])

    # cargo-about refuses to write to a pipe when started from PowerShell.
    with tempfile.TemporaryDirectory() as folder:
        listing = pathlib.Path(folder) / "about.json"
        cargo(
            "about",
            "generate",
            "--format",
            "json",
            "--locked",
            "--manifest-path",
            str(APP),
            "--output-file",
            str(listing),
        )
        about = json.loads(listing.read_text(encoding="utf-8"))
    webp = package_folder("libwebp-sys") / "vendor"

    parts = [HEADER]
    parts.append(
        section(
            "Pretendard JP",
            "Copyright (c) 2021 Kil Hyung-jin.\n\n"
            "The interface font. Licensed under the SIL Open Font License 1.1:\n\n"
            + read(ROOT / "resources" / "fonts" / "OFL.txt"),
        )
    )
    parts.append(
        section(
            "libwebp",
            "Built from source into the executable through the libwebp-sys "
            "crate, for animated WebP export.\n\n"
            + read(webp / "COPYING")
            + "\n\nAdditional IP rights grant (patents):\n\n"
            + read(webp / "PATENTS"),
        )
    )

    crates = sorted(
        about["crates"],
        key=lambda entry: (entry["package"]["name"], entry["package"]["version"]),
    )
    listing = "\n".join(
        f"{entry['package']['name']} {entry['package']['version']}: {entry['license']}"
        for entry in crates
    )
    parts.append(
        section(
            "Rust crates",
            "The executable links the crates below. Their license texts follow, "
            "each with the crates it applies to.\n\n" + listing,
        )
    )
    for license in about["licenses"]:
        users = ", ".join(
            sorted(
                f"{used['crate']['name']} {used['crate']['version']}"
                for used in license["used_by"]
            )
        )
        parts.append(
            section(
                f"{license['name']} ({license['id']})",
                f"Used by: {users}\n\n{license['text'].replace(chr(13), '')}",
            )
        )

    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text("\n\n".join(parts), encoding="utf-8", newline="\r\n")
    print(f"{output}: {len(crates)} crates, {len(about['licenses'])} license texts")


if __name__ == "__main__":
    main()
