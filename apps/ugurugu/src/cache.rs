// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Renders on one worker thread, so the render thread keeps showing the
//! previous image and taking input while a frame is drawn, and renders never
//! compete with each other for the processor.
//!
//! Work goes in this order: the canvas split for editing, playback frames
//! (the one due first at the front), export frames, then layer thumbnails. Only the newest
//! split and the newest list of playback frames matter; anything waiting
//! that they replace is dropped. A render under way that is no longer wanted
//! is stopped between layers or rows, and an export stopped that way starts
//! again later.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use ugu_core::document::{Document, LayerId};
use ugu_core::history::LayerRevisions;
use ugu_render::compose::Split;
use ugu_render::document::{DocumentRenderer, FULL_DETAIL, Purpose, scaled_size};
use ugu_render::plan::RenderPlan;
use ugu_render::stream::Held;
use vello_cpu::Pixmap;

use crate::budget::Budget;

/// Which state of which open document a render shows. Revisions start
/// again with each document opened, so they alone could match a render of
/// the document before.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Version {
    pub document: u64,
    pub revision: u64,
}

/// What a split shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub version: Version,
    pub layer: LayerId,
    /// The frame within the cycle.
    pub frame: u32,
}

/// How playback frames are drawn: at 1/`shrink` of the canvas size, with
/// stroke samples spaced `detail`/16 as far apart as at full detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preview {
    pub shrink: u32,
    pub detail: u32,
}

pub enum Rendered {
    Split {
        key: Key,
        split: Box<Split>,
        /// The split put together.
        display: Pixmap,
    },
    Frame {
        version: Version,
        frame: u32,
        preview: Preview,
        pixels: Arc<Pixmap>,
    },
    /// Layers' own pixels, small, each with the revision it shows; a
    /// render stopped for other work brings what it had.
    Thumbnails {
        document: u64,
        thumbnails: Vec<(LayerId, u64, Pixmap)>,
    },
}

/// Thumbnails fit in this many pixels, twice the layer list's size.
pub const THUMBNAIL: [u32; 2] = [96, 64];

/// A document state handed to the worker, with what its layers are made
/// from.
#[derive(Clone)]
pub struct Snapshot {
    pub document: Arc<Document>,
    pub layers: Arc<LayerRevisions>,
}

enum Job {
    Split(Key, Snapshot),
    Frame(Version, u32, Preview, Snapshot),
    Export {
        document: Arc<Document>,
        plan: RenderPlan,
        frame: i64,
        reply: Sender<Pixmap>,
    },
    Thumbnails(u64, Vec<(LayerId, u64)>, Arc<Document>),
}

/// What the worker is doing now.
#[derive(Clone, Copy, PartialEq)]
enum Running {
    Split,
    Frame(Version, u32, Preview),
    Export,
    Thumbnails,
}

#[derive(Default)]
struct Queue {
    split: Option<(Key, Snapshot)>,
    frames: VecDeque<(Version, u32, Preview, Snapshot)>,
    exports: VecDeque<Job>,
    /// The newest list of thumbnails wanted.
    thumbnails: Option<Job>,
    running: Option<Running>,
    /// Its tab has closed: only exports are drawn, while any are asked for.
    quit: bool,
    /// `Renders` that may still ask for exports.
    askers: usize,
    /// The thread has ended, so nothing asked of it is answered.
    ended: bool,
}

struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
    /// Set to stop the render under way.
    stop: Arc<AtomicBool>,
    /// What the layers' own surfaces may take, from `Budget`.
    surface_budget: AtomicUsize,
    /// What they took after the last render.
    surface_bytes: AtomicUsize,
}

/// Asks the worker for exports; cheap to clone. While one is held, the
/// worker outlives its tab, so an export of a tab closed meanwhile ends.
pub struct Renders {
    shared: Arc<Shared>,
}

impl Renders {
    fn new(shared: Arc<Shared>) -> Self {
        shared.queue.lock().expect("queue lock").askers += 1;
        Self { shared }
    }
}

impl Clone for Renders {
    fn clone(&self) -> Self {
        Self::new(self.shared.clone())
    }
}

