# Third-party notices

Ugurugu itself is licensed under GPL-3.0-or-later; `LICENSE` holds the GNU
GPL version 3 text that grant refers to. The notices below cover software and
assets that Ugurugu includes or dynamically links, not Ugurugu's own code.
The corresponding license texts are installed next to this file in release
packages.

The app icon artwork in `resources/icons/` is copyright © seuppi and is
distributed under GPL-3.0-or-later with the rest of the project.

Two kinds of package are covered here. The desktop packages for macOS and
Windows contain everything listed below except the web section; the browser
build contains only Qt, Pretendard JP and the components named under
[Web (WebAssembly) build](#web-webassembly-build).

## Qt 6

Copyright © The Qt Company Ltd. and other contributors.

Ugurugu uses Qt under the GNU Lesser General Public License
version 3. Desktop release packages ship Qt as separate dynamic libraries, so
the bundled Qt can be replaced with a modified build of the same version
without rebuilding Ugurugu. LGPLv3 adds permissions on top of
GPLv3; both texts are included, as `LGPL-3.0.txt` and `LICENSE`. Qt source
for the bundled libraries and the full relinking obligations are available
from <https://www.qt.io/licensing/open-source-lgpl-obligations> and
<https://code.qt.io/>.

The browser build links Qt and zlib statically instead; see the web section
below for how the same obligation is met there.

## spdlog 1.16.0

Copyright © 2016 Gabi Melman.

Licensed under the MIT License. The bundled fmt dependency is also licensed
under the MIT License. See `spdlog-LICENSE.txt`.

## libwebp 1.6.0

Copyright © 2010 Google Inc. and other contributors.

Licensed under the BSD 3-Clause License with the additional patent grant
reproduced in `libwebp-PATENTS.txt`. See `libwebp-LICENSE.txt`.

## zlib 1.3.2

Copyright © 1995–2026 Jean-loup Gailly and Mark Adler.

Licensed under the zlib License. See `zlib-LICENSE.txt`.

## Pretendard JP

Copyright © 2021 Kil Hyung-jin.

Licensed under the SIL Open Font License 1.1. See
`Pretendard-OFL.txt`.

## Sparkle 2.9.6 (macOS packages)

Copyright © 2006–2017 the Sparkle contributors.

Licensed under the MIT License and additional licenses reproduced in
`Sparkle-LICENSE.txt`.

## Velopack 1.2.0 (Windows packages)

Copyright © 2021 Caelan Sayler. Copyright © 2024 Velopack Ltd.

Licensed under the MIT License. See `Velopack-LICENSE.txt`.

## Web (WebAssembly) build

The browser build is a different package from the desktop ones. Its engine is
a single `.wasm` binary with Qt 6 Core, Gui and zlib linked **statically**;
spdlog, libwebp, Sparkle and Velopack are not part of it. The shell around it
adds the components below. License texts travel with the upload under
`licenses/`, and the in-app Notices panel points at them.

Because Qt is static here, the relinking route the desktop packages satisfy by
shipping replaceable dynamic libraries is met instead by publishing the
complete corresponding source. Ugurugu is GPL-3.0-or-later and its whole
source — engine, bridge and web shell — is at
<https://github.com/nyabi-gh/Ugurugu>. `BUILDING.md` names the exact toolchain
the binary was produced with (Qt 6.11.1, Emscripten 4.0.7) and the
`wasm-release` preset that rebuilds the engine, so a recipient can relink it
against a modified Qt of the same version.

### Svelte 5.56.8

Copyright © 2016–2025 Svelte Contributors.

Licensed under the MIT License. See `Svelte-LICENSE.txt`.

### Emscripten 4.0.7

Copyright © 2010–2014 Emscripten authors, see the project's AUTHORS file.

Available under both the MIT License and the University of Illinois/NCSA Open
Source License. See `Emscripten-LICENSE.txt`.

### OpenType.js (web text tool)

OpenType.js 2.0.0 is used to read glyph outlines from the bundled Pretendard JP
font. Copyright Frederik De Bleser and contributors; MIT License. The complete
notice is in `resources/licenses/OpenType-LICENSE.txt` and is included in the
web package. Source: <https://github.com/opentypejs/opentype.js>.
