// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Headless `.ugurugu` tool.
//!
//! - `ugu-doc fixture <1-5> <out.ugurugu>`: writes a comparison fixture with the
//!   same make-up as the 2.2.13 ones (docs/rust/m0-evidence.md section 2):
//!   the same canvases, layers, stroke and point counts and stroke shapes,
//!   from a different random sequence.
//! - `ugu-doc many <operations> <layers> <out.ugurugu>`: fixture 4's short
//!   strokes, as many as asked, spread over that many layers, to measure the
//!   operation limit (docs/rust/m3-plan.md M3-10).
//! - `ugu-doc info <file.ugurugu>`: reads, validates and summarises a file, and
//!   lists its layers top first.
//! - `ugu-doc bench <file.ugurugu> [rounds]`: times reading and saving it.
//! - `ugu-doc render <file.ugurugu> [threads [tile]]`: times drawing every frame, by
//!   stage, and editing splits, with the peak working set. Other brushes are
//!   drawn as pens, which keeps the amount of work close and says so.
//! - `ugu-doc pen-only <in.ugurugu> <out.ugurugu>`: makes brushes pens, keeping
//!   everything else, so the app can open a fixture to measure with.
//! - `ugu-doc reframe <in.ugurugu> <out.ugurugu> <crop|resample|both>`: makes
//!   brushes pens and puts in the middle of every layer a crop to 7/8 of the
//!   canvas around its middle, a smooth resample to 3/4, or both one after
//!   the other, to measure what canvas changes cost to draw.
//! - `ugu-doc sparse <in.ugurugu> <out.ugurugu>`: gathers each layer's strokes into
//!   a fifth of the canvas, for work that leaves most of a layer empty.
//! - `ugu-doc fills <in.ugurugu> <out.ugurugu>`: adds a large clipped fill and a
//!   clear to every layer, to measure what masks cost to draw.
//! - `ugu-doc stop <file.ugurugu>`: times how long a frame render on 8 threads
//!   takes to end once it is told to stop, at points spread over the render.
//! - `ugu-doc layers <file.ugurugu> <threads>`: times each layer drawn alone on
//!   one thread and how long that many threads take to draw them all.

use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use sha2::{Digest, Sha256};
use ugu_core::document::{Document, Group, Layer, LayerId, LayerKind};
use ugu_core::ops::{Affine, AssetId, Blend, MaskId, Op, PaintLayer, Rgba8, Sampling, StrokeId};
use ugu_core::store::{Asset, Brush, BrushEngine, Mask, Point, Stroke};

const USAGE: &str = "usage: ugu-doc fixture <1-5> <out.ugurugu> | many <operations> <layers> <out.ugurugu> | info <file.ugurugu> \
     | bench <file.ugurugu> [rounds] | render <file.ugurugu> [threads [tile]] | pen-only <in.ugurugu> <out.ugurugu> | sparse <in.ugurugu> <out.ugurugu> | fills <in.ugurugu> <out.ugurugu> | reframe <in.ugurugu> <out.ugurugu> <crop|resample|both> | stop <file.ugurugu> | layers <file.ugurugu> <threads>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["fixture", number, out] => fixture(number, Path::new(out)),
        ["many", operations, layers, out] => match (operations.parse(), layers.parse()) {
            (Ok(operations), Ok(layers)) if layers > 0 => {
                let document = short_strokes_over(operations, layers);
                document
                    .validate()
                    .map_err(|error| error.to_string())
                    .and_then(|()| save(&document, Path::new(out)))
                    .and_then(|()| info(Path::new(out)))
            }
            _ => Err(USAGE.to_owned()),
        },
        ["info", file] => info(Path::new(file)),
        ["bench", file] => bench(Path::new(file), 10),
        ["bench", file, rounds] => rounds
            .parse()
            .map_err(|_| USAGE.to_owned())
            .and_then(|rounds| bench(Path::new(file), rounds)),
        ["render", file] => render(Path::new(file), 8, None),
        ["pen-only", from, to] => pen_only(Path::new(from), Path::new(to)),
        ["sparse", from, to] => sparse(Path::new(from), Path::new(to)),
        ["fills", from, to] => fills(Path::new(from), Path::new(to)),
        ["reframe", from, to, which] => reframe(Path::new(from), Path::new(to), which),
        ["stop", file] => stop(Path::new(file)),
        ["layers", file, threads] => threads
            .parse()
            .map_err(|_| USAGE.to_owned())
            .and_then(|threads| layers(Path::new(file), threads)),
        ["render", file, threads] => threads
            .parse()
            .map_err(|_| USAGE.to_owned())
            .and_then(|threads| render(Path::new(file), threads, None)),
        ["render", file, threads, tile] => match (threads.parse(), tile.parse()) {
            (Ok(threads), Ok(tile)) => render(Path::new(file), threads, Some(tile)),
            _ => Err(USAGE.to_owned()),
        },
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn save(document: &Document, path: &Path) -> Result<(), String> {
    ugu_io::save::save(document, [0; 16], path, &ugu_win::file::replace_file)
        .map_err(|error| error.to_string())
}

