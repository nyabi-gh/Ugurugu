// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Exporting. Each export runs on a thread of its own, so saves and
//! recovery writes are not held up behind it, and editing goes on while it
//! runs. It renders the document as it was when the export started.
//! Cancelling answers at once; the thread stops at its next step and never
//! puts its file in place after that, so the target stays as it was.
//!
//! An animation's frames are drawn one after another by the render worker,
//! after the canvas's own work, shrunk at once so that only one frame at the
//! document's size is held, and handed through a short queue to the export
//! thread. A GIF takes their colours, then is coded on several threads; a
//! WebP codes each frame as it comes.

use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::sync_channel;
use std::thread::JoinHandle;

use ugu_core::document::Document;
use ugu_core::ops::Rgba8;
use ugu_io::image::Format;
use vello_cpu::peniko::ImageAlphaType;

use crate::cache::Renders;

/// Long edges an animation can be exported at below the document's own:
/// animated images are mostly a few hundred pixels across.
const EDGES: [u32; 6] = [2048, 1024, 768, 512, 384, 256];
/// The long edge offered until another is chosen.
pub const FIRST_EDGE: u32 = 512;
/// Whole enlargements offered, each pixel a block (m5-plan M5-17), while
/// the long edge stays within `MOST_ENLARGED`.
const ENLARGEMENTS: [u32; 3] = [8, 4, 2];
const MOST_ENLARGED: u32 = 4096;

/// Frames drawn ahead of the export thread.
const QUEUE: usize = 2;
/// Frames drawn ahead of each WebP run.
const RUN_QUEUE: usize = 1;

/// Whole frames libwebp holds while coding one, measured.
const WEBP_FRAMES: u64 = 12;
/// A losslessly coded frame is at most about this much smaller, measured.
const WEBP_FILE: u64 = 2;

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Done,
    Cancelled,
    Failed(String),
}

/// The animated formats, as 2.2.13 offers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Animation {
    Gif,
    WebP,
}

/// What is exported.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Job {
    /// One frame at the document's size, as the file's extension says.
    Still { frame: i64 },
    /// Every frame.
    Animation {
        format: Animation,
        size: [u32; 2],
        keep_transparency: bool,
        /// Threads coding it, or WebP runs.
        threads: usize,
    },
}

/// The sizes offered, largest first: the whole enlargements that fit, the
/// document's own, then each of `EDGES` below it, keeping its shape.
pub fn sizes(canvas: [u32; 2]) -> Vec<[u32; 2]> {
    let long = canvas[0].max(canvas[1]);
    ENLARGEMENTS
        .iter()
        .filter(|&&times| long * times <= MOST_ENLARGED)
        .map(|&times| canvas.map(|side| side * times))
        .chain(std::iter::once(canvas))
        .chain(
            EDGES
                .iter()
                .filter(|&&edge| edge < long)
                .map(|&edge| fit(canvas, edge)),
        )
        .collect()
}

/// `canvas` shrunk to a long edge of `edge`.
pub fn fit(canvas: [u32; 2], edge: u32) -> [u32; 2] {
    let long = u64::from(canvas[0].max(canvas[1]));
    canvas.map(|side| ((u64::from(side) * u64::from(edge) + long / 2) / long).max(1) as u32)
}

/// Of `sizes` of `canvas`, the largest no larger than the document whose
/// long edge is no longer than `edge`, else the smallest. An enlargement is
/// only ever chosen by hand.
pub fn nearest(canvas: [u32; 2], sizes: &[[u32; 2]], edge: u32) -> usize {
    let own = original(canvas, sizes);
    sizes[own..]
        .iter()
        .position(|size| size[0].max(size[1]) <= edge)
        .map_or(sizes.len() - 1, |index| own + index)
}

/// Where the document's own size is in its `sizes`.
pub fn original(canvas: [u32; 2], sizes: &[[u32; 2]]) -> usize {
    sizes.iter().position(|&size| size == canvas).unwrap_or(0)
}

/// Premultiplied RGBA8 `pixels` of `from` made `to`: shrunk by averaging,
/// or enlarged a whole number of times with each pixel a block.
pub fn resized(pixels: Vec<u8>, from: [u32; 2], to: [u32; 2]) -> Vec<u8> {
    if to[0] > from[0] {
        ugu_io::image::enlarge(&pixels, from, to[0] / from[0])
    } else {
        ugu_io::image::shrink(pixels, from, to)
    }
}

