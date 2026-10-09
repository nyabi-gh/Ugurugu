// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Exporting. Each export runs on a thread of its own, so saves and
//! recovery writes are not held up behind it, and editing goes on while it
//! runs. It renders the document as it was when the export started.
//! Cancelling answers at once; the thread stops at its next step and never
//! puts its file in place after that, so the target stays as it was.
//!
//! An animation's frames are drawn one after another by the render worker,
//! after the canvas's own work, and handed through a short queue to the
//! export thread, which shrinks them and takes their colours; the GIF is
//! then coded on several threads.

use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::sync_channel;
use std::thread::JoinHandle;

use ugu_core::document::Document;
use ugu_core::ops::Rgba8;
use ugu_io::image::Format;

use crate::cache::Renders;

/// The sizes an animation can be exported at, as 2.2.13 offers them.
pub const SCALES: [u32; 5] = [100, 75, 50, 33, 25];

/// Frames drawn ahead of the export thread.
const QUEUE: usize = 2;

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Done,
    Cancelled,
    Failed(String),
}

/// What is exported.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Job {
    /// One frame at the document's size, as the file's extension says.
    Still { frame: i64 },
    /// Every frame as an animated GIF.
    Gif {
        size: [u32; 2],
        keep_transparency: bool,
    },
}

/// `canvas` at `percent`, as 2.2.13 works it out.
pub fn scaled(canvas: [u32; 2], percent: u32) -> [u32; 2] {
    if percent >= 100 {
        return canvas;
    }
    canvas.map(|edge| (edge * percent / 100).max(1))
}

/// About how much memory a GIF of `frames` at `size` from a document of
/// `canvas` takes: two bytes a pixel of every frame between the two passes,
/// the frames on their way at the document's size and shrunk, and what each
/// coding thread holds.
pub fn gif_bytes(canvas: [u32; 2], size: [u32; 2], frames: u32, threads: usize) -> u64 {
    let pixels = |[width, height]: [u32; 2]| u64::from(width) * u64::from(height);
    let keys = pixels(size) * 2 * u64::from(frames);
    let drawn = pixels(canvas) * 4 * (QUEUE as u64 + 2);
    let shrunk = pixels(size) * 4 * 2;
    let coding = threads as u64 * (4 * 1024 * 1024 + pixels(size) * 2);
    keys + drawn + shrunk + coding
}

