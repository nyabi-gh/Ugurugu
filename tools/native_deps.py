# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

"""Fails when the 3.0 app links native code nobody approved, on any OS.

Native code comes in through crates that link a system library (`links`)
or build C, C++ or assembly in their build scripts. Each one shipped is
listed here with the reason it is accepted (docs/rust/decisions.md M6-6).
Every target is checked from whichever OS runs this.

    python tools/native_deps.py
"""

import json
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parent.parent
BUILDERS = {"cc", "cmake", "bindgen", "nasm-rs", "pkg-config", "vcpkg"}
COMMON = {
    # libwebp 1.6.0 from source, for animated WebP export (scope.md).
    "libwebp-sys",
    # TLS for the update check through Velopack (decisions.md M6-5).
    "ring",
    # Sets `links` only so that a build has one copy; no native code.
    "rayon-core",
}
APPROVED = {
    "x86_64-pc-windows-msvc": COMMON,
    "aarch64-apple-darwin": COMMON
    | {
        # Links macOS's own Objective-C runtime, which AppKit needs (winit,
        # AccessKit); its C part is built only with `unstable-exception`.
        "objc-sys",
    },
    "x86_64-unknown-linux-gnu": COMMON
    | {
        # Loads libwayland-client at run time (`dlopen`, which
        # smithay-clipboard turns on); links nothing.
        "wayland-sys",
        # Builds its C log shim only with `log`, which is off.
        "wayland-backend",
        # Asks pkg-config for xkbcommon only with `xkbcommon`, which is off.
        "smithay-client-toolkit",
        # Loads the X11 libraries at run time; links nothing.
        "x11-dl",
        # Links the system fontconfig, through which Parley finds the
        # installed fonts.
        "yeslogic-fontconfig-sys",
    },
}


def main() -> None:
    failed = False
    for target, approved in APPROVED.items():
        failed |= not check(target, approved)
    if failed:
        raise SystemExit(1)


def check(target: str, approved: set[str]) -> bool:
    metadata = json.loads(
        subprocess.run(
            [
                "cargo",
                "metadata",
                "--locked",
                "--format-version",
                "1",
                "--filter-platform",
                target,
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

    print(f"{target}: {len(shipped)} crates shipped; native: {', '.join(sorted(native))}")
    if native - approved:
        print(f"{target}: not approved: {', '.join(sorted(native - approved))}")
    if approved - native:
        print(f"{target}: approved but no longer shipped: {', '.join(sorted(approved - native))}")
    return native == approved


if __name__ == "__main__":
    main()