/// About how much memory an animation of `frames` at `size` from a document
/// of `canvas` takes: the frame being drawn at the document's size and
/// being shrunk, then for a GIF the frames on their way, two bytes a pixel
/// of every frame between its two passes and what each coding thread holds,
/// and for a WebP each run's frames on their way, what its encoder holds
/// (WEBP_FRAMES frames) and the file so far (a WEBP_FILE-th of each frame).
pub fn animation_bytes(
    format: Animation,
    canvas: [u32; 2],
    size: [u32; 2],
    frames: u32,
    threads: usize,
) -> u64 {
    let pixels = |[width, height]: [u32; 2]| u64::from(width) * u64::from(height);
    let drawn = pixels(canvas) * 4 + pixels(size) * 4;
    let coding = match format {
        Animation::Gif => {
            pixels(size) * 4 * (QUEUE as u64 + 2)
                + pixels(size) * 2 * u64::from(frames)
                + threads as u64 * (4 * 1024 * 1024 + pixels(size) * 2)
        }
        Animation::WebP => {
            let runs = ugu_io::webp::runs(frames as usize, threads).len() as u64;
            runs * pixels(size) * 4 * (RUN_QUEUE as u64 + 2 + WEBP_FRAMES)
                + pixels(size) * 4 * u64::from(frames) / WEBP_FILE
        }
    };
    drawn + coding
}

pub fn threads() -> usize {
    std::thread::available_parallelism().map_or(1, usize::from)
}

/// The most threads, up to `threads()`, an animation can be coded on within
/// `budget`, as a WebP run takes far more than its frame; `None` when even
/// one would not fit.
pub fn threads_within(
    format: Animation,
    canvas: [u32; 2],
    size: [u32; 2],
    frames: u32,
    budget: u64,
) -> Option<usize> {
    (1..=threads())
        .rev()
        .find(|&threads| animation_bytes(format, canvas, size, frames, threads) <= budget)
}

/// An export under way.
pub struct Exporting {
    /// Tells this export's answer from an earlier, cancelled one's.
    pub number: u64,
    cancel: Arc<AtomicBool>,
    /// Steps done, of `steps`.
    done: Arc<AtomicUsize>,
    pub steps: usize,
    thread: JoinHandle<()>,
}

impl Exporting {
    /// Exports `document` to `path` as `job` says, and hands the outcome to
    /// `done`.
    pub fn start(
        number: u64,
        document: Arc<Document>,
        job: Job,
        path: PathBuf,
        renders: Renders,
        finished: impl FnOnce(Outcome) + Send + 'static,
    ) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicUsize::new(0));
        let steps = match job {
            Job::Still { .. } => 1,
            // Taking each frame's colours, then coding it.
            Job::Animation {
                format: Animation::Gif,
                ..
            } => document.frames as usize * 2,
            Job::Animation {
                format: Animation::WebP,
                ..
            } => document.frames as usize,
        };
        let thread = std::thread::Builder::new()
            .name("export".to_owned())
            .spawn({
                let cancel = cancel.clone();
                let done = done.clone();
                move || {
                    let started = std::time::Instant::now();
                    let outcome = match job {
                        Job::Still { frame } => still(document, frame, &path, &renders, &cancel),
                        Job::Animation {
                            format,
                            size,
                            keep_transparency,
                            threads,
                        } => animation(
                            document,
                            format,
                            size,
                            keep_transparency,
                            threads,
                            &path,
                            &renders,
                            &cancel,
                            &done,
                        ),
                    };
                    tracing::info!(
                        ms = started.elapsed().as_secs_f64() * 1000.0,
                        ?job,
                        ?outcome,
                        "exported"
                    );
                    finished(outcome);
                }
            })
            .expect("cannot start the export thread");
        Self {
            number,
            cancel,
            done,
            steps,
            thread,
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// How many of `steps` are done.
    pub fn done(&self) -> usize {
        self.done.load(Ordering::Relaxed)
    }

    pub fn is_finished(&self) -> bool {
        self.thread.is_finished()
    }

    /// Waits for the thread, which removes its unfinished file.
    pub fn join(self) {
        let _ = self.thread.join();
    }
}

