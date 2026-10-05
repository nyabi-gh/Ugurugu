<p align="center">
  <img src="resources/icons/Ugurugu.png" width="112" alt="Ugurugu app icon">
</p>

<h1 align="center">Ugurugu</h1>

<p align="center">
  A drawing app where your pictures wiggle and move.
</p>

<p align="center">
  <a href="https://github.com/nyabi-gh/Ugurugu/releases/latest"><img src="https://img.shields.io/github/v/release/nyabi-gh/Ugurugu?style=flat-square&color=ffc94a" alt="Latest release"></a>
  <a href="https://github.com/nyabi-gh/Ugurugu/releases"><img src="https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2Fnyabi-gh%2FUgurugu%2Fdownload-badge%2Fdownloads.json&style=flat-square" alt="Downloads"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-ffc94a?style=flat-square" alt="License"></a>
</p>

<p align="center"><a href="README.md">KR</a> · <b>EN</b> · <a href="README.ja.md">JP</a></p>

Save your work as a looping GIF or WebP or as an image with a transparent
background, and continue `.wawa` drawings made with WiggleWiggleTool.

## Download

| Platform | Requirements | Download |
| --- | --- | --- |
| Windows | Windows 10 or later, 64-bit | [Setup.exe](https://github.com/nyabi-gh/Ugurugu/releases/latest/download/Ugurugu-Windows-x64-Setup.exe) |
| macOS | macOS 14 or later, Apple Silicon | [DMG](https://github.com/nyabi-gh/Ugurugu/releases/latest/download/Ugurugu-macOS-arm64.dmg) |

- **Windows**: Run the Setup file. If Windows shows an unrecognized-app
  warning, choose **More info → Run anyway**.
- **macOS**: Open the DMG and drag Ugurugu into the Applications folder. The
  app is checked by Apple before release.

Only use files from the official
[Releases page](https://github.com/nyabi-gh/Ugurugu/releases/latest). The other
files there are for automatic updates, so you don't need them.

<details>
<summary>System requirements</summary>

| | Minimum | Recommended |
| --- | --- | --- |
| Operating system | Windows 10 64-bit / macOS 14 (Apple Silicon) | Windows 11 / latest macOS |
| Memory | 8GB | 16GB or more |
| Graphics | No specific requirement | A GPU with Direct3D 11 (Windows) or Metal (macOS) support |
| Input device | Mouse | A pen tablet with pressure support (Wacom and others) |

Canvas display, zooming, panning and playback run on the GPU, and the app
switches to software rendering automatically when graphics acceleration is
unavailable. On large canvases (up to 4096×4096) with several layers, more
memory keeps the preview smooth.

</details>

## Get started

1. Create a canvas or open an existing drawing.
2. Choose a brush on the left and draw a line.
3. Pick a movement in the **Wobble** panel, then press `P` to play it.
4. Export a GIF, WebP, PNG or JPG from the **File** menu.

If you get stuck, press `F1` for the built-in help.

## Features

- **Wobble** — Classic, Smooth and Stepped motion styles; adjustable wobble,
  detail, linking and randomness; broken-line effects. Turn wobble on or off
  per layer so a background holds still while the lines above it move.
- **Drawing** — 17 ready-made tools including pressure-sensitive brushes and
  erasers, line stabilization, paint bucket, freehand/rectangle/oval
  selection and transforms, and two-finger pan, zoom and rotate on
  multitouch displays.
- **Layers** — Groups, opacity, blend modes, image import, and canvas crop,
  expand and resize.
- **Save and export** — Projects are saved as `.ugu` (earlier `.wagle` and
  `.wobble` files still open). Export GIF, WebP, PNG and JPG with transparent
  backgrounds, share settings as `.wwpreset`, and recover work after a crash.
- **Customization** — Drag panels to stack, place side by side or join as
  tabs; pick a highlight color; change any shortcut; use the app in English,
  Korean or Japanese; update from inside the app.

Open settings with the gear button on the toolbar (or **Edit → Settings** on
Windows, **Ugurugu → Settings** on macOS). **Window → Reset panel layout**
restores the original layout. The app checks for new versions at startup, and
**Help → Check for Updates** checks on demand.

<details>
<summary>Wobble settings in detail</summary>

Switch the range at the top of the panel to **Active layer** to apply the
settings to the selected layer only.

| Setting | Range (default) | What it does |
| --- | --- | --- |
| Motion style | Classic · Smooth · Stepped (Classic) | **Classic** is the movement from earlier versions, **Smooth** flows from pose to pose, and **Stepped** snaps between poses for a hand-drawn animation feel. |
| Wobble | 0 – 12 px (1.6 px) | How far a line can stray from where you drew it. At 0 nothing moves. |
| Pose count | 1 – frame count (8) | How many distinct drawings the wobble cycles through. Fewer feel choppy, more feel smooth. At 1 it stops. |
| Detail | 1 – 24 (12) | Spacing of the wobble along a line. Low gives broad waves, high gives a fine shiver. |
| Linked | 0 – 100% (100%) | How much the lines move as one. At 100% the whole drawing sways together; at 0% every line moves on its own. |
| Randomness | 0 – 100% (0%) | Higher values turn the movement into point-by-point noise. |
| Broken line | on · off (off) | Parts of a line vanish and reappear. The two settings below need this on. |
| Break amount | 0 – 100% (35%) | How much of the line disappears. At 100% it vanishes completely. |
| Break range | 2 – 256 px (24 px) | Size of the gaps. Small looks dotted, large breaks the line into chunks. |

**Pose count** and **Detail** only apply to *Smooth* and *Stepped*; *Classic*
does not use them.

</details>

## Continue a WiggleWiggleTool drawing

Open a `.wawa` file saved by WiggleWiggleTool 10 with **File → Open**. It opens
as a new project without changing the original, and the first save suggests a
`.ugu` file with the same name. Because the two apps draw differently, some
wobble, airbrush and filled shapes may look slightly different; the app tells
you what was changed or could not be brought across.

## Shortcuts

Common defaults. You can change all of them in **Settings → Shortcuts**.

| Key | Action |
| --- | --- |
| `B` / `E` | Brush / Eraser |
| `L` / `W` / `G` | Area select / Auto select / Paint bucket |
| `I` or `Alt` + click | Pick a color |
| `P` | Play / pause |
| `Space` + drag, scroll | Pan, zoom |
| `Ctrl/Cmd+Z` | Undo |
| `F1` | Help |

<details>
<summary>All shortcuts</summary>

| Key | Action |
| --- | --- |
| Two-finger drag / pinch / twist | Pan / zoom / rotate the canvas (supported multitouch displays) |
| `Shift+Space` + drag | Rotate the canvas freely |
| `Shift` + scroll | Rotate the canvas in 5° steps |
| `-` / `^` | Rotate the canvas left / right by 5° |
| **View → Rotate canvas → Reset canvas rotation** | Reset rotation to 0° |
| `Alt+Delete` | Fill the selection with the brush color |
| `Ctrl+T` | Collapse / show the animation bar |
| `Ctrl/Cmd+C`, `X`, `V` | Copy / Cut / Paste |
| `Ctrl+Y` (Windows), `Cmd+Shift+Z` (macOS) | Redo |
| `Ctrl/Cmd+0` | Fit the canvas to the window |
| `Ctrl/Cmd+1` | View at actual pixel size |
| `Enter` / `Esc` | Apply / cancel a change |

</details>

## Reporting problems

Open a [GitHub Issue](https://github.com/nyabi-gh/Ugurugu/issues) and tell us
what you were doing and what happened. Attaching the `.ugu` file helps a lot.
Report security vulnerabilities through the private channel in the
[security policy](SECURITY.md), not a public issue.

## Development and contributing

See [BUILDING.md](BUILDING.md) to build from source and
[CONTRIBUTING.md](CONTRIBUTING.md) to contribute.

## Credits and license

- Development support by seuppi
- App icon artwork by seuppi (`resources/icons/`, distributed under GPL-3.0-or-later)

Copyright (C) 2026 Nyabi (nyabi-gh)

This program is free software: you can redistribute it and/or modify it under
the terms of the [GNU General Public License](LICENSE), version 3 or (at your
option) any later version. It comes with no warranty. Contributions are
accepted under the same terms, and the terms for the included font and
libraries are in the [third-party notices](THIRD_PARTY_NOTICES.md).

SPDX: `GPL-3.0-or-later`
