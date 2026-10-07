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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use ugu_core::document::{Document, LayerId};
use ugu_core::history::LayerRevisions;
use ugu_render::compose::Split;
use ugu_render::document::{DocumentRenderer, FULL_DETAIL, Purpose, scaled_size};
use ugu_render::plan::RenderPlan;
use vello_cpu::Pixmap;

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
        split: Split,
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
    quit: bool,
}

struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
    /// Set to stop the render under way.
    stop: Arc<AtomicBool>,
}

/// Hands work to the worker; cheap to clone.
#[derive(Clone)]
pub struct Renders {
    shared: Arc<Shared>,
}

pub struct CacheWorker {
    renders: Renders,
    thread: Option<JoinHandle<()>>,
}

impl CacheWorker {
    /// `done` receives each finished split and playback frame, on the
    /// worker thread.
    pub fn start(done: impl Fn(Rendered) + Send + 'static) -> Self {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue::default()),
            wake: Condvar::new(),
            stop: Arc::new(AtomicBool::new(false)),
        });
        let worker = shared.clone();
        // One thread is left for the render thread, which takes input.
        // `UGU_WORKER_THREADS` sets the count, to measure as a smaller
        // processor would.
        let threads = std::env::var("UGU_WORKER_THREADS")
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or_else(|| {
                std::thread::available_parallelism()
                    .map_or(1, |count| count.get().saturating_sub(1).max(1))
                    .min(usize::from(u16::MAX)) as u16
            });
        let thread = std::thread::Builder::new()
            .name("canvas cache".to_owned())
            .spawn(move || {
                let mut renderer = DocumentRenderer::new(threads);
                renderer.set_stop(worker.stop.clone());
                while let Some(job) = next(&worker) {
                    if let Some(job) = run(&mut renderer, job, &done) {
                        // An export stopped for other work goes first next.
                        worker
                            .queue
                            .lock()
                            .expect("queue lock")
                            .exports
                            .push_front(job);
                    }
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
            })
            .expect("cannot start the canvas cache thread");
        Self {
            renders: Renders { shared },
            thread: Some(thread),
        }
    }

    pub fn renders(&self) -> Renders {
        self.renders.clone()
    }

    pub fn request(&self, key: Key, snapshot: Snapshot) {
        self.renders.shared.request(key, snapshot);
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
        self.renders
            .shared
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
        let shared = &self.renders.shared;
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
        let (reply, answer) = channel();
        let shared = &self.shared;
        shared
            .queue
            .lock()
            .expect("queue lock")
            .exports
            .push_back(Job::Export {
                document,
                frame,
                reply,
            });
        shared.wake.notify_one();
        answer.recv().ok()
    }
}

/// The next job by priority, marked as running; `None` once quitting.
fn next(shared: &Shared) -> Option<Job> {
    let mut queue = shared.queue.lock().expect("queue lock");
    loop {
        if queue.quit {
            return None;
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
            tracing::debug!(
                ms = ms(),
                revision = key.version.revision,
                "canvas split rendered"
            );
            done(Rendered::Split {
                key,
                split,
                display,
            });
        }
        Job::Frame(version, frame, preview, snapshot) => {
            let document = &snapshot.document;
            let [width, height] =
                scaled_size(document.canvas, preview.shrink).map(|edge| edge as u16);
            let mut pixels = Pixmap::new(width, height);
            let plan = RenderPlan::new(document, Purpose::Display);
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
            frame,
            reply,
        } => {
            let [width, height] = document.canvas.map(|edge| edge as u16);
            let mut pixels = Pixmap::new(width, height);
            let plan = RenderPlan::new(&document, Purpose::Export);
            if !renderer.render_plan(&document, &plan, frame, None, &mut pixels) {
                tracing::debug!(ms = ms(), "export frame stopped");
                return Some(Job::Export {
                    document,
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
    fn drop(&mut self) {
        let shared = &self.renders.shared;
        shared.queue.lock().expect("queue lock").quit = true;
        shared.stop.store(true, Ordering::Relaxed);
        shared.wake.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
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