/// The file put in place only if the export was not cancelled meanwhile.
fn replace_unless(cancel: &AtomicBool) -> impl Fn(&Path, &Path) -> std::io::Result<()> + '_ {
    move |written, target| {
        if cancel.load(Ordering::Relaxed) {
            return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
        }
        ugu_win::file::replace_file(written, target)
    }
}

/// Renders `frame` without reference layers at the document's size, on the
/// render worker after the canvas's own work, and writes it to `path`, in
/// the format its extension names (PNG for any other).
pub fn still(
    document: Arc<Document>,
    frame: i64,
    path: &Path,
    renders: &Renders,
    cancel: &AtomicBool,
) -> Outcome {
    let cancelled = || cancel.load(Ordering::Relaxed);
    let size = document.canvas;
    let Some(pixmap) = renders.export(document, frame) else {
        return Outcome::Failed("the render worker has stopped".to_owned());
    };
    if cancelled() {
        return Outcome::Cancelled;
    }
    let format = Format::of(path).unwrap_or(Format::Png);
    match ugu_io::image::export(
        pixmap.data_as_u8_slice(),
        size,
        format,
        path,
        &replace_unless(cancel),
    ) {
        Ok(()) => Outcome::Done,
        Err(_) if cancelled() => Outcome::Cancelled,
        Err(error) => Outcome::Failed(error.to_string()),
    }
}

/// Premultiplied frame pixels as the encoders take them: straight, rounded as
/// Qt does, or put over white when transparency is not kept, as 2.2.13.
fn straight(pixels: &[u8], keep_transparency: bool) -> Vec<u8> {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[red, green, blue, alpha]| {
            if keep_transparency {
                Rgba8::from_premultiplied([red, green, blue, alpha]).0
            } else {
                let paper = 255 - alpha;
                [
                    red.saturating_add(paper),
                    green.saturating_add(paper),
                    blue.saturating_add(paper),
                    255,
                ]
            }
        })
        .collect()
}

/// `frame` of `document` drawn on the render worker, after the canvas's own
/// work, and shrunk to `size`; `None` when the worker has stopped.
fn drawn(
    renders: &Renders,
    document: &Arc<Document>,
    frame: i64,
    size: [u32; 2],
) -> Option<Vec<u8>> {
    let pixmap = renders.export(document.clone(), frame)?;
    Some(resized(
        pixmap.take_rgba8(ImageAlphaType::AlphaPremultiplied),
        document.canvas,
        size,
    ))
}

/// Draws every frame of `document` and hands each to `take` at `size` as
/// the encoders take it.
fn each_frame(
    document: &Arc<Document>,
    size: [u32; 2],
    keep_transparency: bool,
    renders: &Renders,
    cancel: &AtomicBool,
    mut take: impl FnMut(usize, &[u8]) -> Result<(), Outcome>,
) -> Result<(), Outcome> {
    let cancelled = || cancel.load(Ordering::Relaxed);
    let frames = document.frames;
    let (to_export, shrunk) = sync_channel(QUEUE);
    let taken = std::thread::scope(|scope| {
        scope.spawn(|| {
            for frame in 0..frames {
                if cancelled() {
                    break;
                }
                let Some(frame) = drawn(renders, document, i64::from(frame), size) else {
                    break;
                };
                if to_export.send(frame).is_err() {
                    break;
                }
            }
            drop(to_export);
        });
        let mut taken = 0;
        for frame in shrunk {
            if cancelled() {
                // The drawing thread stops at its next frame.
                return Err(Outcome::Cancelled);
            }
            take(taken, &straight(&frame, keep_transparency))?;
            taken += 1;
        }
        Ok(taken)
    })?;
    if cancelled() {
        return Err(Outcome::Cancelled);
    }
    if taken != frames as usize {
        return Err(Outcome::Failed("the render worker has stopped".to_owned()));
    }
    Ok(())
}

/// What became of writing the file.
fn written(written: Result<(), ugu_io::save::SaveError>, cancel: &AtomicBool) -> Outcome {
    match written {
        Ok(()) => Outcome::Done,
        Err(_) if cancel.load(Ordering::Relaxed) => Outcome::Cancelled,
        Err(error) => Outcome::Failed(error.to_string()),
    }
}

