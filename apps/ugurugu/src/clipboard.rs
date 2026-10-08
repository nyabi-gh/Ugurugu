// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Copy, cut and paste. A copy keeps the layer's operations for pasting in
//! this app and puts its pixels on the system clipboard for other apps. A
//! paste takes the kept copy while the system clipboard still holds what
//! this app put there, and otherwise an image another app put there, which
//! becomes a placed image. The system clipboard is read and written in
//! order on a thread of its own, as a large image takes a while.

use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};

use ugu_core::clip::Clip;
use ugu_core::ops::AssetId;
use ugu_core::store::Asset;

use ugu_win::clipboard::Image;

use crate::canvas::Canvas;
use crate::i18n::tr;

pub enum ClipboardEvent {
    /// The copy's image is on the system clipboard as of this sequence
    /// number.
    Written(Result<u32, String>),
    /// The image another app put on the clipboard, as an asset; `None` when
    /// there is none.
    Read(Result<Option<(AssetId, Asset)>, String>),
}

enum Job {
    Write(Image),
    Read { fit: [u32; 2] },
}

pub struct Clipboard {
    to_worker: Sender<Job>,
    clip: Option<Arc<Clip>>,
    /// Images being written.
    writing: u32,
    /// The system clipboard's sequence number when it last held this app's
    /// copy.
    ours: Option<u32>,
}

impl Clipboard {
    pub fn new(events: impl Fn(ClipboardEvent) + Send + 'static) -> Self {
        let (to_worker, jobs) = channel::<Job>();
        std::thread::Builder::new()
            .name("clipboard".to_owned())
            .spawn(move || {
                for job in jobs {
                    events(run(job));
                }
            })
            .expect("cannot start the clipboard thread");
        Self {
            to_worker,
            clip: None,
            writing: 0,
            ours: None,
        }
    }

    pub fn copy(&mut self, canvas: &mut Canvas) {
        if let Some((clip, image)) = canvas.copy() {
            self.keep(clip, image);
        }
    }

    pub fn cut(&mut self, canvas: &mut Canvas) {
        if let Some((clip, image)) = canvas.copy() {
            self.keep(clip, image);
            canvas.delete_selected();
        }
    }

    fn keep(&mut self, clip: Arc<Clip>, image: Image) {
        self.clip = Some(clip);
        self.writing += 1;
        let _ = self.to_worker.send(Job::Write(image));
    }

    pub fn paste(&mut self, canvas: &mut Canvas) {
        let holds_ours = self.writing > 0
            || self
                .ours
                .is_some_and(|ours| ours == ugu_win::clipboard::sequence());
        match &self.clip {
            Some(clip) if holds_ours => canvas.paste(clip),
            _ => {
                let fit = canvas.session().document().canvas;
                let _ = self.to_worker.send(Job::Read { fit });
            }
        }
    }

    pub fn handle(&mut self, event: ClipboardEvent, canvas: &mut Canvas) {
        match event {
            ClipboardEvent::Written(result) => {
                self.writing -= 1;
                let sequence = result.unwrap_or_else(|error| {
                    // Other apps do not get it, but this app still pastes it.
                    tracing::warn!(%error, "the copy could not be put on the clipboard");
                    ugu_win::clipboard::sequence()
                });
                if self.writing == 0 {
                    self.ours = Some(sequence);
                }
            }
            ClipboardEvent::Read(Ok(Some((id, asset)))) => canvas.place_image(id, asset),
            ClipboardEvent::Read(Ok(None)) => canvas.set_notice(tr("paste-nothing").to_owned()),
            ClipboardEvent::Read(Err(error)) => {
                tracing::warn!(%error, "the clipboard could not be read");
                canvas.set_notice(tr("paste-failed").to_owned());
            }
        }
    }
}

fn run(job: Job) -> ClipboardEvent {
    let started = std::time::Instant::now();
    match job {
        Job::Write(image) => {
            let size = image.size;
            let result = ugu_win::clipboard::write_image(image);
            tracing::debug!(
                ms = started.elapsed().as_secs_f64() * 1000.0,
                ?size,
                "copy put on the clipboard"
            );
            ClipboardEvent::Written(result)
        }
        Job::Read { fit } => {
            let result = ugu_win::clipboard::read_image().and_then(|image| {
                image
                    .map(|image| {
                        ugu_io::import::from_rgba(image.size, image.straight, fit)
                            .map_err(|error| error.to_string())
                    })
                    .transpose()
            });
            tracing::debug!(
                ms = started.elapsed().as_secs_f64() * 1000.0,
                "clipboard image read"
            );
            ClipboardEvent::Read(result)
        }
    }
}