pub fn threads() -> usize {
    std::thread::available_parallelism().map_or(1, usize::from)
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
        // Drawing each frame, then coding it.
        let steps = match job {
            Job::Still { .. } => 1,
            Job::Gif { .. } => document.frames as usize * 2,
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
                        Job::Gif {
                            size,
                            keep_transparency,
                        } => gif(
                            document,
                            size,
                            keep_transparency,
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

/// Premultiplied frame pixels as the GIF takes them: straight, rounded as
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

/// Every frame of `document` at `size` as an animated GIF at `path`.
pub fn gif(
    document: Arc<Document>,
    size: [u32; 2],
    keep_transparency: bool,
    path: &Path,
    renders: &Renders,
    cancel: &AtomicBool,
    done: &AtomicUsize,
) -> Outcome {
    let cancelled = || cancel.load(Ordering::Relaxed);
    let frames = document.frames;
    let canvas = document.canvas;
    let delays: Vec<u16> = ugu_io::gif::delays(frames, f64::from(document.frames_per_second), 100)
        .into_iter()
        .map(|delay| delay.min(u32::from(u16::MAX)) as u16)
        .collect();
    let (to_export, drawn) = sync_channel(QUEUE);
    let quantized = std::thread::scope(|scope| {
        scope.spawn(|| {
            for frame in 0..frames {
                if cancelled() {
                    break;
                }
                let Some(pixmap) = renders.export(document.clone(), i64::from(frame)) else {
                    break;
                };
                if to_export.send(pixmap).is_err() {
                    break;
                }
            }
            drop(to_export);
        });
        let mut quantizer = ugu_io::gif::Quantizer::new(size);
        let mut taken = 0;
        for pixmap in drawn {
            if cancelled() {
                // The drawing thread stops at its next frame.
                return None;
            }
            let shrunk = ugu_io::image::shrink(pixmap.data_as_u8_slice(), canvas, size);
            quantizer.add(&straight(&shrunk, keep_transparency));
            taken += 1;
            done.store(taken, Ordering::Relaxed);
        }
        Some((taken, quantizer.finish()))
    });
    let Some((taken, quantized)) = quantized else {
        return Outcome::Cancelled;
    };
    if cancelled() {
        return Outcome::Cancelled;
    }
    if taken != frames as usize {
        return Outcome::Failed("the render worker has stopped".to_owned());
    }
    let written = ugu_io::save::replace_with(path, &replace_unless(cancel), |file, _| {
        quantized
            .write(
                BufWriter::new(file),
                &delays,
                threads(),
                &cancelled,
                &mut |coded| done.store(taken + coded, Ordering::Relaxed),
            )
            .map_err(ugu_io::save::SaveError::from)
    });
    match written {
        Ok(()) => Outcome::Done,
        Err(_) if cancelled() => Outcome::Cancelled,
        Err(error) => Outcome::Failed(error.to_string()),
    }
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

    fn animation(
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
        gif(
            Arc::new(document),
            size,
            keep,
            path,
            &worker.renders(),
            &AtomicBool::new(cancel),
            &AtomicUsize::new(0),
        )
    }

    fn decoded(path: &Path) -> Vec<(u32, image::RgbaImage)> {
        let file = std::io::BufReader::new(std::fs::File::open(path).unwrap());
        image::AnimationDecoder::into_frames(image::codecs::gif::GifDecoder::new(file).unwrap())
            .map(|frame| {
                let frame = frame.unwrap();
                let (numerator, denominator) = frame.delay().numer_denom_ms();
                (numerator / denominator, frame.into_buffer())
            })
            .collect()
    }

    #[test]
    fn an_animation_is_every_frame_at_the_size_chosen() {
        let folder = folder("gif");
        let path = folder.join("motion.gif");
        let size = scaled([40, 30], 50);
        assert_eq!(size, [20, 15]);
        assert_eq!(
            animation([200, 100, 50, 255], size, true, &path, false),
            Outcome::Done
        );
        let frames = decoded(&path);
        // 1/12 s each, in hundredths adding up: 8, 9, 8.
        assert_eq!(
            frames.iter().map(|(delay, _)| *delay).collect::<Vec<_>>(),
            [80, 90, 80]
        );
        for (_, frame) in &frames {
            assert_eq!(frame.dimensions(), (20, 15));
            assert!(frame.pixels().all(|pixel| pixel.0 == [200, 100, 50, 255]));
        }
        // A transparent background stays so, or becomes white paper.
        assert_eq!(
            animation([0, 0, 0, 0], size, true, &path, false),
            Outcome::Done
        );
        assert!(decoded(&path)[0].1.pixels().all(|pixel| pixel.0[3] == 0));
        assert_eq!(
            animation([0, 0, 0, 0], size, false, &path, false),
            Outcome::Done
        );
        assert!(
            decoded(&path)[0]
                .1
                .pixels()
                .all(|pixel| pixel.0 == [255; 4])
        );
    }

    #[test]
    fn a_cancelled_animation_leaves_the_target_as_it_was() {
        let folder = folder("gif-cancelled");
        let target = folder.join("motion.gif");
        std::fs::write(&target, b"the old file").unwrap();
        assert_eq!(
            animation([200, 100, 50, 255], [40, 30], true, &target, true),
            Outcome::Cancelled
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"the old file");
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);
    }

    #[test]
    fn the_estimate_counts_the_kept_keys_and_what_is_on_its_way() {
        let canvas = [2048, 2048];
        let half = scaled(canvas, 50);
        let mib = |bytes: u64| bytes / (1024 * 1024);
        // Two bytes a pixel of 30 frames, then the drawn and shrunk frames.
        assert_eq!(mib(gif_bytes(canvas, half, 30, 1)), 60 + 64 + 8 + 4 + 2);
        assert!(gif_bytes(canvas, canvas, 30, 8) > gif_bytes(canvas, half, 30, 8));
        assert_eq!(scaled([10, 3], 25), [2, 1]);
        assert_eq!(scaled([10, 3], 100), [10, 3]);
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