/// Every frame of `document` at `size` as an animated GIF or WebP at
/// `path`.
#[allow(clippy::too_many_arguments)]
pub fn animation(
    document: Arc<Document>,
    format: Animation,
    size: [u32; 2],
    keep_transparency: bool,
    threads: usize,
    path: &Path,
    renders: &Renders,
    cancel: &AtomicBool,
    done: &AtomicUsize,
) -> Outcome {
    let options = Options {
        size,
        keep_transparency,
        threads,
    };
    let outcome = match format {
        Animation::Gif => gif(&document, options, path, renders, cancel, done),
        Animation::WebP => webp(&document, options, path, renders, cancel, done),
    };
    outcome.unwrap_or_else(|outcome| outcome)
}

/// How an animation's frames are made and coded.
#[derive(Clone, Copy)]
struct Options {
    size: [u32; 2],
    keep_transparency: bool,
    /// Threads coding it, or WebP runs.
    threads: usize,
}

/// The GIF's colours from every frame, then its frames coded on several
/// threads.
fn gif(
    document: &Arc<Document>,
    Options {
        size,
        keep_transparency,
        threads,
    }: Options,
    path: &Path,
    renders: &Renders,
    cancel: &AtomicBool,
    done: &AtomicUsize,
) -> Result<Outcome, Outcome> {
    let cancelled = || cancel.load(Ordering::Relaxed);
    let delays: Vec<u16> =
        ugu_io::animation::delays(document.frames, f64::from(document.frames_per_second), 100)
            .into_iter()
            .map(|delay| delay.min(u32::from(u16::MAX)) as u16)
            .collect();
    let mut quantizer = ugu_io::gif::Quantizer::new(size);
    each_frame(
        document,
        size,
        keep_transparency,
        renders,
        cancel,
        |index, frame| {
            quantizer.add(frame);
            done.store(index + 1, Ordering::Relaxed);
            Ok(())
        },
    )?;
    let quantized = quantizer.finish();
    let taken = delays.len();
    let result = ugu_io::save::replace_with(path, &replace_unless(cancel), |file, _| {
        quantized
            .write(
                BufWriter::new(file),
                &delays,
                threads,
                &cancelled,
                &mut |coded| done.store(taken + coded, Ordering::Relaxed),
            )
            .map_err(ugu_io::save::SaveError::from)
    });
    Ok(written(result, cancel))
}

