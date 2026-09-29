<p align="center">
  <img src="resources/icons/Ugurugu.png" width="128" alt="Ugurugu app icon">
</p>

# Ugurugu

[![Latest Release](https://img.shields.io/github/v/release/nyabi-gh/Ugurugu?style=for-the-badge&logo=github&logoColor=white&labelColor=1e1b2e&color=ffc94a)](https://github.com/nyabi-gh/Ugurugu/releases/latest)
[![Downloads](https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2Fnyabi-gh%2FUgurugu%2Fdownload-badge%2Fdownloads.json&style=for-the-badge&logo=github&logoColor=white&labelColor=1e1b2e)](https://github.com/nyabi-gh/Ugurugu/releases)
![License](https://img.shields.io/badge/license-GPL--3.0--or--later-ffc94a?style=for-the-badge&logo=gnu&logoColor=white&labelColor=1e1b2e)
![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-ffc94a?style=for-the-badge&logo=qt&logoColor=white&labelColor=1e1b2e)

<p align="center"><a href="README.md">KR</a> · <b>EN</b> · <a href="README.ja.md">JP</a></p>

A drawing app where your pictures wiggle and move.

Save your work as a looping GIF or WebP, or create a still image with a
transparent background. You can also continue working on `.wawa` drawings
made with WiggleWiggleTool.

> [!NOTE]
> If something goes wrong, please open a
> [GitHub Issue](https://github.com/nyabi-gh/Ugurugu/issues) and tell
> us what you were doing and what happened. Attaching the `.ugu` file, when
> possible, makes the problem much easier to find.
>
> Security vulnerabilities are the exception: please use the private channel in
> the [security policy](SECURITY.md) instead of a public issue.

## Download and install

| Platform | Requirements | Download |
| --- | --- | --- |
| Windows | Windows 10 or later, 64-bit | [Windows installer](https://github.com/nyabi-gh/Ugurugu/releases/latest/download/Ugurugu-Windows-x64-Setup.exe) |
| macOS | macOS 14 or later, Apple Silicon | [macOS installer](https://github.com/nyabi-gh/Ugurugu/releases/latest/download/Ugurugu-macOS-arm64.dmg) |

### Windows

Run the downloaded Setup file. If Windows shows an unrecognized-app warning,
choose **More info → Run anyway**. For your safety, only use files from the
official [Releases page](https://github.com/nyabi-gh/Ugurugu/releases/latest).

### macOS

Open the DMG and drag Ugurugu into the Applications folder. The macOS
app is checked by Apple before release.

The other files on the release page are used by automatic updates. For a
normal installation, you only need the installer in the table above.

## System requirements

| | Minimum | Recommended |
| --- | --- | --- |
| Operating system | Windows 10 64-bit / macOS 14 (Apple Silicon) | Windows 11 / latest macOS |
| Memory | 8GB | 16GB or more |
| Graphics | No specific requirement | A GPU with Direct3D 11 (Windows) or Metal (macOS) support |
| Input device | Mouse | A pen tablet with pressure support (Wacom and others) |

Canvas display, zooming, panning and playback are GPU accelerated, and
the app automatically switches to software rendering when graphics
acceleration is unavailable. When working on large canvases (up to
4096×4096) with several layers, more memory keeps the preview smooth.

## Get started

1. Open the app and create a canvas, or open an existing drawing.
2. Choose a brush from the left side and draw a line.
3. Pick a movement in the **Wobble** panel, then press `P` to play it.
4. Use the **File** menu to export a moving GIF or WebP, or a PNG or JPG image.

Press `F1` at any time to open the built-in help.

## What can you do?

### Make lines move your way

- Choose smooth movement, a stop-motion-like stepped movement, or the familiar
  classic movement.
- Adjust how far lines move, how detailed the movement feels, how closely the
  lines move together, and how much surprise is added.
- Create lines with playful gaps, or let erased areas move with the drawing.
- Turn movement off whenever you want to use the app like a regular paint app.

#### Wobble settings in detail

The **Wobble** panel holds these settings. Switch the range at the top of
the panel to **Active layer** to apply them to the selected layer only.

| Setting | Range (default) | What it does |
| --- | --- | --- |
| Motion style | Classic · Smooth · Stepped (Classic) | How the movement is built. **Classic** is the familiar movement from earlier versions, **Smooth** flows from one pose into the next, and **Stepped** snaps between poses for a hand-drawn animation feel. |
| Wobble | 0 – 12 px (1.6 px) | How far a line can stray from where you drew it. At 0 nothing moves. |
| Pose count | 1 – frame count (8) | How many **distinct drawings** the wobble cycles through. With 30 frames and 8 poses it is like drawing 8 pictures and looping them. Fewer poses feel choppy and hand-drawn, more feel smooth. **At 1 the animation stops entirely.** |
| Detail | 1 – 24 (12) | The **spacing** of the wobble along a line. Low values give broad, gentle waves; high values give a fine shiver. |
| Linked | 0 – 100% (100%) | How much the lines **move as one**. At 100% the whole drawing sways together like a single sheet; at 0% every line does its own thing. |
| Randomness | 0 – 100% (0%) | At 0% the movement flows smoothly. The higher you go, the more it becomes point-by-point noise. |
| Broken line | on · off (off) | Lets parts of a line vanish and reappear. The two settings below only apply when this is on. |
| Break amount | 0 – 100% (35%) | **How much** of the line disappears. At 0% nothing breaks; at 100% the line vanishes completely. |
| Break range | 2 – 256 px (24 px) | The **size of the gaps**. Small values look like a dotted line, large values break the line into big chunks. |

> [!TIP]
> **Pose count** and **Detail** only apply when the motion style is *Smooth* or
> *Stepped*. *Classic* does not use them.

### Draw and color comfortably

- Use pressure-sensitive brushes and erasers, with 17 ready-made pens,
  markers, airbrushes, and sprays.
- Smooth out slow or long strokes with adjustable line stabilization.
- On supported multitouch displays, use two fingers to pan the canvas, pinch
  to zoom, and twist to rotate.
- Tell the paint bucket how similar nearby colors should be and whether it
  should look at the current layer or other layers too.
- Select freehand, rectangular, or oval areas, then move, resize, rotate,
  flip, copy, and paste them.
- Switch the area tool from selecting to painting when you want to fill the
  shape you draw immediately.

### Work with layers and images

- Stack layers, organize them into groups, and change their transparency or
  how their colors mix. The list shows each layer's opacity and blend mode
  at a glance.
- Switch wobble off one layer at a time, so a background can hold still
  while the lines above it keep moving.
- Merge with the layer below when the result can stay exactly as it looks.
- Place a photo or another drawing on a new layer, then move, resize, or rotate
  it. Repeated resizing always starts from the original image.
- Crop or expand the canvas, or resize the whole drawing.

### Save and share

- Save projects as `.ugu` files with layers and movement settings intact.
- Projects saved by earlier versions as `.wagle` or `.wobble` still open.
- Export moving GIF and WebP files, or still PNG and JPG images.
- Use a transparent canvas to make sticker-like pictures and animations with
  no background.
- Save your favorite brush, fill, and movement setup as a `.wwpreset` file and
  load it on another computer.
- Recover your work after an unexpected shutdown.

### Make the app yours

- Drag panels wherever you want them. Drop one **above or below** another to
  stack them, or on its **left or right edge** to put them side by side in
  two columns. Drop one on a title bar to join them as tabs.
- One button folds the whole right or left panel away for more drawing room.
  Your arrangement and folded state come back the next time you open the app.
- **Window → Reset panel layout** puts everything back where it started.
- Pick the highlight color used around the app.
- Change any shortcut and restore the defaults whenever you want.
- Use the app in English, Korean, or Japanese.
- Check for new versions and update from inside the app.

## Continue a WiggleWiggleTool drawing

Open a `.wawa` file saved by WiggleWiggleTool 10 with **File → Open**. It opens
as a new project without changing the original. The first time you save it,
Ugurugu suggests a `.ugu` file with the same name.

Because the two apps draw in different ways, some wobble, airbrush, and filled
shapes may look a little different. After opening the file, the app tells you
what was changed or could not be brought across.

## Handy shortcuts

These are the defaults. You can change all of them in **Settings → Shortcuts**.

| Key | Action |
| --- | --- |
| `B` / `E` | Brush / Eraser |
| `L` / `W` / `G` | Area select / Auto select / Paint bucket |
| `I` | Eyedropper |
| `P` | Play or pause movement |
| Two-finger drag / pinch / twist | Pan / zoom / rotate the canvas (supported multitouch displays) |
| `Space` + drag | Move around the canvas |
| `Shift+Space` + mouse/pen drag | Rotate the canvas freely |
| `Shift` + scroll | Rotate the canvas in 5° steps |
| `-` / `^` | Rotate the canvas left / right by 5° |
| **View → Rotate canvas → Reset canvas rotation** | Reset the canvas rotation to 0° |
| Scroll | Zoom in or out |
| `Alt` + click | Pick a color from the drawing |
| `Alt+Delete` | Fill the selected area with the brush color |
| `Ctrl+T` | Collapse or show the animation bar |
| `Ctrl/Cmd+C`, `X`, `V` | Copy / Cut / Paste |
| `Ctrl+Z` / `Ctrl+Y` | Undo / Redo on Windows |
| `Cmd+Z` / `Cmd+Shift+Z` | Undo / Redo on macOS |
| `Ctrl/Cmd+0` | Fit the canvas to the window |
| `Ctrl/Cmd+1` | View at actual pixel size |
| `Enter` / `Esc` | Apply / Cancel a selection change |
| `F1` | Open the built-in help |

## Settings and updates

Use the gear button to change the language, app color, drawing behavior,
default save folder, and shortcuts. Settings are also under **Edit → Settings**
on Windows and **Ugurugu → Settings** on macOS.

The app checks for a new version when it starts. You can check at any time with
**Help → Check for Updates**.

## For developers

See [BUILDING.md](BUILDING.md) if you want to build the app from source.

## Credits

- Development support by seuppi
- App icon artwork by seuppi

The app icon artwork in `resources/icons/` is copyright seuppi and is
distributed under GPL-3.0-or-later along with the rest of the project.

## License

Copyright (C) 2026 Nyabi (nyabi-gh)

This program is free software: you can redistribute it and/or modify it
under the terms of the [GNU General Public License](LICENSE) as published by
the Free Software Foundation, either version 3 of the License, or (at your
option) any later version.

This program is distributed in the hope that it will be useful, but WITHOUT
ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
FOR A PARTICULAR PURPOSE. See the GNU General Public License for more
details. You should have received a copy of the GNU General Public License
along with this program. If not, see <https://www.gnu.org/licenses/>.

SPDX identifier: `GPL-3.0-or-later`

Contributions are accepted under the same terms; see the
[contributing guide](CONTRIBUTING.md). Copyright and license details for the
included font and libraries are in the
[third-party notices](THIRD_PARTY_NOTICES.md).
