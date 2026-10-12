# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

"""Fails when the 3.0 app links native code nobody approved.

Native code comes in through crates that link a system library (`links`)
or build C, C++ or assembly in their build scripts. Each one shipped is
listed here with the reason it is accepted (docs/rust/decisions.md M6-6).

    python tools/native_deps.py
"""

import json
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parent.parent
BUILDERS = {"cc", "cmake", "bindgen", "nasm-rs", "pkg-config", "vcpkg"}
APPROVED = {
    # libwebp 1.6.0 from source, for animated WebP export (scope.md).
    "libwebp-sys",
    # TLS for the update check through Velopack (decisions.md M6-5).
    "ring",
    # Sets `links` only so that a build has one copy; no native code.
    "rayon-core",
}


def main() -> None:
    metadata = json.loads(
        subprocess.run(
            [
                "cargo",
                "metadata",
                "--locked",
                "--format-version",
                "1",
                "--filter-platform",
                "x86_64-pc-windows-msvc",
            ],
            cwd=ROOT,
            check=True,
            stdout=subprocess.PIPE,
            encoding="utf-8",
        ).stdout
    )
    packages = {package["id"]: package for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    (app,) = [id for id, package in packages.items() if package["name"] == "ugurugu"]

    # What the executable links: normal dependencies, not build or dev ones.
    shipped, pending = set(), [app]
    while pending:
        id = pending.pop()
        if id in shipped:
            continue
        shipped.add(id)
        pending.extend(
            dependency["pkg"]
            for dependency in nodes[id]["deps"]
            if any(kind["kind"] is None for kind in dependency["dep_kinds"])
        )

    native = set()
    for id in shipped:
        package = packages[id]
        builders = {
            dependency["name"]
            for dependency in package["dependencies"]
            if dependency["kind"] == "build"
        }
        if package.get("links") or builders & BUILDERS:
            native.add(package["name"])

    print(f"{len(shipped)} crates shipped; native: {', '.join(sorted(native))}")
    if native - APPROVED:
        raise SystemExit(f"not approved: {', '.join(sorted(native - APPROVED))}")
    if APPROVED - native:
        raise SystemExit(f"approved but no longer shipped: {', '.join(sorted(APPROVED - native))}")


if __name__ == "__main__":
    main()