fn open(path: &Path) -> Result<Document, String> {
    let file = std::fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    ugu_io::read::read(std::io::BufReader::new(file))
        .map(|(document, _)| document)
        .map_err(|error| error.to_string())
}

fn fixture(number: &str, out: &Path) -> Result<(), String> {
    let document = match number {
        "1" => simple_strokes(),
        "2" => mixed_work(),
        "3" => long_strokes(),
        "4" => short_strokes(),
        "5" => image_mask_group()?,
        _ => return Err(USAGE.to_owned()),
    };
    document.validate().map_err(|error| error.to_string())?;
    save(&document, out)?;
    info(out)
}

fn info(path: &Path) -> Result<(), String> {
    let document = open(path)?;
    let mut layers = 0;
    let mut operations = 0;
    count(&document.layers, &mut layers, &mut operations);
    let points: usize = document
        .store
        .strokes
        .values()
        .map(|stroke| stroke.points.len())
        .sum();
    let bytes = std::fs::metadata(path)
        .map_err(|error| error.to_string())?
        .len();
    println!(
        "{}: {}x{}, {} layers, {} operations, {} strokes, {} points, {} masks, {} images, {} bytes",
        path.display(),
        document.canvas[0],
        document.canvas[1],
        layers,
        operations,
        document.store.strokes.len(),
        points,
        document.store.masks.len(),
        document.store.assets.len(),
        bytes
    );
    tree(&document.layers, 1);
    Ok(())
}

/// One line per layer, top first, indented by depth.
fn tree(layers: &[Layer], depth: usize) {
    for layer in layers.iter().rev() {
        let (kind, operations) = match &layer.kind {
            LayerKind::Paint(paint) => ("paint", paint.ops.len()),
            LayerKind::Group(group) => ("group", group.children.len()),
        };
        println!(
            "{:indent$}{kind} \"{}\" {operations} {:?} {:.0}%{}{}{}",
            "",
            layer.name,
            layer.blend(),
            layer.opacity() * 100.0,
            if layer.visible { "" } else { " hidden" },
            if layer.clip_to_below() {
                " clipped"
            } else {
                ""
            },
            if layer.reference { " reference" } else { "" },
            indent = depth * 2
        );
        if let LayerKind::Group(group) = &layer.kind {
            tree(&group.children, depth + 1);
        }
    }
}

fn count(layers: &[Layer], total: &mut usize, operations: &mut usize) {
    for layer in layers {
        *total += 1;
        match &layer.kind {
            LayerKind::Paint(paint) => *operations += paint.ops.len(),
            LayerKind::Group(group) => count(&group.children, total, operations),
        }
    }
}

