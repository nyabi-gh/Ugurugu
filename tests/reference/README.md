# Reference scene matrix

The scenes the Rust port's drawing and wobble feel is compared on
(docs/RUST_PORT_PLAN.md §4). Generated, not edited by hand.

- `scenes/`: `<name>.ugu` for each scene and `matrix.json` listing them with
  their group and parameters. `ugurugu_reference_scenes` builds them
  deterministically, so regenerating gives the same bytes.
- `cpp/<platform>/<name>/`: what the C++ engine makes of each scene, from
  `ugurugu_reference_export all --frames 0,3,6,9`: `scene.json`,
  `geometry.jsonl`, `frames/`, `strokes/` and `manifest.json`.

Regenerate both from a Release build of this checkout:

```sh
cmake --build out/build/windows-release --config Release --target ugurugu_reference_scenes ugurugu_reference_export
node tools/reference_matrix.mjs out/build/windows-release/Release windows
```

The C++ references are frozen once the Rust port's feel is signed off; until
then, a C++ change that alters rendering regenerates them.

Compare two sides by the feel metrics of docs/RUST_PORT_PLAN.md §4.1 and open
the side-by-side viewer:

```sh
cargo run -p ugu-reference --release -- compare tests/reference/cpp/windows tests/reference/cpp/macos --matrix tests/reference/scenes/matrix.json --out target/reference-report --labels windows,macos
```

`target/reference-report/report.json` has every scene's metrics and their
per-group median, p95 and max; `index.html` plays both sides, overlaid or as
the difference, sorted by any metric. Browsers block local images on some
`file://` setups, so serve the folder (for example `python -m http.server`)
if the frames do not show.
