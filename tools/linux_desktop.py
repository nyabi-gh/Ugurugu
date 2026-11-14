# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 NewfolderGames

"""Installs or removes Ugurugu 3's XDG desktop entry, icon, and `.ugurugu` MIME
registration under a user data directory (`$XDG_DATA_HOME` or `~/.local/share`).

    python3 tools/linux_desktop.py install target/release/ugurugu
    python3 tools/linux_desktop.py uninstall
"""

import argparse
import os
import pathlib
import shutil
import subprocess

ROOT = pathlib.Path(__file__).resolve().parent.parent
APP_ID = "dev.ugurugu.app3"
MIME_ICON = "application-x-ugurugu"


def default_data_home() -> pathlib.Path:
    xdg = os.environ.get("XDG_DATA_HOME", "")
    if xdg and os.path.isabs(xdg):
        return pathlib.Path(xdg)
    return pathlib.Path.home() / ".local" / "share"


def desktop_exec_quote(executable: pathlib.Path) -> str:
    # FreeDesktop Exec value quoting rules: wrap in double quotes and escape
    # `\`, `"`, `` ` ``, and `$`, then double `%` so field codes are preserved.
    raw = str(executable.resolve()).replace("%", "%%")
    for ch in ("\\", '"', "`", "$"):
        raw = raw.replace(ch, "\\" + ch)
    return f'"{raw}" %f'


def refresh_caches(data_home: pathlib.Path) -> None:
    mime_dir = data_home / "mime"
    apps_dir = data_home / "applications"
    icons_dir = data_home / "icons" / "hicolor"

    if shutil.which("update-mime-database") and mime_dir.is_dir():
        subprocess.run(
            ["update-mime-database", str(mime_dir)],
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    if shutil.which("update-desktop-database") and apps_dir.is_dir():
        subprocess.run(
            ["update-desktop-database", str(apps_dir)],
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    if shutil.which("gtk-update-icon-cache") and icons_dir.is_dir():
        subprocess.run(
            ["gtk-update-icon-cache", "-f", "-t", str(icons_dir)],
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )


def install(executable: pathlib.Path, data_home: pathlib.Path) -> None:
    if not executable.is_file():
        raise SystemExit(f"executable not found: {executable}")

    apps_dir = data_home / "applications"
    mime_pkg_dir = data_home / "mime" / "packages"
    app_icon_dir = data_home / "icons" / "hicolor" / "512x512" / "apps"
    mime_icon_dir = data_home / "icons" / "hicolor" / "512x512" / "mimetypes"

    for directory in (apps_dir, mime_pkg_dir, app_icon_dir, mime_icon_dir):
        directory.mkdir(parents=True, exist_ok=True)

    template = (ROOT / "resources" / "linux" / f"{APP_ID}.desktop").read_text(
        encoding="utf-8"
    )
    exec_line = f"Exec={desktop_exec_quote(executable)}"
    rendered_lines = [
        exec_line if line.startswith("Exec=") else line
        for line in template.splitlines()
    ]
    desktop_path = apps_dir / f"{APP_ID}.desktop"
    desktop_path.write_text("\n".join(rendered_lines) + "\n", encoding="utf-8")

    shutil.copy2(
        ROOT / "resources" / "linux" / f"{APP_ID}.xml",
        mime_pkg_dir / f"{APP_ID}.xml",
    )
    icon_src = ROOT / "resources" / "icons" / "Ugurugu.png"
    shutil.copy2(icon_src, app_icon_dir / f"{APP_ID}.png")
    shutil.copy2(icon_src, mime_icon_dir / f"{MIME_ICON}.png")

    refresh_caches(data_home)
    print(desktop_path)


def uninstall(data_home: pathlib.Path) -> None:
    targets = [
        data_home / "applications" / f"{APP_ID}.desktop",
        data_home / "mime" / "packages" / f"{APP_ID}.xml",
        data_home / "icons" / "hicolor" / "512x512" / "apps" / f"{APP_ID}.png",
        data_home / "icons" / "hicolor" / "512x512" / "mimetypes" / f"{MIME_ICON}.png",
    ]
    for path in targets:
        if path.exists():
            path.unlink()
    refresh_caches(data_home)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Install or uninstall Ugurugu 3 Linux desktop integration."
    )
    sub = parser.add_subparsers(dest="command", required=True)

    install_cmd = sub.add_parser("install", help="Install .desktop, icon, and MIME XML")
    install_cmd.add_argument("executable", type=pathlib.Path)
    install_cmd.add_argument(
        "--data-home",
        type=pathlib.Path,
        default=default_data_home(),
    )

    uninstall_cmd = sub.add_parser("uninstall", help="Remove installed desktop files")
    uninstall_cmd.add_argument(
        "--data-home",
        type=pathlib.Path,
        default=default_data_home(),
    )

    args = parser.parse_args()
    if args.command == "install":
        install(args.executable, args.data_home)
    else:
        uninstall(args.data_home)


if __name__ == "__main__":
    main()