/// Times opening (read and validate), encoding into memory, and saving
/// (write, flush, check and replace), in turn, then prints p50 and max.
fn bench(path: &Path, rounds: usize) -> Result<(), String> {
    let copy = path.with_extension("bench.ugurugu");
    std::fs::copy(path, &copy).map_err(|error| error.to_string())?;
    let mut opens = Vec::new();
    let mut encodes = Vec::new();
    let mut saves = Vec::new();
    for _ in 0..rounds {
        let started = Instant::now();
        let document = open(&copy)?;
        opens.push(started.elapsed().as_secs_f64() * 1000.0);
        let started = Instant::now();
        ugu_io::write::write(&document, [0; 16], std::io::Cursor::new(Vec::new()))
            .map_err(|error| error.to_string())?;
        encodes.push(started.elapsed().as_secs_f64() * 1000.0);
        let started = Instant::now();
        save(&document, &copy)?;
        saves.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    let _ = std::fs::remove_file(&copy);
    let summary = |name: &str, times: &mut Vec<f64>| {
        times.sort_by(f64::total_cmp);
        println!(
            "{name}: n={} p50={:.1} ms max={:.1} ms",
            times.len(),
            times[times.len() / 2],
            times[times.len() - 1]
        );
    };
    summary("open", &mut opens);
    summary("encode in memory", &mut encodes);
    summary("save", &mut saves);
    Ok(())
}

/// Makes every brush a pen, keeping everything else.
fn to_pens(document: &mut Document) {
    let mut changed = 0;
    for stroke in document.store.strokes.values_mut() {
        if stroke.brush.engine != BrushEngine::Line {
            stroke.brush.engine = BrushEngine::Line;
            changed += 1;
        }
    }
    if changed > 0 {
        println!("{changed} brushes made pens");
    }
}

/// Puts canvas changes in the middle of every layer; see the module notes.
fn reframe(from: &Path, to: &Path, which: &str) -> Result<(), String> {
    let mut document = open(from)?;
    to_pens(&mut document);
    let [width, height] = document.canvas;
    let cropped = [width * 7 / 8, height * 7 / 8];
    let crop = Op::Crop {
        offset: [-(width as i32 / 16), -(height as i32 / 16)],
        size: cropped,
    };
    let (changes, canvas) = match which {
        "crop" => (vec![crop], cropped),
        "resample" => {
            let size = [width * 3 / 4, height * 3 / 4];
            let resample = Op::Resample {
                size,
                sampling: Sampling::Smooth,
            };
            (vec![resample], size)
        }
        "both" => {
            let size = [cropped[0] * 3 / 4, cropped[1] * 3 / 4];
            let resample = Op::Resample {
                size,
                sampling: Sampling::Smooth,
            };
            (vec![crop, resample], size)
        }
        _ => return Err(USAGE.to_owned()),
    };
    each_paint(&mut document.layers, &mut |paint| {
        let middle = paint.ops.len() / 2;
        paint.ops.splice(middle..middle, changes.iter().cloned());
    });
    document.canvas = canvas;
    document.validate().map_err(|error| error.to_string())?;
    save(&document, to)?;
    info(to)
}

/// Adds to every paint layer a fill of a disc a third of the canvas wide,
/// cut to stripes, and a clear of a smaller disc, at different places per
/// layer, for measuring what masks cost to draw.
fn fills(from: &Path, to: &Path) -> Result<(), String> {
    let mut document = open(from)?;
    to_pens(&mut document);
    let [width, height] = document.canvas.map(|edge| edge as i32);
    let mut next = document
        .store
        .masks
        .keys()
        .map(|id| id.0 + 1)
        .max()
        .unwrap_or(0);
    let mut add = |masks: &mut std::collections::HashMap<MaskId, Mask>,
                   inside: &dyn Fn(i32, i32) -> bool| {
        let row_bytes = Mask::row_bytes(width);
        let mut bits = vec![0u8; row_bytes * height as usize];
        for y in 0..height {
            for x in 0..width {
                if inside(x, y) {
                    bits[y as usize * row_bytes + x as usize / 8] |= 0x80 >> (x % 8);
                }
            }
        }
        let id = MaskId(next);
        next += 1;
        masks.insert(
            id,
            Mask {
                bounds: [0, 0, width, height],
                bits: Arc::from(bits),
            },
        );
        id
    };
    let mut layer = 0;
    let masks = &mut document.store.masks;
    each_paint(&mut document.layers, &mut |paint| {
        let [cx, cy] = [
            width / 4 + (layer * 397) % (width / 2),
            height / 4 + (layer * 251) % (height / 2),
        ];
        let radius = width / 6;
        let disc = add(masks, &|x, y| {
            (x - cx) * (x - cx) + (y - cy) * (y - cy) < radius * radius
        });
        let stripes = add(masks, &|x, y| (x / 37 + y / 53) % 3 != 0);
        let small = add(masks, &|x, y| {
            (x - cx) * (x - cx) + (y - cy) * (y - cy) < radius * radius / 9
        });
        let at = paint.ops.len() / 2;
        paint.ops.insert(
            at,
            Op::Fill {
                coverage: disc,
                color: Rgba8([40, 140, 200, 200]),
                antialias: true,
                clip: Some(stripes),
            },
        );
        paint.ops.push(Op::ClearSelection { mask: small });
        layer += 1;
    });
    document.validate().map_err(|error| error.to_string())?;
    save(&document, to)?;
    info(to)
}

fn each_paint(layers: &mut [Layer], visit: &mut impl FnMut(&mut PaintLayer)) {
    for layer in layers {
        match &mut layer.kind {
            LayerKind::Paint(paint) => visit(paint),
            LayerKind::Group(group) => each_paint(&mut group.children, visit),
        }
    }
}

/// Gathers each layer's strokes into a box a fifth of the canvas wide,
/// boxes spread over the canvas, so most tiles of a layer stay empty.
fn sparse(from: &Path, to: &Path) -> Result<(), String> {
    let mut document = open(from)?;
    let size = document.canvas.map(|edge| edge as f32);
    let mut layer_strokes: Vec<Vec<StrokeId>> = Vec::new();
    each_paint(&mut document.layers, &mut |paint| {
        fn ids(ops: &[Op], into: &mut Vec<StrokeId>) {
            for op in ops {
                match op {
                    Op::Paint { stroke, .. } | Op::Erase { stroke, .. } => into.push(*stroke),
                    Op::Isolated(section) => ids(&section.ops, into),
                    _ => {}
                }
            }
        }
        let mut found = Vec::new();
        ids(&paint.ops, &mut found);
        layer_strokes.push(found);
    });
    for (index, strokes) in layer_strokes.iter().enumerate() {
        let cell = [(index * 7) % 5, (index * 3) % 5].map(|cell| cell as f32 * 0.2);
        for id in strokes {
            let stroke = document
                .store
                .strokes
                .get_mut(id)
                .ok_or("a stroke is missing")?;
            let points: Vec<Point> = stroke
                .points
                .iter()
                .map(|point| Point {
                    x: (cell[0] + point.x / size[0] * 0.2) * size[0],
                    y: (cell[1] + point.y / size[1] * 0.2) * size[1],
                    pressure: point.pressure,
                })
                .collect();
            stroke.points = Arc::from(points);
        }
    }
    document.validate().map_err(|error| error.to_string())?;
    save(&document, to)?;
    info(to)
}

fn pen_only(from: &Path, to: &Path) -> Result<(), String> {
    let mut document = open(from)?;
    to_pens(&mut document);
    document.validate().map_err(|error| error.to_string())?;
    save(&document, to)?;
    info(to)
}

fn peak_working_set_mib() -> f64 {
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::GetCurrentProcess;
    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    // SAFETY: valid out pointer of the stated size for this process.
    let _ = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
    counters.PeakWorkingSetSize as f64 / (1024.0 * 1024.0)
}

/// `UGU_SURFACE_BUDGET_MIB` sets the layer surface budget, to measure what
/// happens beyond it.
fn set_budget(renderer: &mut ugu_render::document::DocumentRenderer) {
    if let Some(mib) = std::env::var("UGU_SURFACE_BUDGET_MIB")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
    {
        renderer.set_surface_budget(mib * 1024 * 1024);
    }
}

fn percentiles(times: &mut [f64]) -> String {
    times.sort_by(f64::total_cmp);
    format!(
        "n={} p50={:.1} p95={:.1} max={:.1} ms",
        times.len(),
        times[times.len() / 2],
        times[times.len() * 95 / 100],
        times[times.len() - 1]
    )
}

/// Draws every frame once to warm up, then twice more, and prints the
/// first frame's time and p50 and max per frame of the later rounds.
/// `UGU_SHRINK` (2, 4, 8) draws the frames smaller, as playback does.
fn render(path: &Path, threads: u16, tile: Option<u32>) -> Result<(), String> {
    use ugu_render::document::{
        DocumentRenderer, Purpose, TILE_EDGE, scaled_size, surface_estimate,
    };
    use ugu_render::plan::RenderPlan;

    let mut document = open(path)?;
    to_pens(&mut document);
    let shrink = std::env::var("UGU_SHRINK")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|shrink| shrink.is_power_of_two())
        .unwrap_or(1);
    let plan = RenderPlan::new(&document, Purpose::Display);
    let [width, height] = scaled_size(document.canvas, shrink).map(|edge| edge as u16);
    let mut pixmap = vello_cpu::Pixmap::new(width, height);
    let mut renderer = DocumentRenderer::new(threads);
    set_budget(&mut renderer);
    if let Some(tile) = tile {
        renderer.set_tile_edge(tile);
    }
    let mut times = Vec::new();
    let mut stages = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    let mut first = 0.0;
    for round in 0..3 {
        for frame in 0..i64::from(document.frames) {
            let started = Instant::now();
            renderer.render_scaled(&document, &plan, frame, None, shrink, &mut pixmap);
            let ms = started.elapsed().as_secs_f64() * 1000.0;
            if round == 0 && frame == 0 {
                first = ms;
            } else if round > 0 {
                times.push(ms);
                let timings = renderer.timings();
                for (stage, time) in stages.iter_mut().zip([
                    timings.outlines,
                    timings.encode,
                    timings.rasterize,
                    timings.composite,
                ]) {
                    stage.push(time.as_secs_f64() * 1000.0);
                }
            }
        }
    }
    // Summed over threads when layers are drawn at once.
    let [outlines, encode, rasterize, composite] = &mut stages;
    println!("  outlines  {}", percentiles(outlines));
    println!("  encode    {}", percentiles(encode));
    println!("  rasterize {}", percentiles(rasterize));
    println!("  composite {}", percentiles(composite));
    println!(
        "layer surfaces: {:.0} MiB in the last frame",
        renderer.surface_bytes() as f64 / (1024.0 * 1024.0)
    );
    let estimates: Vec<String> = [1, 2, 4, 8]
        .map(|shrink| {
            let bytes = surface_estimate(&document, &plan, shrink, TILE_EDGE);
            format!("1/{shrink} {:.0}", bytes as f64 / (1024.0 * 1024.0))
        })
        .into();
    println!("layer surface estimate (MiB): {}", estimates.join(", "));
    let whole_peak = peak_working_set_mib();
    splits(&document, threads);
    println!(
        "peak working set: {whole_peak:.0} MiB after whole frames, {:.0} MiB in the end",
        peak_working_set_mib()
    );
    // The part of a frame spent making stroke outlines on one thread.
    let started = Instant::now();
    let mut samples_total = 0;
    let all_samples: Vec<_> = document
        .store
        .strokes
        .values()
        .map(|stroke| {
            let samples = ugu_render::stroke::Resampler::whole(
                &stroke.points,
                ugu_render::stroke::spacing(stroke.width),
            );
            samples_total += samples.len();
            (stroke, samples)
        })
        .collect();
    let resampled = started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    for (stroke, samples) in &all_samples {
        let pen = ugu_render::stroke::Pen {
            width: stroke.width,
            brush: stroke.brush,
            seed: stroke.seed,
            wobble: f64::from(document.wobble.amount),
        };
        std::hint::black_box(ugu_render::stroke::displaced(samples, &pen, 1));
    }
    println!(
        "displacement only: {:.1} ms",
        started.elapsed().as_secs_f64() * 1000.0
    );
    let started = Instant::now();
    let mut elements = 0;
    for (stroke, samples) in &all_samples {
        let pen = ugu_render::stroke::Pen {
            width: stroke.width,
            brush: stroke.brush,
            seed: stroke.seed,
            wobble: f64::from(document.wobble.amount),
        };
        if let Some(path) = ugu_render::stroke::outline(samples, &pen, 1) {
            elements += path.elements().len();
        }
    }
    println!(
        "geometry on 1 thread: resample {resampled:.1} ms ({samples_total} samples), \
         outlines {:.1} ms ({elements} path elements)",
        started.elapsed().as_secs_f64() * 1000.0
    );
    println!(
        "{threads} threads: first frame {first:.1} ms, then per frame {}",
        percentiles(&mut times)
    );
    Ok(())
}

