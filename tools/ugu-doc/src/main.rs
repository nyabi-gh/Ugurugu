// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Headless `.ugu2` tool.
//!
//! - `ugu-doc fixture <1-5> <out.ugu2>`: writes a comparison fixture with the
//!   same make-up as the 2.2.13 ones (docs/rust/m0-evidence.md section 2):
//!   the same canvases, layers, stroke and point counts and stroke shapes,
//!   from a different random sequence.
//! - `ugu-doc info <file.ugu2>`: reads, validates and summarises a file.
//! - `ugu-doc bench <file.ugu2> [rounds]`: times reading and saving it.
//! - `ugu-doc render <file.ugu2> [threads]`: times drawing every frame, by
//!   stage, on separate layer surfaces and as editing splits, with the peak
//!   working set. Other brushes are drawn as pens, and what M4 adds is left
//!   out; groups and blend modes not drawn yet are lifted out and made
//!   Normal, which keeps the amount of work close and says so.
//! - `ugu-doc pen-only <in.ugu2> <out.ugu2>`: makes brushes pens and leaves
//!   out what M4 adds, keeping groups, blend modes and clipping, so the app
//!   can open a fixture to measure with.

use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use sha2::{Digest, Sha256};
use ugu_core::document::{Document, Group, Layer, LayerId, LayerKind};
use ugu_core::ops::{Affine, AssetId, Blend, MaskId, Op, PaintLayer, Rgba8, Sampling, StrokeId};
use ugu_core::store::{Asset, Brush, BrushEngine, Mask, Point, Stroke};

const USAGE: &str = "usage: ugu-doc fixture <1-5> <out.ugu2> | info <file.ugu2> \
     | bench <file.ugu2> [rounds] | render <file.ugu2> [threads] | pen-only <in.ugu2> <out.ugu2>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["fixture", number, out] => fixture(number, Path::new(out)),
        ["info", file] => info(Path::new(file)),
        ["bench", file] => bench(Path::new(file), 10),
        ["bench", file, rounds] => rounds
            .parse()
            .map_err(|_| USAGE.to_owned())
            .and_then(|rounds| bench(Path::new(file), rounds)),
        ["render", file] => render(Path::new(file), 8),
        ["pen-only", from, to] => pen_only(Path::new(from), Path::new(to)),
        ["render", file, threads] => threads
            .parse()
            .map_err(|_| USAGE.to_owned())
            .and_then(|threads| render(Path::new(file), threads)),
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
    Ok(())
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
    let copy = path.with_extension("bench.ugu2");
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

/// Makes every brush a pen and drops what M4 adds (fills, images, selections
/// and stroke clips), keeping layers, groups, blend modes and clipping.
fn to_pens(document: &mut Document) {
    let mut changed = 0;
    for stroke in document.store.strokes.values_mut() {
        if stroke.brush.engine != BrushEngine::Line {
            stroke.brush.engine = BrushEngine::Line;
            changed += 1;
        }
    }
    let mut dropped = 0;
    each_paint(&mut document.layers, &mut |paint| {
        strip(&mut paint.ops, &mut dropped);
    });
    document.store.masks.clear();
    document.store.assets.clear();
    if changed + dropped > 0 {
        println!("{changed} brushes made pens, {dropped} fills, images or selections dropped");
    }
}

fn strip(ops: &mut Vec<Op>, dropped: &mut usize) {
    let before = ops.len();
    ops.retain(|op| {
        !matches!(
            op,
            Op::Fill { .. }
                | Op::PlaceImage { .. }
                | Op::TransformSelection { .. }
                | Op::ClearSelection { .. }
        )
    });
    *dropped += before - ops.len();
    for op in ops {
        match op {
            Op::Paint { clip, .. } | Op::Erase { clip, .. } => {
                *dropped += usize::from(clip.take().is_some());
            }
            Op::Isolated(section) => strip(&mut section.ops, dropped),
            _ => {}
        }
    }
}

fn each_paint(layers: &mut [Layer], visit: &mut impl FnMut(&mut PaintLayer)) {
    for layer in layers {
        match &mut layer.kind {
            LayerKind::Paint(paint) => visit(paint),
            LayerKind::Group(group) => each_paint(&mut group.children, visit),
        }
    }
}