impl Drop for Renders {
    fn drop(&mut self) {
        self.shared.queue.lock().expect("queue lock").askers -= 1;
        // A worker whose tab has closed may be waiting for the last one.
        self.shared.wake.notify_one();
    }
}

pub struct CacheWorker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl CacheWorker {
    /// `done` receives each finished split and playback frame, on the
    /// worker thread.
    pub fn start(done: impl Fn(Rendered) + Send + 'static) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue::default()),
            wake: Condvar::new(),
            stop: Arc::new(AtomicBool::new(false)),
            surface_budget: AtomicUsize::new(Budget::now(0).surfaces),
            surface_bytes: AtomicUsize::new(0),
        });
        let worker = shared.clone();
        let threads = worker_threads();
        let thread = std::thread::Builder::new()
            .name("canvas cache".to_owned())
            .spawn(move || {
                let renderer_for = |worker: &Shared| {
                    let mut renderer = DocumentRenderer::new(threads);
                    renderer.set_stop(worker.stop.clone());
                    renderer
                };
                let mut renderer = renderer_for(&worker);
                while let Some(job) = next(&worker) {
                    renderer.set_surface_budget(worker.surface_budget.load(Ordering::Relaxed));
                    // A job that panics is dropped, so an export waiting for
                    // it gets no picture; the panic hook has recorded it.
                    let again = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        run(&mut renderer, job, &done)
                    }))
                    .unwrap_or_else(|_| {
                        renderer = renderer_for(&worker);
                        None
                    });
                    if let Some(job) = again {
                        // An export stopped for other work goes first next.
                        worker
                            .queue
                            .lock()
                            .expect("queue lock")
                            .exports
                            .push_front(job);
                    }
                    worker
                        .surface_bytes
                        .store(renderer.surface_bytes(), Ordering::Relaxed);
                    let mut queue = worker.queue.lock().expect("queue lock");
                    queue.running = None;
                    let idle = queue.split.is_none()
                        && queue.frames.is_empty()
                        && queue.exports.is_empty()
                        && queue.thumbnails.is_none();
                    drop(queue);
                    // Measured free: drawing allocates it again at no cost.
                    if idle {
                        renderer.release_scratch();
                    }
                }
                worker.queue.lock().expect("queue lock").ended = true;
            })?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    pub fn renders(&self) -> Renders {
        Renders::new(self.shared.clone())
    }

    /// Lets the layers' own surfaces take `bytes` from the next render on.
    pub fn set_surface_budget(&self, bytes: usize) {
        self.shared.surface_budget.store(bytes, Ordering::Relaxed);
    }

    /// What the layers' own surfaces took after the last render.
    pub fn surface_bytes(&self) -> usize {
        self.shared.surface_bytes.load(Ordering::Relaxed)
    }

    pub fn request(&self, key: Key, snapshot: Snapshot) {
        self.shared.request(key, snapshot);
    }

    /// Replaces the playback frames waiting to be rendered, the first due
    /// first, drawn as `preview` says. Layers that do not move are drawn
    /// once for all of them.
    pub fn request_frames(
        &self,
        version: Version,
        frames: &[u32],
        snapshot: &Snapshot,
        preview: Preview,
    ) {
        self.shared
            .request_frames(version, frames, snapshot, preview);
    }

    /// Replaces the thumbnails waiting to be drawn: `layers` of `document`
    /// with the revisions they are wanted at.
    pub fn request_thumbnails(
        &self,
        generation: u64,
        layers: Vec<(LayerId, u64)>,
        document: Arc<Document>,
    ) {
        let shared = &self.shared;
        let mut queue = shared.queue.lock().expect("queue lock");
        queue.thumbnails = Some(Job::Thumbnails(generation, layers, document));
        shared.wake.notify_one();
    }
}

impl Shared {
    fn request(&self, key: Key, snapshot: Snapshot) {
        let mut queue = self.queue.lock().expect("queue lock");
        queue.split = Some((key, snapshot));
        // Editing comes first: whatever is under way is no longer wanted
        // now, or waits.
        if queue.running.is_some() {
            self.stop.store(true, Ordering::Relaxed);
        }
        self.wake.notify_one();
    }