/// Each layer's own drawing time on one thread (p50 over every frame of two
/// rounds), and how long `threads` threads would take to draw them all when
/// they take layers in document order, as the renderer does, or the most
/// expensive first.
fn layers(path: &Path, threads: usize) -> Result<(), String> {
    use ugu_render::document::{DocumentRenderer, Purpose};
    use ugu_render::plan::RenderPlan;

    fn keep_only(layers: &mut [Layer], keep: ugu_core::document::LayerId) {
        for layer in layers {
            match &mut layer.kind {
                LayerKind::Paint(paint) => {
                    paint.clip_to_below = false;
                    if layer.id != keep {
                        paint.ops.clear();
                    }
                }
                LayerKind::Group(group) => keep_only(&mut group.children, keep),
            }
        }
    }

    let mut document = open(path)?;
    to_pens(&mut document);
    let plan = RenderPlan::new(&document, Purpose::Display);
    let [width, height] = document.canvas.map(|edge| edge as u16);
    let mut pixmap = vello_cpu::Pixmap::new(width, height);
    let mut costs = Vec::new();
    for (id, _) in &plan.layers {
        let mut alone = document.clone();
        keep_only(&mut alone.layers, *id);
        let alone_plan = RenderPlan::new(&alone, Purpose::Display);
        let mut renderer = DocumentRenderer::new(0);
        let mut stages = [Vec::new(), Vec::new(), Vec::new()];
        for round in 0..3 {
            for frame in 0..i64::from(alone.frames) {
                renderer.render_scaled(&alone, &alone_plan, frame, None, 1, &mut pixmap);
                if round > 0 {
                    let timings = renderer.timings();
                    for (stage, time) in
                        stages
                            .iter_mut()
                            .zip([timings.outlines, timings.encode, timings.rasterize])
                    {
                        stage.push(time.as_secs_f64() * 1000.0);
                    }
                }
            }
        }
        let median = |times: &mut Vec<f64>| {
            times.sort_by(f64::total_cmp);
            times[times.len() / 2]
        };
        let [outlines, encode, rasterize] = stages.each_mut().map(median);
        let name = document.layer(*id).map_or("", |layer| layer.name.as_str());
        let strokes = match document.layer(*id).map(|layer| &layer.kind) {
            Some(LayerKind::Paint(paint)) => paint.ops.len(),
            _ => 0,
        };
        let total = outlines + encode + rasterize;
        let painted = renderer.surface(*id).map_or(0, |surface| {
            (0..alone.canvas[1])
                .map(|y| {
                    surface
                        .row(y, 0, alone.canvas[0])
                        .map(|(_, part)| part.iter().filter(|pixel| pixel[3] != 0).count())
                        .sum::<usize>()
                })
                .sum()
        });
        let share = painted as f64 * 100.0 / f64::from(alone.canvas[0] * alone.canvas[1]);
        println!(
            "{:>3} {name:<24} ops {strokes:>5}  outlines {outlines:>5.1}  encode {encode:>5.1}  \
             rasterize {rasterize:>5.1}  total {total:>5.1} ms  painted {share:>4.1}%",
            id.0
        );
        costs.push(total);
    }
    let finish = |order: &[f64]| {
        let mut free = vec![0.0f64; threads];
        for cost in order {
            let earliest = free
                .iter_mut()
                .min_by(|a, b| a.total_cmp(b))
                .expect("at least one thread");
            *earliest += cost;
        }
        free.into_iter().fold(0.0, f64::max)
    };
    let mut largest_first = costs.clone();
    largest_first.sort_by(|a, b| b.total_cmp(a));
    let sum: f64 = costs.iter().sum();
    println!(
        "sum {sum:.1} ms, largest {:.1} ms, {threads} threads: document order {:.1} ms, \
         largest first {:.1} ms, even split {:.1} ms",
        largest_first[0],
        finish(&costs),
        finish(&largest_first),
        sum / threads as f64
    );
    Ok(())
}

