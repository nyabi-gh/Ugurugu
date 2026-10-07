// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Renders on a worker thread, so the render thread keeps showing the
//! previous image and taking input while a frame is drawn: the canvas split
//! for editing, and whole frames for playback. Only the newest split request
//! matters, and it goes before any frame.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use ugu_core::document::{Document, LayerId};
use ugu_render::compose::{Split, composite};
use ugu_render::document::{DocumentRenderer, Purpose};
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
        pixels: Arc<Pixmap>,
    },
}

#[derive(Default)]
struct Queue {
    split: Option<(Key, Arc<Document>)>,
    frames: VecDeque<(Version, u32, Arc<Document>)>,
    stop: bool,
}

pub struct CacheWorker {
    queue: Arc<(Mutex<Queue>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

enum Job {
    Split(Key, Arc<Document>),
    Frame(Version, u32, Arc<Document>),
}

impl CacheWorker {
    /// `done` receives each finished render, on the worker thread.
    pub fn start(done: impl Fn(Rendered) + Send + 'static) -> Self {
        let queue = Arc::new((Mutex::new(Queue::default()), Condvar::new()));
        let shared = queue.clone();
        let threads = std::thread::available_parallelism()
            .map_or(1, |count| count.get().saturating_sub(1))
            .min(8) as u16;
        let thread = std::thread::Builder::new()
            .name("canvas cache".to_owned())
            .spawn(move || {
                let mut renderer = DocumentRenderer::new(threads);
                while let Some(job) = next(&shared) {
                    let started = Instant::now();
                    match job {
                        Job::Split(key, document) => {
                            let Some(split) =
                                renderer.split(&document, key.layer, i64::from(key.frame))
                            else {
                                continue;
                            };
                            let [width, height] = document.canvas.map(|edge| edge as u16);
                            let mut display = Pixmap::new(width, height);
                            composite(
                                &split,
                                None,
                                [0, 0, u32::from(width), u32::from(height)],
                                &mut display,
                            );
                            tracing::debug!(
                                ms = started.elapsed().as_secs_f64() * 1000.0,
                                revision = key.version.revision,
                                "canvas split rendered"
                            );
                            done(Rendered::Split {
                                key,
                                split,
                                display,
                            });
                        }
                        Job::Frame(version, frame, document) => {
                            let [width, height] = document.canvas.map(|edge| edge as u16);
                            let mut pixels = Pixmap::new(width, height);
                            renderer.render(
                                &document,
                                i64::from(frame),
                                Purpose::Display,
                                &mut pixels,
                            );
                            tracing::debug!(
                                ms = started.elapsed().as_secs_f64() * 1000.0,
                                frame,
                                "playback frame rendered"
                            );
                            done(Rendered::Frame {
                                version,
                                frame,
                                pixels: Arc::new(pixels),
                            });
                        }
                    }
                }
            })
            .expect("cannot start the canvas cache thread");
        Self {
            queue,
            thread: Some(thread),
        }
    }

    pub fn request(&self, key: Key, document: Arc<Document>) {
        let (lock, wake) = &*self.queue;
        lock.lock().expect("queue lock").split = Some((key, document));
        wake.notify_one();
    }

    /// Replaces the frames waiting to be rendered for playback.
    pub fn request_frames(&self, version: Version, frames: &[u32], document: &Arc<Document>) {
        let (lock, wake) = &*self.queue;
        let mut queue = lock.lock().expect("queue lock");
        queue.frames = frames
            .iter()
            .map(|&frame| (version, frame, document.clone()))
            .collect();
        wake.notify_one();
    }
}

/// The next job, splits first; `None` once stopped.
fn next(queue: &(Mutex<Queue>, Condvar)) -> Option<Job> {
    let (lock, wake) = queue;
    let mut queue = lock.lock().expect("queue lock");
    loop {
        if queue.stop {
            return None;
        }
        if let Some((key, document)) = queue.split.take() {
            return Some(Job::Split(key, document));
        }
        if let Some((version, frame, document)) = queue.frames.pop_front() {
            return Some(Job::Frame(version, frame, document));
        }
        queue = wake.wait(queue).expect("queue lock");
    }
}

impl Drop for CacheWorker {
    fn drop(&mut self) {
        let (lock, wake) = &*self.queue;
        lock.lock().expect("queue lock").stop = true;
        wake.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
