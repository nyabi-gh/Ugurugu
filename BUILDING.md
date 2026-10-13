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

CI runs these before anything is merged:

```powershell
cargo fmt --all --check
cargo deny --locked check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --release --locked
python tools/native_deps.py
python -X utf8 tools/check_docs.py
```

Running `cargo test --workspace --release --locked` as well catches
differences that only the optimised build shows.

`cargo deny` is [cargo-deny](https://github.com/EmbarkStudios/cargo-deny)
0.18.9. `tools/native_deps.py` fails when a crate the app links builds or
links native code that is not on its approved list. `tools/check_docs.py`
fails when a document links to a file that is not in the tree, or names a
backlog ID that `docs/rust/backlog.md` has no row for.

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
`tools/sandbox/prepare.ps1 -Work <new folder>` packs the build as 3.0.0 and
as 3.0.1 in a local update feed and writes a `.wsb` that runs
`tools/sandbox/accept.ps1` in a Sandbox without network or GPU: install, a
run on WARP, opening a document from a Korean path through the file
association, updating with unsaved changes, checking offline and
uninstalling. The results are in `<folder>\out\results.md`. The app looks for
updates in the folder `UGURUGU_UPDATE_FEED` names instead of GitHub.
Pushing a `v3.*` tag builds and publishes a release with
`.github/workflows/release-3.yml`.

## Ugurugu 2.x

The C++ and Qt app (2.2.13 and earlier) and its browser build are no longer
in this tree. Their sources and build instructions are at the `v2.2.13` tag.