fn stop(path: &Path) -> Result<(), String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use ugu_render::document::{DocumentRenderer, Purpose};
    use ugu_render::plan::RenderPlan;

    let mut document = open(path)?;
    to_pens(&mut document);
    let plan = RenderPlan::new(&document, Purpose::Display);
    let [width, height] = document.canvas.map(|edge| edge as u16);
    let mut pixmap = vello_cpu::Pixmap::new(width, height);
    let flag = Arc::new(AtomicBool::new(false));
    // Without reuse, so every render draws every layer.
    let mut renderer = DocumentRenderer::new(8);
    renderer.set_stop(flag.clone());
    let started = Instant::now();
    renderer.render_plan(&document, &plan, 0, None, &mut pixmap);
    let whole = started.elapsed();
    let rounds = 40;
    let mut waits = Vec::new();
    for round in 0..rounds {
        flag.store(false, Ordering::Relaxed);
        let delay = whole.mul_f64(f64::from(round) / f64::from(rounds));
        let finished = std::thread::scope(|scope| {
            let setter = scope.spawn(|| {
                std::thread::sleep(delay);
                flag.store(true, Ordering::Relaxed);
                Instant::now()
            });
            renderer.render_plan(&document, &plan, i64::from(round), None, &mut pixmap);
            let returned = Instant::now();
            let set = setter.join().expect("the stop setter ran");
            returned.checked_duration_since(set)
        });
        if let Some(wait) = finished {
            waits.push(wait.as_secs_f64() * 1000.0);
        }
    }
    println!(
        "whole render {:.1} ms; from stop to return: {}",
        whole.as_secs_f64() * 1000.0,
        percentiles(&mut waits)
    );
    Ok(())
}