    fn request_frames(
        &self,
        version: Version,
        frames: &[u32],
        snapshot: &Snapshot,
        preview: Preview,
    ) {
        let mut queue = self.queue.lock().expect("queue lock");
        queue.frames = frames
            .iter()
            .map(|&frame| (version, frame, preview, snapshot.clone()))
            .collect();
        if let Some(Running::Frame(running, frame, running_preview)) = queue.running
            && !(running == version && running_preview == preview && frames.contains(&frame))
        {
            self.stop.store(true, Ordering::Relaxed);
        }
        self.wake.notify_one();
    }
}

impl Renders {
    /// Renders `frame` of `document` for export, after the canvas's own
    /// work; waits for it. `None` when the worker has stopped.
    pub fn export(&self, document: Arc<Document>, frame: i64) -> Option<Pixmap> {
        let plan = RenderPlan::new(&document, Purpose::Export, frame);
        self.render(document, plan, frame)
    }

    /// Renders `frame` of `document` as `plan` says, after the canvas's own
    /// work; waits for it. `None` when the worker has stopped.
    pub fn render(&self, document: Arc<Document>, plan: RenderPlan, frame: i64) -> Option<Pixmap> {
        let (reply, answer) = channel();
        let shared = &self.shared;
        let mut queue = shared.queue.lock().expect("queue lock");
        if queue.ended {
            return None;
        }
        queue.exports.push_back(Job::Export {
            document,
            plan,
            frame,
            reply,
        });
        drop(queue);
        shared.wake.notify_one();
        answer.recv().ok()
    }
}

/// The next job by priority, marked as running; `None` once the tab has
/// closed and no export is or can still be asked for.
fn next(shared: &Shared) -> Option<Job> {
    let mut queue = shared.queue.lock().expect("queue lock");
    loop {
        if queue.quit {
            // Nothing shows them any more.
            queue.split = None;
            queue.frames.clear();
            queue.thumbnails = None;
            if queue.exports.is_empty() && queue.askers == 0 {
                return None;
            }
        }
        let job = if let Some((key, snapshot)) = queue.split.take() {
            Some((Running::Split, Job::Split(key, snapshot)))
        } else if let Some((version, frame, preview, snapshot)) = queue.frames.pop_front() {
            Some((
                Running::Frame(version, frame, preview),
                Job::Frame(version, frame, preview, snapshot),
            ))
        } else if let Some(job) = queue.exports.pop_front() {
            Some((Running::Export, job))
        } else {
            queue
                .thumbnails
                .take()
                .map(|job| (Running::Thumbnails, job))
        };
        if let Some((running, job)) = job {
            queue.running = Some(running);
            shared.stop.store(false, Ordering::Relaxed);
            return Some(job);
        }
        queue = shared.wake.wait(queue).expect("queue lock");
    }
}

