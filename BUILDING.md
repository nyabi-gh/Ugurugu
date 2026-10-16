# Building Ugurugu

Ugurugu 3 for Windows is written in Rust.

## Ugurugu 3 (Windows)

### Requirements

- Windows 10 or later, x64
- [rustup](https://rustup.rs/); `rust-toolchain.toml` pins the toolchain
  (Rust 1.99.0) and rustup installs it on first use
- Visual Studio 2026 or its Build Tools with the Desktop development with
  C++ workload: the MSVC linker, the resource compiler, and the C compiler
  for libwebp and ring
- Python 3 on `PATH` as `python`, for the scripts in `tools/` (standard
  library only): the native dependency and document checks, the translation
  files, and the third-party notices the package includes
- [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) 0.18.9 for the
  dependency check: `cargo install cargo-deny --version 0.18.9 --locked`
- To make the installer: the [.NET SDK](https://dotnet.microsoft.com/download),
  which installs `vpk`, and cargo-about (see Package)
- To change `.github/scripts/`: [Node.js](https://nodejs.org/), which runs
  the download badge test

### Build and run

```powershell
cargo build --package ugurugu --release --locked
.\target\release\ugurugu.exe
```

The C runtime is linked statically (`.cargo/config.toml`), so the
executable runs on a clean Windows without the Visual C++ redistributable.
To keep a development run apart from an installed Ugurugu 3, point
`UGURUGU_SETTINGS_PATH` at a settings file and `UGURUGU_RECOVERY_PATH` and
`UGURUGU_LOG_PATH` at folders of their own.

### Checks

CI (`.github/workflows/ci.yml`) runs two jobs before anything is merged.
"Format and release metadata" checks that every tracked `.rs` and `.js` file
has the `SPDX-License-Identifier` line, runs `tools/check_docs.py` and the
download badge test (`node --test
.github/scripts/update-download-badge.test.js`), and checks that the
workspace version has release notes in `release-notes/<version>.en.md`.
"Rust Windows" runs only when something other than documents changed (the
paths are listed in the "Detect changes" job):

```powershell
cargo fmt --all --check
cargo deny --locked check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --release --locked
python tools/native_deps.py
```

It then packs the release build with `tools/package_windows.ps1` and installs,
runs and uninstalls the result with `tools/package_smoke.ps1` (see Package).
Run `python -X utf8 tools/check_docs.py` yourself as well.

CI tests the debug build only, and the release workflow runs no tests. Also
run `cargo test --workspace --release --locked` before you commit: it catches
differences that only the optimised build shows (`docs/rust/backlog.md`
B3-116 records one).

`tools/native_deps.py` fails when a crate the app links builds or links
native code that is not on its approved list. `tools/check_docs.py` fails
when a document links to a file that is not in the tree, or names a backlog
ID that `docs/rust/backlog.md` has no row for.

### Package

The installer is made with [Velopack](https://velopack.io/) `vpk` 1.2.161,
the version of the `velopack` crate, and the third-party notices with
[cargo-about](https://github.com/EmbarkStudios/cargo-about) 0.9.2:

```powershell
dotnet tool install --global vpk --version 1.2.161
cargo install cargo-about --version 0.9.2 --locked --features cli
.\tools\package_windows.ps1 -Executable target\release\ugurugu.exe -OutputDirectory artifacts
```

`tools/package_smoke.ps1` installs the result, runs it and uninstalls it.
It changes the current user's file association and shortcuts while it
runs, so CI runs it on a fresh runner; on your own PC, use Windows Sandbox.
`tools/sandbox/prepare.ps1 -Work <new folder>` packs the build as the
workspace version and as the next patch version in a local update feed and
writes a `.wsb` that runs `tools/sandbox/accept.ps1` in a Sandbox without
network or GPU. Its 11 steps are: install, run, a run on WARP, opening a
document from a Korean path through the file association, the taskbar's
recent documents, an update that fails to download, an update offered at
every start, updating with unsaved changes, checking for updates offline,
uninstalling, and installing and running as a standard user. The results are
in `<folder>\out\results.md`. The app looks for updates in the folder
`UGURUGU_UPDATE_FEED` names instead of GitHub.
Pushing a `v3.*` tag builds and publishes a release with
`.github/workflows/release-3.yml`, once main's CI has succeeded on that
commit; the release workflow runs no tests of its own.

## Ugurugu 2.x

The C++ and Qt app (2.2.13 and earlier) and its browser build are no longer
in this tree. Their sources and build instructions are at the `v2.2.13` tag.