/// Times the editing split on frame 0: the first one, which draws every
/// layer, and one per shown layer after a stroke on it, which draws that
/// layer only. Each layer then takes a stroke at pen-up, timed as the app
/// shows it: added to the layer's surface and put together where it changed.
fn splits(document: &Document, threads: u16) {
    use ugu_core::history::History;
    use ugu_render::compose::{Stamp, composite};
    use ugu_render::document::{DocumentRenderer, Purpose};
    use ugu_render::plan::RenderPlan;

    let mut history = History::new(document.clone(), true);
    let layers: Vec<LayerId> = RenderPlan::new(document, Purpose::Display)
        .layers
        .iter()
        .map(|(id, _)| *id)
        .collect();
    let [width, height] = document.canvas.map(|edge| edge as u16);
    let mut display = vello_cpu::Pixmap::new(width, height);
    let mut renderer = DocumentRenderer::new(threads);
    set_budget(&mut renderer);
    let started = Instant::now();
    renderer.split(
        history.document(),
        layers[0],
        0,
        Some(history.layer_revisions()),
        &mut display,
    );
    let first = started.elapsed().as_secs_f64() * 1000.0;
    let mut stamp = Stamp::default();
    let (mut later, mut pen_ups) = (Vec::new(), Vec::new());
    for layer in layers {
        let Some(LayerKind::Paint(paint)) =
            history.document().layer(layer).map(|layer| &layer.kind)
        else {
            continue;
        };
        let Some(stroke) = paint.ops.iter().find_map(|op| match op {
            Op::Paint { stroke, .. } => history.document().store.strokes.get(stroke).cloned(),
            _ => None,
        }) else {
            continue;
        };
        let wobble = paint.wobble.unwrap_or(history.document().wobble).amount;
        // As the app does whenever its worker runs out of work, to time it.
        if std::env::var_os("UGU_RELEASE_SCRATCH").is_some() {
            renderer.release_scratch();
        }
        let started = Instant::now();
        let mut split = renderer
            .split(
                history.document(),
                layer,
                0,
                Some(history.layer_revisions()),
                &mut display,
            )
            .expect("nothing stops it");
        later.push(started.elapsed().as_secs_f64() * 1000.0);
        let started = Instant::now();
        if let Some(rect) = split.stamp(&mut stamp, &stroke, false, wobble) {
            composite(&split, None, rect, &mut display);
        }
        pen_ups.push(started.elapsed().as_secs_f64() * 1000.0);
        // Refused at the operation limit, which leaves the next split as it is.
        history
            .edit("Draw", |document| {
                ugu_core::command::draw(document, layer, stroke, false, None)
            })
            .ok();
    }
    println!("first split: {first:.1} ms");
    println!("split after a stroke: {}", percentiles(&mut later));
    println!(
        "pen-up (stamp and composite): {}",
        percentiles(&mut pen_ups)
    );
}

/// The same generator as tools/FixtureGenerator.cpp.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 16
    }

    fn unit(&mut self) -> f64 {
        (self.next() % 1_000_000) as f64 / 1_000_000.0
    }

    fn range(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.unit()
    }
}

#[derive(Clone, Copy)]
struct Shape {
    engine: BrushEngine,
    erase: bool,
    points: usize,
    width: (f64, f64),
    minimum_alpha: u64,
    /// Path length relative to the canvas edge.
    reach: f64,
}

impl Shape {
    fn plain(points: usize) -> Self {
        Self {
            engine: BrushEngine::Line,
            erase: false,
            points,
            width: (2.0, 16.0),
            minimum_alpha: 160,
            reach: 0.25,
        }
    }

    /// Mostly pen, with airbrush, spray and eraser strokes mixed in.
    fn mixed(index: usize, points: usize) -> Self {
        let mut shape = Self::plain(points);
        match index % 10 {
            6 | 7 => {
                shape.engine = BrushEngine::Airbrush;
                shape.width = (12.0, 48.0);
            }
            8 => {
                shape.engine = BrushEngine::Spray;
                shape.width = (16.0, 40.0);
            }
            9 => {
                shape.erase = true;
                shape.width = (8.0, 24.0);
            }
            _ => {}
        }
        shape
    }
}

