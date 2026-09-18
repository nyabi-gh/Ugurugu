# Building Ugurugu

## Requirements

- CMake 3.31 or later, and 4.2 or later on Windows for the
  Visual Studio 18 2026 generator
- Qt 6.10 or later, including the Qt Shader Tools module; releases are
  built against Qt 6.11.1
- An internet connection while configuring so CMake can fetch the
  pinned zlib, spdlog, libwebp, Sparkle, or Velopack dependencies

## macOS

The macOS presets use Ninja. Install the dependencies with Homebrew:

```sh
brew install cmake ninja qt
```

Configure, build, and create a deployable app:

```sh
cmake --preset macos-release
cmake --build --preset macos-release
cmake --install out/build/macos-release --prefix out/install/macos-release
open out/install/macos-release/Ugurugu.app
```

The supported deployment target is macOS 14 or later. Use the official
Qt 6.11.1 binaries when creating a distributable build; Homebrew Qt may
have a newer deployment target. Homebrew Qt ships Qt Shader Tools; the
Qt online installer lists it under Additional Libraries and does not
select it by default.

## Windows

Install Visual Studio 2026 with the Desktop development with C++
workload and its C++ Clang tools for Windows component, plus
Qt 6.11.1 for MSVC 2022 x64 with the Qt Shader Tools additional
library.

The Windows presets use the Visual Studio 18 2026 generator with the
ClangCL toolset, so the build uses clang-cl against the MSVC ABI. The
Qt MSVC 2022 kit is the right one to install: MSVC v145 keeps binary
compatibility with v143, and the Velopack updater ships an MSVC import
library.

Configure, build, and create a deployable application directory:

```powershell
cmake --preset windows-release "-DCMAKE_PREFIX_PATH=C:\Qt\6.11.1\msvc2022_64"
cmake --build --preset windows-release
cmake --install out/build/windows-release --config Release --prefix out/install/windows-release
.\out\install\windows-release\Ugurugu.exe
```

## WebAssembly engine (experimental)

The `wasm-release` preset cross-compiles the engine subset — document,
brush, input, render, serializer, no Widgets UI — to single-threaded
WebAssembly with a small C ABI bridge. It expects:

- Qt 6.11.1 `wasm_singlethread` and `macos` kits under `~/Qt/6.11.1`,
  for example via `aqt install-qt all_os wasm 6.11.1 wasm_singlethread`
  and `aqt install-qt mac desktop 6.11.1 clang_64 --archives qtbase`
- Emscripten 4.0.7, the version Qt 6.11.1 pins, activated in `~/emsdk`

```sh
cmake --preset wasm-release
cmake --build --preset wasm-release
node tools/wasm_engine_smoke.mjs
node tools/wasm_content_smoke.mjs
```

The preset is macOS-only. On Windows, configure the same build
directory by hand with `EMSDK` in the environment:

```sh
cmake -S . -B out/build/wasm-release -G Ninja \
  -DCMAKE_TOOLCHAIN_FILE=C:/Qt/6.11.1/wasm_singlethread/lib/cmake/Qt6/qt.toolchain.cmake \
  -DQT_HOST_PATH=C:/Qt/6.11.1/mingw_64 \
  -DCMAKE_BUILD_TYPE=Release -DBUILD_TESTING=OFF
cmake --build out/build/wasm-release --parallel
```

The smoke script loads `examples/Wave.ugu`, renders three frames, and
round-trips the serializer in Node. `tools/wasm_worker_harness/` runs
the same check inside a browser Dedicated Worker: serve `index.html`,
`engine-worker.js`, the two `ugurugu_engine_spike.*` build outputs, and
`Wave.ugu` from one directory over HTTP. The native counterpart for
comparing digests is the `ugurugu_engine_digest_probe` tool target.

## Tests

Use the debug preset for the current platform:

```sh
cmake --preset macos-debug
cmake --build --preset macos-debug
ctest --preset macos-debug
```

On Windows, replace `macos-debug` with `windows-debug`.

## Fuzzing

The `macos-fuzzing` preset instruments the build for libFuzzer and adds
AddressSanitizer and UndefinedBehaviorSanitizer, then builds one entry
point per parser that reads untrusted bytes: the legacy `.wagle`
importer, the project JSON serializer, the selection clipboard codec,
and the WWP preset codec.

Xcode's clang ships no libFuzzer runtime, and older Apple linkers reject
current LLVM's sanitizer relocations, so the build needs Homebrew LLVM and
LLD:

```sh
brew install llvm@22 lld@22
export PATH="$(brew --prefix lld@22)/bin:$PATH"
cmake --preset macos-fuzzing \
    -DCMAKE_C_COMPILER="$(brew --prefix llvm@22)/bin/clang" \
    -DCMAKE_CXX_COMPILER="$(brew --prefix llvm@22)/bin/clang++" \
    -DCMAKE_LINKER_TYPE=LLD
cmake --build --preset macos-fuzzing
out/build/macos-fuzzing/ugurugu_fuzz_document_json \
    out/build/macos-fuzzing/fuzz-corpus/ugurugu_fuzz_document_json \
    -max_total_time=90
```

Each target's corpus directory is seeded from the repository fixtures at
build time and libFuzzer writes newly discovered inputs back into it.
There is no Windows equivalent: clang-cl compiles the instrumentation,
but LLVM's libFuzzer runtime wants the static CRT and the prebuilt Qt
requires the dynamic one.
Leak detection is left off in CI because Qt's process-lifetime caches
are indistinguishable from leaks; run with `ASAN_OPTIONS=detect_leaks=1`
when a leak is what you are looking for.

## Web shell

The Svelte shell in `web/` wraps the WebAssembly engine.

```sh
cd web
npm ci
npm run check
npm run build
```

`npm run check` runs `svelte-check` over the TypeScript and the
components. `npm run build` copies the `wasm-release` outputs into
`public/engine` when they exist, and warns instead of failing when they
do not, so the shell can be type-checked and bundled without a wasm
build. It also copies the licence texts the in-app notices panel links
to from `resources/licenses/` and the repository root into
`public/licenses/`; those come from the repository rather than from a
toolchain directory so the web package carries them whether or not
Emscripten is installed. `npm run test:browser` drives the built shell in
headless Chromium; it needs a real wasm build plus a browser from
`npx playwright install chromium` or `UGURUGU_CHROMIUM_PATH`.

`web/src/assets/PretendardJP-ui.woff2` is the desktop font cut to Latin
and Hangul. It is committed, and it lives under `src/` rather than
`public/` so Vite emits a relative URL for it. Regenerating it needs
`pip install fonttools brotli`:

```sh
python -m fontTools.subset resources/fonts/PretendardJP-Medium.otf \
  --output-file=web/src/assets/PretendardJP-ui.woff2 \
  --flavor=woff2 --layout-features='' --no-hinting --desubroutinize \
  --unicodes='U+0020-007E,U+00A0-00FF,U+00D7,U+00B7,U+2010-2027,U+2030,U+20A9,U+2190-2193,U+3000-303F,U+3130-318F,U+AC00-D7A3'
```

The web text tool loads the original `resources/fonts/PretendardJP-Medium.otf`
through Vite only when text is used, alongside OpenType.js. The UI keeps its
existing WOFF2 subset. Both the font licence and the OpenType.js licence ship
in the web notices panel. `npm run test:browser -- text-wobble-image` runs the
content-editing regression scenario; append `--screenshots` to save desktop
and phone-sized text-panel screenshots in the system temporary directory.