/// Runs `job`; returns an export that was stopped, to run again.
fn run(renderer: &mut DocumentRenderer, job: Job, done: &impl Fn(Rendered)) -> Option<Job> {
    let started = Instant::now();
    let ms = || started.elapsed().as_secs_f64() * 1000.0;
    renderer.set_detail(match &job {
        Job::Frame(_, _, preview, _) => preview.detail,
        _ => FULL_DETAIL,
    });
    match job {
        Job::Split(key, snapshot) => {
            let document = &snapshot.document;
            let [width, height] = document.canvas.map(|edge| edge as u16);
            let mut display = Pixmap::new(width, height);
            let Some(split) = renderer.split(
                document,
                key.layer,
                i64::from(key.frame),
                Some(&snapshot.layers),
                &mut display,
            ) else {
                tracing::debug!(ms = ms(), "canvas split stopped");
                return None;
            };
            let held: usize = split
                .sources
                .iter()
                .map(|source| match source {
                    Held::Tiles(surface) => surface.bytes(),
                    Held::Whole(pixmap) => pixmap.data().len() * 4,
                })
                .sum();
            tracing::debug!(
                ms = ms(),
                revision = key.version.revision,
                held_mib = held >> 20,
                surfaces_mib = renderer.surface_bytes() >> 20,
                "canvas split rendered"
            );
            done(Rendered::Split {
                key,
                split: Box::new(split),
                display,
            });
        }
        Job::Frame(version, frame, preview, snapshot) => {
            let document = &snapshot.document;
            let [width, height] =
                scaled_size(document.canvas, preview.shrink).map(|edge| edge as u16);
            let mut pixels = Pixmap::new(width, height);
            let plan = RenderPlan::new(document, Purpose::Display, i64::from(frame));
            let finished = renderer.render_scaled(
                document,
                &plan,
                i64::from(frame),
                Some(&snapshot.layers),
                preview.shrink,
                &mut pixels,
            );
            if !finished {
                tracing::debug!(ms = ms(), frame, "playback frame stopped");
                return None;
            }
            tracing::debug!(ms = ms(), frame, "playback frame rendered");
            done(Rendered::Frame {
                version,
                frame,
                preview,
                pixels: Arc::new(pixels),
            });
        }
        Job::Export {
            document,
            plan,
            frame,
            reply,
        } => {
            let [width, height] = document.canvas.map(|edge| edge as u16);
            let mut pixels = Pixmap::new(width, height);
            if !renderer.render_plan(&document, &plan, frame, None, &mut pixels) {
                tracing::debug!(ms = ms(), "export frame stopped");
                return Some(Job::Export {
                    document,
                    plan,
                    frame,
                    reply,
                });
            }
            tracing::debug!(ms = ms(), "export frame rendered");
            let _ = reply.send(pixels);
        }
        Job::Thumbnails(generation, layers, document) => {
            let mut thumbnails = Vec::new();
            for (id, revision) in layers {
                if renderer.is_stopped() {
                    break;
                }
                if let Some(pixels) = renderer.thumbnail(&document, id, THUMBNAIL) {
                    thumbnails.push((id, revision, pixels));
                }
            }
            tracing::debug!(ms = ms(), count = thumbnails.len(), "thumbnails rendered");
            done(Rendered::Thumbnails {
                document: generation,
                thumbnails,
            });
        }
    }
    None
}

impl Drop for CacheWorker {
    /// Stops what was drawn for the tab. An export still asking for frames
    /// keeps the thread, which then ends by itself after it.
    fn drop(&mut self) {
        let shared = &self.shared;
        let mut queue = shared.queue.lock().expect("queue lock");
        queue.quit = true;
        if queue.running != Some(Running::Export) {
            shared.stop.store(true, Ordering::Relaxed);
        }
        let exporting = queue.askers > 0;
        drop(queue);
        shared.wake.notify_one();
        if let Some(thread) = self.thread.take()
            && !exporting
        {
            let _ = thread.join();
        }
    }
}