/// A wandering stroke whose pressure rises and falls like a hand-drawn one.
fn make_stroke(seed: u64, canvas: [u32; 2], shape: Shape) -> Stroke {
    let mut random = Random(seed.wrapping_mul(2654435761).wrapping_add(1));
    let stroke_seed = random.next();
    let width = random.range(shape.width.0, shape.width.1) as f32;
    let alpha = shape.minimum_alpha + random.next() % (256 - shape.minimum_alpha);
    let color = [
        random.next() % 256,
        random.next() % 256,
        random.next() % 256,
        alpha,
    ]
    .map(|value| value as u8);
    let [width_px, height_px] = canvas.map(f64::from);
    let edge = width_px.min(height_px);
    let step = edge * shape.reach / (shape.points.max(2) - 1) as f64;
    let mut x = random.range(0.05, 0.95) * width_px;
    let mut y = random.range(0.05, 0.95) * height_px;
    let mut heading = random.range(0.0, std::f64::consts::TAU);
    let points = (0..shape.points)
        .map(|index| {
            heading += random.range(-0.3, 0.3);
            x = (x + heading.cos() * step).clamp(0.0, width_px - 1.0);
            y = (y + heading.sin() * step).clamp(0.0, height_px - 1.0);
            let progress = if shape.points > 1 {
                index as f64 / (shape.points - 1) as f64
            } else {
                0.5
            };
            Point {
                x: x as f32,
                y: y as f32,
                pressure: (0.3 + 0.7 * (progress * std::f64::consts::PI).sin()) as f32,
            }
        })
        .collect::<Vec<_>>();
    Stroke {
        points: points.into(),
        color: Rgba8(color),
        width,
        brush: Brush {
            engine: shape.engine,
            opacity: 1.0,
            hardness: 1.0,
            antialias: false,
            size_dynamics: 0.8,
            wobble_scale: 1.0,
            ..Brush::default()
        },
        seed: stroke_seed,
    }
}

struct Builder {
    document: Document,
    next_layer: u32,
    next_stroke: u32,
}

impl Builder {
    fn new(canvas: [u32; 2]) -> Self {
        let mut document = Document::new(canvas);
        document.layers.clear();
        Self {
            document,
            next_layer: 1,
            next_stroke: 0,
        }
    }

    fn layer(&mut self, name: &str, kind: LayerKind) -> Layer {
        let id = LayerId(self.next_layer);
        self.next_layer += 1;
        Layer {
            id,
            name: name.to_owned(),
            visible: true,
            reference: false,
            kind,
        }
    }

    fn paint(&mut self, name: &str, strokes: usize, shape: impl Fn(usize) -> Shape) -> Layer {
        let canvas = self.document.canvas;
        let ops = (0..strokes)
            .map(|index| {
                let id = StrokeId(self.next_stroke);
                self.next_stroke += 1;
                let shape = shape(index);
                self.document
                    .store
                    .strokes
                    .insert(id, make_stroke(u64::from(id.0), canvas, shape));
                if shape.erase {
                    Op::Erase {
                        stroke: id,
                        clip: None,
                    }
                } else {
                    Op::Paint {
                        stroke: id,
                        clip: None,
                    }
                }
            })
            .collect();
        self.layer(
            name,
            LayerKind::Paint(PaintLayer {
                ops,
                opacity: 1.0,
                blend: Blend::Normal,
                clip_to_below: false,
                wobble: None,
                initial_size: canvas,
            }),
        )
    }

    fn group(&mut self, name: &str, blend: Blend, opacity: f32, children: Vec<Layer>) -> Layer {
        self.layer(
            name,
            LayerKind::Group(Group {
                opacity,
                blend,
                clip_to_below: false,
                children,
            }),
        )
    }
}

fn set_paint(layer: &mut Layer, update: impl FnOnce(&mut PaintLayer)) {
    if let LayerKind::Paint(paint) = &mut layer.kind {
        update(paint);
    }
}

/// 1: one layer of plain pen strokes on a small canvas.
fn simple_strokes() -> Document {
    let mut builder = Builder::new([1024, 1024]);
    let layer = builder.paint("Simple", 300, |_| Shape {
        width: (2.0, 12.0),
        ..Shape::plain(60)
    });
    builder.document.layers.push(layer);
    builder.document
}

/// 2: fourteen paint layers using every blend mode and opacity, two groups
/// and clipping.
fn mixed_work() -> Document {
    let mut builder = Builder::new([2048, 2048]);
    let blends = [
        Blend::Normal,
        Blend::Multiply,
        Blend::Screen,
        Blend::Overlay,
    ];
    let paint = |builder: &mut Builder, index: usize| {
        let mut layer = builder.paint(&format!("Paint {index}"), 120, |stroke| {
            Shape::mixed(stroke, 80)
        });
        set_paint(&mut layer, |paint| {
            paint.blend = blends[index % 4];
            paint.opacity = 0.6 + 0.4 * ((index * 37) % 10) as f32 / 9.0;
            paint.clip_to_below = matches!(index, 4 | 5 | 10 | 13);
        });
        layer
    };
    let mut top = Vec::new();
    for index in 0..3 {
        top.push(paint(&mut builder, index));
    }
    let children = (3..=6).map(|index| paint(&mut builder, index)).collect();
    top.push(builder.group("Group A", Blend::Normal, 1.0, children));
    for index in 7..=8 {
        top.push(paint(&mut builder, index));
    }
    let children = (9..=11).map(|index| paint(&mut builder, index)).collect();
    top.push(builder.group("Group B", Blend::Multiply, 0.85, children));
    for index in 12..=13 {
        top.push(paint(&mut builder, index));
    }
    builder.document.layers = top;
    builder.document
}

