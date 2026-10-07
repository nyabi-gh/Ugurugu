// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Renders the canvas split on a worker thread, so the render thread keeps
//! showing the previous image and taking input while a frame is drawn.
//! Only the newest request matters; one that arrives while another renders
//! replaces any still waiting.

use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use ugu_core::document::{Document, LayerId};
use ugu_render::compose::{Split, composite};
use ugu_render::document::DocumentRenderer;
use vello_cpu::Pixmap;

/// What a split shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub revision: u64,
    pub layer: LayerId,
    /// The frame within the cycle.
    pub frame: u32,
}

pub struct Rendered {
    pub key: Key,
    pub split: Split,
    /// The split put together.
    pub display: Pixmap,
}

struct Job {
    key: Key,
    document: Document,
}

#[derive(Default)]
struct Queue {
    job: Option<Job>,
    stop: bool,
}

pub struct CacheWorker {
    queue: Arc<(Mutex<Queue>, Condvar)>,
    thread: Option<JoinHandle<()>>,
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
                loop {
                    let job = {
                        let (lock, wake) = &*shared;
                        let mut queue = lock.lock().expect("queue lock");
                        loop {
                            if queue.stop {
                                return;
                            }
                            if let Some(job) = queue.job.take() {
                                break job;
                            }
                            queue = wake.wait(queue).expect("queue lock");
                        }
                    };
                    let started = Instant::now();
                    let Some(split) =
                        renderer.split(&job.document, job.key.layer, i64::from(job.key.frame))
                    else {
                        continue;
                    };
                    let [width, height] = job.document.canvas.map(|edge| edge as u16);
                    let mut display = Pixmap::new(width, height);
                    composite(
                        &split,
                        None,
                        [0, 0, u32::from(width), u32::from(height)],
                        &mut display,
                    );
                    tracing::debug!(
                        ms = started.elapsed().as_secs_f64() * 1000.0,
                        revision = job.key.revision,
                        "canvas split rendered"
                    );
                    done(Rendered {
                        key: job.key,
                        split,
                        display,
                    });
                }
            })
            .expect("cannot start the canvas cache thread");
        Self {
            queue,
            thread: Some(thread),
        }
    }

    pub fn request(&self, key: Key, document: Document) {
        let (lock, wake) = &*self.queue;
        lock.lock().expect("queue lock").job = Some(Job { key, document });
        wake.notify_one();
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
