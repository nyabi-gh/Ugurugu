// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Writes a comparison as a folder a browser opens without a server:
//! report.json, both sides' frames, an amplified difference per frame, and
//! index.html, which plays each scene side by side, overlaid, or as the
//! difference, sortable by any metric.

use std::fs;
use std::path::Path;

use serde_json::json;

use crate::Error;
use crate::compare::Report;
use crate::image::Rgba;
use crate::metrics;
use crate::reference;

pub fn write(
    report: &Report,
    a: &Path,
    b: &Path,
    labels: [&str; 2],
    output: &Path,
) -> Result<(), Error> {
    fs::create_dir_all(output)?;
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(report)?,
    )?;
    let mut scenes = Vec::new();
    for scene in &report.scenes {
        let (files_a, files_b) = (
            reference::scene_files(&a.join(&scene.name))?,
            reference::scene_files(&b.join(&scene.name))?,
        );
        let mut frames = Vec::new();
        for (&frame, path_a) in &files_a.frames {
            let Some(path_b) = files_b.frames.get(&frame) else {
                continue;
            };
            let name = format!("frame-{frame:04}.png");
            for (side, path) in [("a", path_a), ("b", path_b)] {
                let directory = output.join(side).join(&scene.name);
                fs::create_dir_all(&directory)?;
                fs::copy(path, directory.join(&name))?;
            }
            let directory = output.join("difference").join(&scene.name);
            fs::create_dir_all(&directory)?;
            metrics::difference_image(&Rgba::read_png(path_a)?, &Rgba::read_png(path_b)?)
                .write_png(&directory.join(&name))?;
            frames.push(name);
        }
        scenes.push(json!({
            "name": scene.name,
            "group": scene.group,
            "parameters": scene.parameters,
            "frames": frames,
            "worst": scene.worst,
        }));
    }
    let data = json!({"labels": labels, "scenes": scenes, "summary": report.summary});
    // `</` inside the embedded JSON would end the script element early.
    let data = serde_json::to_string(&data)?.replace("</", "<\\/");
    fs::write(
        output.join("index.html"),
        PAGE.replace("/*DATA*/null", &data),
    )?;
    Ok(())
}

const PAGE: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Feel comparison</title>
<style>
:root { --bg: #f6f6f4; --fg: #1d2129; --muted: #6b7078; --line: #d9d9d4; --card: #fff; --warn: #b3261e; }
@media (prefers-color-scheme: dark) { :root { --bg: #16181c; --fg: #e6e6e3; --muted: #9a9ea6; --line: #2c2f35; --card: #1e2126; --warn: #f2b8b5; } }
body { margin: 0; padding: 16px; background: var(--bg); color: var(--fg); font: 14px/1.4 system-ui, sans-serif; }
header { display: flex; flex-wrap: wrap; gap: 12px; align-items: center; margin-bottom: 12px; }
h1 { font-size: 18px; margin: 0 16px 0 0; }
select, label { font: inherit; }
.scene { background: var(--card); border: 1px solid var(--line); border-radius: 8px; padding: 10px; margin-bottom: 10px; }
.scene h2 { font-size: 14px; margin: 0 0 6px; }
.scene .group { color: var(--muted); font-weight: normal; }
.views { display: flex; flex-wrap: wrap; gap: 8px; }
figure { margin: 0; }
figcaption { color: var(--muted); font-size: 12px; }
.stack { position: relative; }
.stack img + img { position: absolute; left: 0; top: 0; opacity: 0.5; }
img { display: block; image-rendering: pixelated; width: 320px; max-width: 100%; background: repeating-conic-gradient(#ccc 0 25%, #fff 0 50%) 0 0 / 16px 16px; }
table { border-collapse: collapse; font-size: 12px; margin-top: 6px; }
td { padding: 1px 8px 1px 0; }
td.value { text-align: right; font-variant-numeric: tabular-nums; }
.sorted { color: var(--warn); font-weight: 600; }
</style>
</head>
<body>
<header>
<h1>Feel comparison</h1>
<label>Group <select id="group"></select></label>
<label>Sort by <select id="sort"></select></label>
<label><input type="checkbox" id="overlay"> Overlay B on A</label>
<label><input type="checkbox" id="play" checked> Play</label>
</header>
<main id="scenes"></main>
<script>
const data = /*DATA*/null;
const [labelA, labelB] = data.labels;
const groupSelect = document.getElementById("group");
const sortSelect = document.getElementById("sort");
const overlay = document.getElementById("overlay");
const play = document.getElementById("play");
const list = document.getElementById("scenes");
const groups = ["all", ...new Set(data.scenes.map((scene) => scene.group))];
const metricNames = [...new Set(data.scenes.flatMap((scene) => Object.keys(scene.worst)))].sort();
groupSelect.innerHTML = groups.map((group) => `<option>${group}</option>`).join("");
sortSelect.innerHTML = ["name", ...metricNames].map((metric) => `<option>${metric}</option>`).join("");
let tick = 0;
function format(value) {
  if (value === null || value === undefined) return "∞";
  return Math.abs(value) >= 100 || Number.isInteger(value) ? String(value) : value.toFixed(4);
}
function render() {
  const group = groupSelect.value;
  const metric = sortSelect.value;
  const scenes = data.scenes.filter((scene) => group === "all" || scene.group === group);
  if (metric !== "name") {
    const key = (scene) => scene.worst[metric] ?? -1;
    scenes.sort((a, b) => key(b) - key(a));
  }
  list.innerHTML = scenes.map((scene) => {
    const first = scene.frames[0] ?? "";
    const rows = Object.entries(scene.worst).map(([name, value]) =>
      `<tr class="${name === metric ? "sorted" : ""}"><td>${name}</td><td class="value">${format(value)}</td></tr>`).join("");
    const a = `<img loading="lazy" data-side="a" src="a/${scene.name}/${first}" alt="">`;
    const b = `<img loading="lazy" data-side="b" src="b/${scene.name}/${first}" alt="">`;
    const pair = overlay.checked
      ? `<figure><div class="stack">${a}${b}</div><figcaption>${labelA} + ${labelB}</figcaption></figure>`
      : `<figure>${a}<figcaption>${labelA}</figcaption></figure><figure>${b}<figcaption>${labelB}</figcaption></figure>`;
    return `<section class="scene" data-name="${scene.name}">
      <h2>${scene.name} <span class="group">${scene.group}</span></h2>
      <div class="views">${pair}
        <figure><img loading="lazy" data-side="difference" src="difference/${scene.name}/${first}" alt=""><figcaption>difference ×4</figcaption></figure>
      </div>
      <table>${rows}</table>
    </section>`;
  }).join("");
}
function advance() {
  if (!play.checked) return;
  tick += 1;
  for (const section of list.children) {
    const scene = data.scenes.find((candidate) => candidate.name === section.dataset.name);
    if (!scene || scene.frames.length < 2) continue;
    const frame = scene.frames[tick % scene.frames.length];
    for (const image of section.querySelectorAll("img[data-side]")) {
      image.src = `${image.dataset.side}/${scene.name}/${frame}`;
    }
  }
}
for (const control of [groupSelect, sortSelect, overlay]) control.addEventListener("change", render);
render();
setInterval(advance, 250);
</script>
</body>
</html>
"#;