/// Lifts layers out of groups and makes them Normal and unclipped, which the
/// renderer can draw until it draws groups (M3-3). The work stays close.
fn flatten(document: &mut Document) -> Result<(), String> {
    use ugu_render::document::check;

    let Err(unsupported) = check(document) else {
        return Ok(());
    };
    fn lift(layers: Vec<Layer>, into: &mut Vec<Layer>) {
        for mut layer in layers {
            match layer.kind {
                LayerKind::Group(group) => lift(group.children, into),
                LayerKind::Paint(ref mut paint) => {
                    paint.blend = Blend::Normal;
                    paint.clip_to_below = false;
                    into.push(layer);
                }
            }
        }
    }
    let mut flat = Vec::new();
    lift(std::mem::take(&mut document.layers), &mut flat);
    document.layers = flat;
    check(document).map_err(|left| format!("cannot draw {unsupported} or {left} yet"))?;
    println!("drawn approximately: groups lifted out, blend modes and clipping made Normal");
    Ok(())
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
fn render(path: &Path, threads: u16) -> Result<(), String> {
    use ugu_render::document::{DocumentRenderer, Purpose};

    let mut document = open(path)?;
    to_pens(&mut document);
    flatten(&mut document)?;
    let [width, height] = document.canvas.map(|edge| edge as u16);
    let mut pixmap = vello_cpu::Pixmap::new(width, height);
    let mut renderer = DocumentRenderer::new(threads);
    let mut times = Vec::new();
    let mut stages = [Vec::new(), Vec::new(), Vec::new()];
    let mut first = 0.0;
    for round in 0..3 {
        for frame in 0..i64::from(document.frames) {
            let started = Instant::now();
            renderer.render(&document, frame, Purpose::Display, &mut pixmap);
            let ms = started.elapsed().as_secs_f64() * 1000.0;
            if round == 0 && frame == 0 {
                first = ms;
            } else if round > 0 {
                times.push(ms);
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
    let [outlines, encode, rasterize] = &mut stages;
    println!("  outlines  {}", percentiles(outlines));
    println!("  encode    {}", percentiles(encode));
    println!("  rasterize {}", percentiles(rasterize));
    let whole_peak = peak_working_set_mib();
    separate_layers(&document, threads, &pixmap)?;
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

/// Draws each layer on its own surface and puts the surfaces together, the
/// shape M3 gives every render, and compares it with one Vello scene.
fn separate_layers(
    document: &Document,
    threads: u16,
    whole: &vello_cpu::Pixmap,
) -> Result<(), String> {
    use ugu_render::compose::src_over;
    use ugu_render::document::{DocumentRenderer, Purpose};

    let [width, height] = document.canvas.map(|edge| edge as u16);
    let mut renderer = DocumentRenderer::new(threads);
    let mut surfaces: Vec<(vello_cpu::Pixmap, u8)> = Vec::new();
    let mut single = document.clone();
    single.background = Rgba8([0, 0, 0, 0]);
    let mut layers_ms = Vec::new();
    let mut composite_ms = Vec::new();
    let mut out = vello_cpu::Pixmap::new(width, height);
    for frame in 0..i64::from(document.frames) {
        let started = Instant::now();
        surfaces.clear();
        for layer in document.layers.iter().filter(|layer| layer.visible) {
            let LayerKind::Paint(paint) = &layer.kind else {
                return Err("groups are drawn from M3-3 on".to_owned());
            };
            let mut own = layer.clone();
            if let LayerKind::Paint(paint) = &mut own.kind {
                paint.opacity = 1.0;
            }
            single.layers = vec![own];
            let mut surface = vello_cpu::Pixmap::new(width, height);
            renderer.render(&single, frame, Purpose::Display, &mut surface);
            let opacity = (paint.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
            surfaces.push((surface, opacity));
        }
        layers_ms.push(started.elapsed().as_secs_f64() * 1000.0);

        let started = Instant::now();
        let Rgba8(background) = document.background;
        let background = ugu_render::compose::premultiplied(background);
        let rows = usize::from(height).div_ceil(usize::from(threads.max(1)));
        let row_bytes = usize::from(width) * 4;
        std::thread::scope(|scope| {
            for (band, target) in out
                .data_as_u8_slice_mut()
                .chunks_mut(rows * row_bytes)
                .enumerate()
            {
                let surfaces = &surfaces;
                scope.spawn(move || {
                    let start = band * rows * row_bytes;
                    for (at, pixel) in target.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                        pixel.copy_from_slice(&background);
                        let from = start + at * 4;
                        for (surface, opacity) in surfaces {
                            let source: [u8; 4] = surface.data_as_u8_slice()[from..from + 4]
                                .try_into()
                                .expect("4 bytes");
                            if source[3] == 0 {
                                continue;
                            }
                            let faded = source.map(|value| {
                                ((u16::from(value) * u16::from(*opacity) + 255) >> 8) as u8
                            });
                            src_over(pixel, &faded);
                        }
                    }
                });
            }
        });
        composite_ms.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    let most = out
        .data_as_u8_slice()
        .iter()
        .zip(whole.data_as_u8_slice())
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0);
    println!(
        "separate surfaces ({} layers): render {}, composite {}; last frame differs from one scene by up to {most}",
        surfaces.len(),
        percentiles(&mut layers_ms),
        percentiles(&mut composite_ms)
    );
    Ok(())
}

/// Times the M2 editing split around each top-level layer, frame 0.
fn splits(document: &Document, threads: u16) {
    let mut renderer = ugu_render::document::DocumentRenderer::new(threads);
    let mut times = Vec::new();
    for layer in &document.layers {
        let started = Instant::now();
        if renderer.split(document, layer.id, 0).is_some() {
            times.push(started.elapsed().as_secs_f64() * 1000.0);
        }
    }
    if !times.is_empty() {
        println!("split per layer: {}", percentiles(&mut times));
    }
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
    let mut builder = Builder::new([2048, 2048]);
    let limit = ugu_core::document::limits::OPERATIONS;
    let layer = builder.paint("Short strokes", limit, |_| Shape {
        width: (2.0, 8.0),
        reach: 0.01,
        ..Shape::plain(6)
    });
    builder.document.layers.push(layer);
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