/// How many threads a renderer off the render thread uses: all but one,
/// which is left for the render thread, as it takes input.
/// `UGU_WORKER_THREADS` sets the count, to measure as a smaller processor
/// would.
pub fn worker_threads() -> u16 {
    std::env::var("UGU_WORKER_THREADS")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map_or(1, |count| count.get().saturating_sub(1).max(1))
                .min(usize::from(u16::MAX)) as u16
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(document: Document) -> Snapshot {
        Snapshot {
            document: Arc::new(document),
            layers: Arc::new(LayerRevisions::default()),
        }
    }

    fn shared() -> Shared {
        Shared {
            queue: Mutex::new(Queue::default()),
            wake: Condvar::new(),
            stop: Arc::new(AtomicBool::new(false)),
            surface_budget: AtomicUsize::new(0),
            surface_bytes: AtomicUsize::new(0),
        }
    }

    /// Waits for `export` on a thread of its own, at most ten seconds.
    fn export_within(renders: &Renders, document: Document) -> Option<Option<Pixmap>> {
        let renders = renders.clone();
        let (answer, answered) = channel();
        std::thread::spawn(move || {
            let _ = answer.send(renders.export(Arc::new(document), 0));
        });
        answered
            .recv_timeout(std::time::Duration::from_secs(10))
            .ok()
    }

    #[test]
    fn an_export_whose_render_panics_gets_no_picture_and_the_next_one_does() {
        use ugu_core::document::LayerKind;
        use ugu_core::ops::{MaskId, Op, Rgba8};
        use ugu_core::store::Mask;

        let worker = CacheWorker::start(|_| {}).unwrap();
        let renders = worker.renders();
        // A mask no file or edit can hold any more: its pixels on either
        // side of i32::MAX made drawing it panic.
        let mut broken = Document::new([8, 8]);
        broken.store.masks.insert(
            MaskId(0),
            Mask {
                bounds: [i32::MAX - 3, 0, 8, 2],
                bits: vec![0b1000_0000, 0b0000_0001].into(),
            },
        );
        let LayerKind::Paint(paint) = &mut broken.layers[0].kind else {
            unreachable!()
        };
        paint.ops.push(Op::Fill {
            coverage: MaskId(0),
            color: Rgba8([0, 0, 0, 255]),
            antialias: false,
            clip: None,
        });
        assert!(matches!(export_within(&renders, broken), Some(None)));
        assert!(matches!(
            export_within(&renders, Document::new([8, 8])),
            Some(Some(_))
        ));
    }

    #[test]
    fn a_closed_tabs_worker_draws_what_an_export_asks_then_ends() {
        let worker = CacheWorker::start(|_| {}).unwrap();
        let shared = worker.shared.clone();
        let renders = worker.renders();
        drop(worker);
        for _ in 0..2 {
            assert!(matches!(
                export_within(&renders, Document::new([8, 8])),
                Some(Some(_))
            ));
        }
        drop(renders);
        let started = Instant::now();
        while !shared.queue.lock().unwrap().ended {
            assert!(started.elapsed().as_secs() < 10, "the worker ends");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn a_split_goes_before_frames_frames_before_exports_and_exports_before_thumbnails() {
        let shared = shared();
        let version = Version {
            document: 0,
            revision: 0,
        };
        let state = snapshot(Document::new([8, 8]));
        {
            let mut queue = shared.queue.lock().unwrap();
            let (reply, _) = channel();
            queue.thumbnails = Some(Job::Thumbnails(
                0,
                vec![(LayerId(1), 1)],
                state.document.clone(),
            ));
            queue.exports.push_back(Job::Export {
                document: state.document.clone(),
                plan: RenderPlan::new(&state.document, Purpose::Export, 0),
                frame: 0,
                reply,
            });
            queue.frames.push_back((version, 3, full(1), state.clone()));
            queue.split = Some((
                Key {
                    version,
                    layer: LayerId(1),
                    frame: 0,
                },
                state,
            ));
        }
        let order: Vec<_> = (0..4)
            .map(|_| match next(&shared).unwrap() {
                Job::Split(..) => "split",
                Job::Frame(..) => "frame",
                Job::Export { .. } => "export",
                Job::Thumbnails(..) => "thumbnails",
            })
            .collect();
        assert_eq!(order, ["split", "frame", "export", "thumbnails"]);
    }

    fn full(shrink: u32) -> Preview {
        Preview {
            shrink,
            detail: FULL_DETAIL,
        }
    }

    #[test]
    fn a_new_split_stops_the_render_under_way() {
        let shared = shared();
        let state = snapshot(Document::new([8, 8]));
        let version = Version {
            document: 0,
            revision: 0,
        };
        shared.queue.lock().unwrap().running = Some(Running::Frame(version, 2, full(1)));
        // A list that still has the frame keeps it going; one without it, or
        // at another size or detail, stops it.
        shared.request_frames(version, &[2, 3], &state, full(1));
        assert!(!shared.stop.load(Ordering::Relaxed));
        shared.request_frames(version, &[2, 3], &state, full(2));
        assert!(shared.stop.load(Ordering::Relaxed));
        shared.stop.store(false, Ordering::Relaxed);
        let fewer = Preview {
            shrink: 1,
            detail: FULL_DETAIL * 2,
        };
        shared.request_frames(version, &[2, 3], &state, fewer);
        assert!(shared.stop.load(Ordering::Relaxed));
        shared.stop.store(false, Ordering::Relaxed);
        shared.request_frames(version, &[3], &state, full(1));
        assert!(shared.stop.load(Ordering::Relaxed));
        shared.stop.store(false, Ordering::Relaxed);
        let key = Key {
            version,
            layer: LayerId(1),
            frame: 0,
        };
        shared.queue.lock().unwrap().running = Some(Running::Export);
        shared.request(key, state);
        assert!(shared.stop.load(Ordering::Relaxed));
    }
}