/// The frames cut into runs coded at once (`ugu_io::webp`), each run fed
/// its frames in turn by one drawing thread, then joined.
fn webp(
    document: &Arc<Document>,
    Options {
        size,
        keep_transparency,
        threads,
    }: Options,
    path: &Path,
    renders: &Renders,
    cancel: &AtomicBool,
    done: &AtomicUsize,
) -> Result<Outcome, Outcome> {
    let cancelled = || cancel.load(Ordering::Relaxed);
    let failed = |error: std::io::Error| {
        if cancelled() {
            Outcome::Cancelled
        } else {
            Outcome::Failed(error.to_string())
        }
    };
    let frames = document.frames as usize;
    let delays =
        ugu_io::animation::delays(document.frames, f64::from(document.frames_per_second), 1000);
    let runs = ugu_io::webp::runs(frames, threads);
    let coded = std::thread::scope(|scope| {
        let (senders, receivers): (Vec<_>, Vec<_>) =
            runs.iter().map(|_| sync_channel(RUN_QUEUE)).unzip();
        let runs = &runs;
        scope.spawn(move || {
            let longest = runs.iter().map(|run| run.len()).max().unwrap_or(0);
            'drawing: for step in 0..longest {
                for (run, sender) in runs.iter().zip(&senders) {
                    let index = run.start + step;
                    if index >= run.end {
                        continue;
                    }
                    if cancelled() {
                        break 'drawing;
                    }
                    let Some(frame) = drawn(renders, document, index as i64, size) else {
                        break 'drawing;
                    };
                    if sender.send(frame).is_err() {
                        break 'drawing;
                    }
                }
            }
            // Ends the runs below.
            drop(senders);
        });
        let coders: Vec<_> = runs
            .iter()
            .zip(receivers)
            .map(|(range, shrunk)| {
                let delays = &delays;
                scope.spawn(move || {
                    let mut run = ugu_io::webp::Run::new(size, range.start == 0, &cancelled)
                        .map_err(failed)?;
                    let mut taken = 0;
                    for frame in shrunk {
                        run.add(
                            &straight(&frame, keep_transparency),
                            delays[range.start + taken],
                        )
                        .map_err(failed)?;
                        taken += 1;
                        done.fetch_add(1, Ordering::Relaxed);
                    }
                    if taken != range.len() {
                        return Err(if cancelled() {
                            Outcome::Cancelled
                        } else {
                            Outcome::Failed("the render worker has stopped".to_owned())
                        });
                    }
                    run.finish().map_err(failed)
                })
            })
            .collect();
        coders
            .into_iter()
            .map(|coder| coder.join().expect("a WebP run panicked"))
            .collect::<Result<Vec<_>, Outcome>>()
    })?;
    let result = ugu_io::save::replace_with(path, &replace_unless(cancel), |file, _| {
        ugu_io::webp::join(coded, size, BufWriter::new(file)).map_err(ugu_io::save::SaveError::from)
    });
    Ok(written(result, cancel))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::CacheWorker;
    use ugu_core::ops::Rgba8;

    fn folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("ugurugu-export-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn document(background: [u8; 4]) -> Arc<Document> {
        let mut document = Document::new([40, 30]);
        document.background = Rgba8(background);
        Arc::new(document)
    }

    fn export(document: Arc<Document>, path: &Path, cancel: bool) -> Outcome {
        let worker = CacheWorker::start(|_| {});
        still(
            document,
            0,
            path,
            &worker.renders(),
            &AtomicBool::new(cancel),
        )
    }

    #[test]
    fn a_frame_is_written_as_its_extension_says() {
        let folder = folder("formats");
        let png = folder.join("frame.png");
        assert_eq!(
            export(document([200, 100, 50, 255]), &png, false),
            Outcome::Done
        );
        let decoded = image::open(&png).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), (40, 30));
        assert_eq!(decoded.get_pixel(5, 5).0, [200, 100, 50, 255]);

        let jpeg = folder.join("frame.JPG");
        assert_eq!(
            export(document([200, 100, 50, 255]), &jpeg, false),
            Outcome::Done
        );
        let pixel = image::open(&jpeg).unwrap().to_rgb8().get_pixel(5, 5).0;
        for (got, want) in pixel.into_iter().zip([200, 100, 50]) {
            assert!(got.abs_diff(want) <= 3, "{pixel:?}");
        }
        // A transparent background is white paper in a JPEG.
        assert_eq!(export(document([0, 0, 0, 0]), &jpeg, false), Outcome::Done);
        let pixel = image::open(&jpeg).unwrap().to_rgb8().get_pixel(5, 5).0;
        assert!(pixel.iter().all(|&channel| channel >= 252), "{pixel:?}");
    }

    #[test]
    fn a_cancelled_export_leaves_the_target_as_it_was() {
        let folder = folder("cancelled");
        let target = folder.join("frame.png");
        std::fs::write(&target, b"the old file").unwrap();
        assert_eq!(
            export(document([200, 100, 50, 255]), &target, true),
            Outcome::Cancelled
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"the old file");
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);
    }

    fn animated(
        format: Animation,
        background: [u8; 4],
        size: [u32; 2],
        keep: bool,
        path: &Path,
        cancel: bool,
    ) -> Outcome {
        let mut document = Document::new([40, 30]);
        document.background = Rgba8(background);
        document.frames = 3;
        document.frames_per_second = 12.0;
        let worker = CacheWorker::start(|_| {});
        animation(
            Arc::new(document),
            format,
            size,
            keep,
            threads(),
            path,
            &worker.renders(),
            &AtomicBool::new(cancel),
            &AtomicUsize::new(0),
        )
    }

    fn decoded(path: &Path) -> Vec<(u32, image::RgbaImage)> {
        let file = std::io::BufReader::new(std::fs::File::open(path).unwrap());
        let frames = if path
            .extension()
            .is_some_and(|extension| extension == "webp")
        {
            image::AnimationDecoder::into_frames(
                image::codecs::webp::WebPDecoder::new(file).unwrap(),
            )
        } else {
            image::AnimationDecoder::into_frames(image::codecs::gif::GifDecoder::new(file).unwrap())
        };
        let frames: Vec<_> = frames
            .map(|frame| {
                let frame = frame.unwrap();
                let (numerator, denominator) = frame.delay().numer_denom_ms();
                (numerator / denominator, frame.into_buffer())
            })
            .collect();
        if frames.is_empty() {
            // A still image, without a duration.
            return vec![(0, image::open(path).unwrap().to_rgba8())];
        }
        frames
    }

    #[test]
    fn an_animation_is_every_frame_at_the_size_chosen() {
        let folder = folder("animation");
        let size = fit([40, 30], 20);
        assert_eq!(size, [20, 15]);
        for (format, name, delays) in [
            // 1/12 s each, in hundredths adding up: 8, 9, 8.
            (Animation::Gif, "motion.gif", &[80, 90, 80][..]),
            // libwebp writes frames that never change as a still image, as
            // 2.2.13's file is.
            (Animation::WebP, "motion.webp", &[0]),
        ] {
            let path = folder.join(name);
            let animated =
                |background, keep| animated(format, background, size, keep, &path, false);
            assert_eq!(animated([200, 100, 50, 255], true), Outcome::Done);
            let frames = decoded(&path);
            assert_eq!(
                frames.iter().map(|(delay, _)| *delay).collect::<Vec<_>>(),
                delays,
                "{name}"
            );
            for (_, frame) in &frames {
                assert_eq!(frame.dimensions(), (20, 15));
                assert!(frame.pixels().all(|pixel| pixel.0 == [200, 100, 50, 255]));
            }
            // A transparent background stays so, or becomes white paper.
            assert_eq!(animated([0, 0, 0, 0], true), Outcome::Done);
            assert!(decoded(&path)[0].1.pixels().all(|pixel| pixel.0[3] == 0));
            assert_eq!(animated([0, 0, 0, 0], false), Outcome::Done);
            assert!(
                decoded(&path)[0]
                    .1
                    .pixels()
                    .all(|pixel| pixel.0 == [255; 4]),
                "{name}"
            );
        }
    }

    /// A small document of pixel strokes in three colours on white.
    fn pixel_art() -> Document {
        use ugu_core::document::LayerKind;
        use ugu_core::ops::{Op, StrokeId};
        use ugu_core::store::{Point, Stroke};
        let mut document = Document::new([16, 12]);
        document.background = Rgba8([255; 4]);
        document.frames = 2;
        let brush = ugu_core::brush::find("pixel-pencil").unwrap().brush;
        let mut ops = Vec::new();
        for (index, (color, from, to)) in [
            ([200, 30, 40, 255], [1.5, 1.5], [14.5, 9.5]),
            ([20, 120, 220, 255], [2.5, 10.5], [12.5, 3.5]),
            ([250, 200, 0, 255], [7.5, 0.5], [7.5, 11.5]),
        ]
        .into_iter()
        .enumerate()
        {
            let id = StrokeId(index as u32);
            let points: Vec<Point> = [from, to]
                .into_iter()
                .map(|[x, y]| Point {
                    x,
                    y,
                    pressure: 1.0,
                })
                .collect();
            document.store.strokes.insert(
                id,
                Stroke {
                    points: points.into(),
                    color: Rgba8(color),
                    width: 1.0,
                    brush,
                    seed: index as u64,
                },
            );
            ops.push(Op::Paint {
                stroke: id,
                clip: None,
            });
        }
        if let LayerKind::Paint(paint) = &mut document.layers[0].kind {
            paint.ops = ops;
        }
        document
    }

    #[test]
    fn enlarged_eight_times_each_pixel_is_an_eight_by_eight_block() {
        let folder = folder("enlarged");
        let worker = CacheWorker::start(|_| {});
        let document = Arc::new(pixel_art());
        let export = |format, size, name: &str| {
            let path = folder.join(name);
            let outcome = animation(
                document.clone(),
                format,
                size,
                true,
                threads(),
                &path,
                &worker.renders(),
                &AtomicBool::new(false),
                &AtomicUsize::new(0),
            );
            assert_eq!(outcome, Outcome::Done);
            decoded(&path)
        };
        for (format, extension) in [(Animation::Gif, "gif"), (Animation::WebP, "webp")] {
            let original = export(format, [16, 12], &format!("one.{extension}"));
            let enlarged = export(format, [128, 96], &format!("eight.{extension}"));
            assert_eq!(original.len(), enlarged.len());
            for ((_, small), (_, large)) in original.iter().zip(&enlarged) {
                assert_eq!(large.dimensions(), (128, 96));
                for (x, y, pixel) in large.enumerate_pixels() {
                    assert_eq!(
                        *pixel,
                        *small.get_pixel(x / 8, y / 8),
                        "{extension} {x}, {y}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_cancelled_animation_leaves_the_target_as_it_was() {
        for (format, name) in [(Animation::Gif, "gif"), (Animation::WebP, "webp")] {
            let folder = folder(&format!("{name}-cancelled"));
            let target = folder.join(format!("motion.{name}"));
            std::fs::write(&target, b"the old file").unwrap();
            assert_eq!(
                animated(format, [200, 100, 50, 255], [40, 30], true, &target, true),
                Outcome::Cancelled
            );
            assert_eq!(std::fs::read(&target).unwrap(), b"the old file");
            assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);
        }
    }

    #[test]
    fn the_estimate_counts_what_the_encoders_keep_and_what_is_on_its_way() {
        let canvas = [2048, 2048];
        let half = fit(canvas, 1024);
        let mib = |bytes: u64| bytes / (1024 * 1024);
        let gif = |size, threads| animation_bytes(Animation::Gif, canvas, size, 30, threads);
        // The frame being drawn and shrunk, the frames on their way, two
        // bytes a pixel of 30 frames and the coding thread.
        assert_eq!(mib(gif(half, 1)), 16 + 4 + 4 * 4 + 60 + 4 + 2);
        assert!(gif(canvas, 8) > gif(half, 8));
        // Seven runs, each with its frames on their way and its encoder's
        // frames, and part of every frame's pixels.
        assert_eq!(
            mib(animation_bytes(Animation::WebP, canvas, half, 30, 8)),
            16 + 4 + 7 * 4 * (3 + WEBP_FRAMES) + 4 * 30 / WEBP_FILE
        );
        // Fewer WebP runs within a smaller budget, none when one does not fit.
        let webp = |budget| threads_within(Animation::WebP, canvas, canvas, 30, budget);
        let one = animation_bytes(Animation::WebP, canvas, canvas, 30, 1);
        let two = animation_bytes(Animation::WebP, canvas, canvas, 30, 2);
        assert_eq!(webp(two), Some(2.min(threads())));
        assert_eq!(webp(two - 1), Some(1));
        assert_eq!(webp(one - 1), None);
    }

    #[test]
    fn the_sizes_offered_are_the_document_s_and_smaller_long_edges() {
        assert_eq!(
            sizes([4096, 2048]),
            [
                [4096, 2048],
                [2048, 1024],
                [1024, 512],
                [768, 384],
                [512, 256],
                [384, 192],
                [256, 128]
            ]
        );
        assert_eq!(
            sizes([600, 300]),
            [
                [2400, 1200],
                [1200, 600],
                [600, 300],
                [512, 256],
                [384, 192],
                [256, 128]
            ]
        );
        assert_eq!(
            sizes([200, 100]),
            [[1600, 800], [800, 400], [400, 200], [200, 100]]
        );
        assert_eq!(
            sizes([1024, 32])[..3],
            [[4096, 128], [2048, 64], [1024, 32]]
        );
        assert_eq!(fit([10, 3], 5), [5, 2]);
        assert_eq!(fit([3000, 1], 256), [256, 1]);
        let large = sizes([4096, 4096]);
        let nearest_large = |edge| nearest([4096, 4096], &large, edge);
        assert_eq!(large[nearest_large(FIRST_EDGE)], [512, 512]);
        assert_eq!(large[nearest_large(700)], [512, 512]);
        assert_eq!(nearest_large(100), large.len() - 1);
        let small = sizes([32, 16]);
        // An enlargement only by hand, even after one was chosen.
        assert_eq!(small[nearest([32, 16], &small, FIRST_EDGE)], [32, 16]);
        assert_eq!(small[nearest([32, 16], &small, 256)], [32, 16]);
    }

    #[test]
    fn an_enlargement_makes_each_pixel_a_block() {
        let pixels = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let enlarged = resized(pixels, [2, 1], [6, 3]);
        let left = [1, 2, 3, 4].repeat(3);
        let right = [5, 6, 7, 8].repeat(3);
        let row = [left, right].concat();
        assert_eq!(enlarged, row.repeat(3));
    }

    #[test]
    fn a_failed_export_says_why() {
        let target = folder("failed").join("no such folder").join("frame.png");
        assert!(matches!(
            export(document([200, 100, 50, 255]), &target, false),
            Outcome::Failed(_)
        ));
    }
}
