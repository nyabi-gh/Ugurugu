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

CI (`.github/workflows/ci.yml`) runs these jobs before anything is merged.
"Sources and release metadata" checks that every tracked `.rs`, `.js`,
`.py`, `.ps1` and `.sh` file has the `SPDX-License-Identifier` line, runs
`tools/check_docs.py` and the download badge test (`node --test
.github/scripts/update-download-badge.test.js`), checks that the `.ftl` files
are what `tools/strings_to_ftl.py` makes, and checks that the workspace
version has release notes in `release-notes/<version>.en.md`. The others run
only when something other than documents changed (the paths are listed in
the "Detect changes" job), side by side: "Format and dependencies" on Ubuntu,
and "Rust Windows tests", "Rust Windows release tests" and "Windows package"
on Windows. Together they run:

```powershell
cargo fmt --all --check
cargo deny --locked check bans licenses sources
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --release --locked
python tools/native_deps.py
cargo build --package ugurugu --release --locked
```

The release build is the command `release-3.yml` builds the installer's
executable with; "Windows package" then packs it with
`tools/package_windows.ps1` and installs, runs and uninstalls the result with
`tools/package_smoke.ps1` (see Package). CI leaves the advisories of
`cargo deny` out of pull requests, so an advisory published today fails no
unrelated change; it checks them weekly (`advisories.yml`) and the release
workflow checks them before it builds. Locally, run the commands above with
the whole `cargo deny --locked check`, and also:

```powershell
python -X utf8 tools/check_docs.py
python -X utf8 tools/strings_to_ftl.py; git diff --exit-code apps/ugurugu/i18n
```

`tools/native_deps.py` fails when a crate the app links builds or links
native code that is not on its approved list. `tools/check_docs.py` fails
when a document links to a file that is not in the tree, or names a backlog
ID that `docs/rust/backlog.md` has no row for. The last line fails when the
`.ftl` files are not what `strings.txt` makes.

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
commit and no advisory is open; the release workflow runs no tests of its own.

## Ugurugu 2.x

The C++ and Qt app (2.2.13 and earlier) and its browser build are no longer
in this tree. Their sources and build instructions are at the `v2.2.13` tag.
