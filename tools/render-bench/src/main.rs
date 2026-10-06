// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Compares CPU renderers on the same drawing document (M0 days 10-11).
//!
//! `run` times one renderer per process, so peak memory belongs to it alone,
//! and writes its images; `diff` compares the images of two runs.

mod backend;
mod scene;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use backend::{Backend, TinySkia, Vello};
use scene::{Document, LAYER_BLENDS};

const USAGE: &str = "usage:
  render-bench run --renderer tiny-skia|vello|vello-mt[=THREADS] [--size 2048] [--strokes 2000]
                   [--points 100] [--frames 30] [--segments 500] [--out DIR]
  render-bench diff A.rgba B.rgba [HEATMAP.png]";

struct Options {
    renderer: String,
    size: u32,
    strokes: usize,
    points: usize,
    frames: usize,
    segments: usize,
    out: PathBuf,
}

fn parse_run(mut args: impl Iterator<Item = String>) -> Option<Options> {
    let mut options = Options {
        renderer: String::new(),
        size: 2048,
        strokes: 2000,
        points: 100,
        frames: 30,
        segments: 500,
        out: std::env::temp_dir().join("render-bench"),
    };
    while let Some(arg) = args.next() {
        let value = args.next()?;
        match arg.as_str() {
            "--renderer" => options.renderer = value,
            "--size" => options.size = value.parse().ok()?,
            "--strokes" => options.strokes = value.parse().ok()?,
            "--points" => options.points = value.parse().ok()?,
            "--frames" => options.frames = value.parse().ok().filter(|&frames| frames > 0)?,
            "--segments" => options.segments = value.parse().ok()?,
            "--out" => options.out = PathBuf::from(value),
            _ => return None,
        }
    }
    (!options.renderer.is_empty()).then_some(options)
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let result = match args.next().as_deref() {
        Some("run") => match parse_run(args) {
            Some(options) => run(&options),
            None => Err(USAGE.to_owned()),
        },
        Some("diff") => match (args.next(), args.next()) {
            (Some(a), Some(b)) => diff(Path::new(&a), Path::new(&b), args.next().as_deref()),
            _ => Err(USAGE.to_owned()),
        },
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("render-bench: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(options: &Options) -> Result<(), String> {
    std::fs::create_dir_all(&options.out)
        .map_err(|error| format!("cannot create {}: {error}", options.out.display()))?;
    match options.renderer.as_str() {
        "tiny-skia" => measure(options, TinySkia::new(options.size)),
        "vello" => measure(options, Vello::new(options.size, 0)),
        "vello-mt" => {
            let threads = vello_cpu::RenderSettings::default().num_threads;
            measure(options, Vello::new(options.size, threads))
        }
        other => match other.strip_prefix("vello-mt=").map(str::parse) {
            Some(Ok(threads)) => measure(options, Vello::new(options.size, threads)),
            _ => Err(format!("unknown renderer {other}")),
        },
    }
}

fn milliseconds(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

fn report(stage: &str, mut values: Vec<f64>) {
    values.sort_by(f64::total_cmp);
    let rank = |fraction: f64| values[(fraction * (values.len() - 1) as f64).round() as usize];
    println!(
        "{stage:<10} n={:<4} p50={:>9.3}ms p95={:>9.3}ms max={:>9.3}ms total={:>10.1}ms",
        values.len(),
        rank(0.5),
        rank(0.95),
        values[values.len() - 1],
        values.iter().sum::<f64>(),
    );
}

fn measure<B: Backend>(options: &Options, mut backend: B) -> Result<(), String> {
    let document = Document::generate(options.size, options.strokes, options.points);
    println!(
        "renderer {} size {}^2 layers {} strokes {} points {} frames {}",
        options.renderer,
        options.size,
        LAYER_BLENDS.len(),
        document.stroke_count(),
        document.point_count(),
        options.frames,
    );
    let mut layers: Vec<B::Image> = (0..LAYER_BLENDS.len())
        .map(|_| backend.new_image(options.size))
        .collect();
    let mut out = backend.new_image(options.size);

    let (mut geometry, mut rasterize, mut composite) = (Vec::new(), Vec::new(), Vec::new());
    for frame in 0..options.frames {
        let started = Instant::now();
        let draws: Vec<_> = (0..layers.len())
            .map(|layer| document.layer_draws(layer, frame))
            .collect();
        geometry.push(milliseconds(started));

        let started = Instant::now();
        for (layer, draws) in layers.iter_mut().zip(&draws) {
            backend.rasterize(draws, layer);
        }
        rasterize.push(milliseconds(started));

        let started = Instant::now();
        backend.composite(&mut layers, &mut out);
        composite.push(milliseconds(started));

        if frame == 0 {
            save(options, "composite", &backend.bytes(&out))?;
        }
    }

    // Frame 0 again, as the source of the edits below.
    backend.rasterize(&document.layer_draws(0, 0), &mut layers[0]);
    let (angle, scale) = (12f32.to_radians(), 1.15f32);
    let (cos, sin) = (angle.cos() * scale, angle.sin() * scale);
    let centre = options.size as f32 * 0.5;
    let affine = [
        cos,
        sin,
        -sin,
        cos,
        centre - cos * centre + sin * centre,
        centre - sin * centre - cos * centre,
    ];
    let mut transform = Vec::new();
    for _ in 0..10 {
        let started = Instant::now();
        backend.transform(&mut layers[0], affine, &mut out);
        transform.push(milliseconds(started));
    }
    save(options, "transform", &backend.bytes(&out))?;

    let mut segment = Vec::new();
    for (draw, dirty) in document.live_segments(options.segments) {
        let started = Instant::now();
        backend.draw_segment(&draw, dirty, &mut layers[0]);
        segment.push(milliseconds(started));
    }
    save(options, "live", &backend.bytes(&layers[0]))?;

    report("geometry", geometry);
    report("rasterize", rasterize);
    report("composite", composite);
    report("transform", transform);
    if !segment.is_empty() {
        report("segment", segment);
    }
    let (working_set, private) = peak_memory();
    println!(
        "peak working set {:.1} MiB, peak private {:.1} MiB",
        working_set as f64 / 1048576.0,
        private as f64 / 1048576.0
    );
    Ok(())
}

/// Writes premultiplied RGBA8 as raw bytes for `diff` and as PNG to look at.
fn save(options: &Options, name: &str, bytes: &[u8]) -> Result<(), String> {
    let stem = options.out.join(format!("{}-{name}", options.renderer));
    let raw = stem.with_extension("rgba");
    std::fs::write(&raw, bytes)
        .map_err(|error| format!("cannot write {}: {error}", raw.display()))?;
    let size = tiny_skia::IntSize::from_wh(options.size, options.size).ok_or("invalid size")?;
    let pixmap = tiny_skia::Pixmap::from_vec(bytes.to_vec(), size).ok_or("invalid image")?;
    let png = stem.with_extension("png");
    pixmap
        .save_png(&png)
        .map_err(|error| format!("cannot write {}: {error}", png.display()))
}

fn peak_memory() -> (usize, usize) {
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::GetCurrentProcess;
    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    // SAFETY: valid out pointer of the stated size for this process.
    let _ = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
    (counters.PeakWorkingSetSize, counters.PeakPagefileUsage)
}

/// `heatmap`, if given, gets each pixel's largest channel difference times 4.
fn diff(a: &Path, b: &Path, heatmap: Option<&str>) -> Result<(), String> {
    let read = |path: &Path| {
        std::fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))
    };
    let (a, b) = (read(a)?, read(b)?);
    if a.len() != b.len() || a.len() % 4 != 0 {
        return Err("the images differ in size".to_owned());
    }
    let pixels = a.len() / 4;
    let (mut total, mut largest, mut over_2, mut over_8) = (0u64, 0u8, 0usize, 0usize);
    let mut map = Vec::with_capacity(a.len());
    for (pa, pb) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0) {
        let worst = pa
            .iter()
            .zip(pb)
            .map(|(x, y)| x.abs_diff(*y))
            .inspect(|&difference| total += u64::from(difference))
            .max()
            .unwrap_or(0);
        largest = largest.max(worst);
        let shade = worst.saturating_mul(4);
        map.extend_from_slice(&[shade, shade, shade, 255]);
        over_2 += usize::from(worst > 2);
        over_8 += usize::from(worst > 8);
    }
    println!(
        "pixels {pixels} mean channel difference {:.4} max {largest} pixels >2: {:.3}% >8: {:.3}%",
        total as f64 / a.len() as f64,
        over_2 as f64 * 100.0 / pixels as f64,
        over_8 as f64 * 100.0 / pixels as f64,
    );
    if let Some(path) = heatmap {
        let side = (pixels as f64).sqrt() as u32;
        let size = tiny_skia::IntSize::from_wh(side, side).ok_or("the images are not square")?;
        tiny_skia::Pixmap::from_vec(map, size)
            .ok_or("the images are not square")?
            .save_png(path)
            .map_err(|error| format!("cannot write {path}: {error}"))?;
    }
    Ok(())
}