/// 3: 2,000 strokes of 100 points on four layers, 200,000 points.
fn long_strokes() -> Document {
    let mut builder = Builder::new([2048, 2048]);
    for index in 0..4 {
        let layer = builder.paint(&format!("Layer {index}"), 500, |_| Shape::plain(100));
        builder.document.layers.push(layer);
    }
    builder.document
}

/// 4: twenty thousand short strokes, the most a document may hold.
fn short_strokes() -> Document {
    short_strokes_over(ugu_core::document::limits::OPERATIONS, 1)
}

/// `operations` short strokes of six points, spread evenly over `layers`.
fn short_strokes_over(operations: usize, layers: usize) -> Document {
    let mut builder = Builder::new([2048, 2048]);
    for index in 0..layers {
        let strokes = operations / layers + usize::from(index < operations % layers);
        let name = if layers == 1 {
            "Short strokes".to_owned()
        } else {
            format!("Short strokes {index}")
        };
        let layer = builder.paint(&name, strokes, |_| Shape {
            width: (2.0, 8.0),
            reach: 0.01,
            ..Shape::plain(6)
        });
        builder.document.layers.push(layer);
    }
    builder.document
}

/// 5: the largest canvas with a placed image, a masked selection move and a
/// group whose layers clip to its base.
fn image_mask_group() -> Result<Document, String> {
    let mut builder = Builder::new([4096, 4096]);
    let png = test_image(2048)?;
    let asset = AssetId(Sha256::digest(&png).into());
    builder.document.store.assets.insert(
        asset,
        Asset {
            size: [2048, 2048],
            png: png.into(),
        },
    );
    let mut image = builder.paint("Image", 0, |_| Shape::plain(1));
    set_paint(&mut image, |paint| {
        paint.ops.push(Op::PlaceImage {
            asset,
            // Qt's QTransform(1.8, 0.2, -0.2, 1.8, 400, 100) in row-major form.
            transform: Affine([1.8, -0.2, 400.0, 0.2, 1.8, 100.0]),
            sampling: Sampling::Smooth,
        })
    });
    builder.document.layers.push(image);

    let mut painted = builder.paint("Painted", 400, |stroke| Shape {
        reach: 0.2,
        ..Shape::mixed(stroke, 100)
    });
    let ellipse = ellipse_mask([800, 900, 1400, 1100]);
    builder.document.store.masks.insert(MaskId(0), ellipse);
    set_paint(&mut painted, |paint| {
        paint.ops.push(Op::TransformSelection {
            mask: MaskId(0),
            transform: Affine::translation(600.0, -300.0),
            sampling: Sampling::Smooth,
            keep_source: false,
        })
    });
    builder.document.layers.push(painted);

    let children = (0..3)
        .map(|index| {
            let mut layer = builder.paint(&format!("Group layer {index}"), 150, |stroke| Shape {
                width: (20.0, 120.0),
                ..Shape::mixed(stroke, 100)
            });
            set_paint(&mut layer, |paint| {
                paint.clip_to_below = index > 0;
                paint.blend = if index == 2 {
                    Blend::Screen
                } else {
                    Blend::Normal
                };
            });
            layer
        })
        .collect();
    let group = builder.group("Clipped group", Blend::Overlay, 1.0, children);
    builder.document.layers.push(group);
    Ok(builder.document)
}

fn ellipse_mask(bounds: [i32; 4]) -> Mask {
    let [_, _, width, height] = bounds;
    let row = Mask::row_bytes(width);
    let mut bits = vec![0u8; row * height as usize];
    let (radius_x, radius_y) = (f64::from(width) / 2.0, f64::from(height) / 2.0);
    for y in 0..height {
        for x in 0..width {
            let dx = (f64::from(x) + 0.5 - radius_x) / radius_x;
            let dy = (f64::from(y) + 0.5 - radius_y) / radius_y;
            if dx * dx + dy * dy <= 1.0 {
                bits[y as usize * row + x as usize / 8] |= 0x80 >> (x % 8);
            }
        }
    }
    Mask {
        bounds,
        bits: Arc::from(bits),
    }
}

/// Gradients and rings, as in tools/FixtureGenerator.cpp.
fn test_image(size: u32) -> Result<Vec<u8>, String> {
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    let half = f64::from(size) / 2.0;
    for y in 0..size {
        for x in 0..size {
            let ring = ((f64::from(x) - half).hypot(f64::from(y) - half) / 24.0) as u32;
            pixels.extend_from_slice(&[
                (x * 255 / size) as u8,
                (y * 255 / size) as u8,
                if ring % 2 == 1 { 220 } else { 40 },
                if ring.is_multiple_of(3) { 180 } else { 255 },
            ]);
        }
    }
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, size, size);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
    writer
        .write_image_data(&pixels)
        .map_err(|error| error.to_string())?;
    writer.finish().map_err(|error| error.to_string())?;
    Ok(png)
}
