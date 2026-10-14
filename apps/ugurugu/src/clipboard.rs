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

use ugu_win::clipboard::{Encoded, ReadError};

use crate::canvas::{Canvas, CopiedPixels};
use crate::i18n::tr;

pub enum ClipboardEvent {
    /// The copy's image is on the system clipboard as of this sequence
    /// number.
    Written(Result<u32, String>),
    /// The image another app put on the clipboard, as an asset for document
    /// `.0`; `None` when there is none.
    Read(u64, Result<Option<(AssetId, Asset)>, String>),
}

enum Job {
    Write(CopiedPixels),
    Read { fit: [u32; 2], generation: u64 },
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
                    events(run_answering(job, run));
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

    fn keep(&mut self, clip: Arc<Clip>, pixels: CopiedPixels) {
        self.clip = Some(clip);
        self.writing += 1;
        let _ = self.to_worker.send(Job::Write(pixels));
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
                let generation = canvas.generation();
                let _ = self.to_worker.send(Job::Read { fit, generation });
            }
        }
    }

    pub fn handle(&mut self, event: ClipboardEvent, canvas: &mut Canvas) {
        match event {
            ClipboardEvent::Written(result) => {
                self.writing -= 1;
                let sequence = result.unwrap_or_else(|error| {
                    tracing::warn!(%error, "the copy could not be put on the clipboard");
                    canvas.set_notice(tr("copy-not-shared").to_owned());
                    ugu_win::clipboard::sequence()
                });
                if self.writing == 0 {
                    self.ours = Some(sequence);
                }
            }
            // Read for a document no longer shown.
            ClipboardEvent::Read(generation, _) if generation != canvas.generation() => {}
            ClipboardEvent::Read(_, Ok(Some((id, asset)))) => canvas.place_image(id, asset),
            ClipboardEvent::Read(_, Ok(None)) => {
                canvas.set_notice(tr("paste-nothing").to_owned());
            }
            ClipboardEvent::Read(_, Err(error)) => {
                tracing::warn!(%error, "the clipboard could not be read");
                canvas.set_notice(tr("paste-failed").to_owned());
            }
        }
    }
}

/// Runs `job`; one that panics is answered as failed, which the panic hook
/// has recorded, so the copy or paste waiting for it goes on and the thread
/// takes the next.
fn run_answering(job: Job, run: impl FnOnce(Job) -> ClipboardEvent) -> ClipboardEvent {
    let failed = match &job {
        Job::Write(_) => ClipboardEvent::Written(Err("the copy could not be drawn".to_owned())),
        Job::Read { generation, .. } => ClipboardEvent::Read(
            *generation,
            Err("the clipboard could not be read".to_owned()),
        ),
    };
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(job))).unwrap_or(failed)
}

fn run(job: Job) -> ClipboardEvent {
    let started = std::time::Instant::now();
    match job {
        Job::Write(pixels) => {
            let image = pixels.image();
            let size = image.size;
            let drawn = started.elapsed();
            let result = ugu_win::clipboard::write_image(image);
            tracing::debug!(
                drawn_ms = drawn.as_secs_f64() * 1000.0,
                ms = started.elapsed().as_secs_f64() * 1000.0,
                ?size,
                "copy put on the clipboard"
            );
            ClipboardEvent::Written(result)
        }
        Job::Read { fit, generation } => {
            let limit = usize::try_from(ugu_io::import::ALLOCATION).unwrap_or(usize::MAX);
            let result = ugu_win::clipboard::read_encoded(limit)
                .map_err(|error| match error {
                    ReadError::TooLarge => ugu_io::import::ImportError::TooLarge.to_string(),
                    ReadError::System(reason) => reason,
                })
                .and_then(|image| {
                    image
                        .map(|image| {
                            match image {
                                Encoded::Png(png) => {
                                    ugu_io::import::decode(std::io::Cursor::new(png), fit)
                                }
                                Encoded::Dib(dib) => ugu_io::import::decode_dib(dib, fit),
                            }
                            .map_err(|error| error.to_string())
                        })
                        .transpose()
                });
            tracing::debug!(
                ms = started.elapsed().as_secs_f64() * 1000.0,
                "clipboard image read"
            );
            ClipboardEvent::Read(generation, result)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ugu_core::document::Document;

    #[test]
    fn a_job_that_panics_is_answered_as_failed() {
        let read = Job::Read {
            fit: [8, 8],
            generation: 7,
        };
        let answer = run_answering(read, |_| panic!("drawing failed"));
        assert!(matches!(answer, ClipboardEvent::Read(7, Err(_))));
    }

    #[test]
    fn a_copy_other_apps_do_not_get_is_said() {
        let mut clipboard = Clipboard::new(|_| {});
        let mut canvas = Canvas::new(Document::new([8, 8]), |_| {});
        clipboard.writing = 1;
        clipboard.handle(ClipboardEvent::Written(Err("busy".to_owned())), &mut canvas);
        assert_eq!(canvas.notice(), Some(tr("copy-not-shared")));
        assert!(clipboard.ours.is_some());
    }
}
