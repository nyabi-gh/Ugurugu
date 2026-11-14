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

use ugu_platform::clipboard::{Encoded, ReadError};

use crate::canvas::{Canvas, CopiedPixels};
use crate::i18n::tr;

/// How the app recognizes that the system clipboard still holds its last copy.
#[cfg(not(target_os = "linux"))]
pub type Stamp = u32;

/// Wayland and X11 expose no global clipboard sequence counter, and clipboard
/// managers such as KDE Klipper re-own selections immediately after a copy.
/// Recording the copied image's dimensions and straight-RGBA8 SHA-256 lets
/// `paste` detect whether the system clipboard still holds our pixels.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stamp {
    Shared { size: [u32; 2], sha256: [u8; 32] },
    Unshared,
}

pub enum ClipboardEvent {
    /// The copy's image is on the system clipboard with this ownership stamp.
    Written(Result<Stamp, String>),
    /// The system clipboard still holds our last copy's pixels for document `.0`.
    #[cfg(target_os = "linux")]
    StillOurs(u64),
    /// The image another app put on the clipboard, as an asset for document
    /// `.0`; `None` when there is none.
    Read(u64, Result<Option<(AssetId, Asset)>, String>),
}

enum Job {
    Write(CopiedPixels),
    Read {
        fit: [u32; 2],
        generation: u64,
        #[cfg(target_os = "linux")]
        expected: Option<([u32; 2], [u8; 32])>,
    },
}

pub struct Clipboard {
    to_worker: Sender<Job>,
    clip: Option<Arc<Clip>>,
    /// Images being written.
    writing: u32,
    /// Ownership stamp from when the clipboard last held this app's copy.
    ours: Option<Stamp>,
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

    #[cfg(not(target_os = "linux"))]
    pub fn paste(&mut self, canvas: &mut Canvas) {
        let holds_ours = self.writing > 0
            || self
                .ours
                .is_some_and(|ours| ours == ugu_platform::clipboard::sequence());
        match &self.clip {
            Some(clip) if holds_ours => canvas.paste(clip),
            _ => {
                let fit = canvas.session().document().canvas;
                let generation = canvas.generation();
                let _ = self.to_worker.send(Job::Read { fit, generation });
            }
        }
    }

    #[cfg(target_os = "linux")]
    pub fn paste(&mut self, canvas: &mut Canvas) {
        let immediate_ours = self.writing > 0 || self.ours == Some(Stamp::Unshared);
        if immediate_ours && let Some(clip) = &self.clip {
            canvas.paste(clip);
            return;
        }
        let expected = match (&self.clip, self.ours) {
            (Some(_), Some(Stamp::Shared { size, sha256 })) => Some((size, sha256)),
            _ => None,
        };
        let fit = canvas.session().document().canvas;
        let generation = canvas.generation();
        let _ = self.to_worker.send(Job::Read {
            fit,
            generation,
            expected,
        });
    }

    pub fn handle(&mut self, event: ClipboardEvent, canvas: &mut Canvas) {
        match event {
            ClipboardEvent::Written(result) => {
                self.writing -= 1;
                let stamp = result.unwrap_or_else(|error| {
                    tracing::warn!(%error, "the copy could not be put on the clipboard");
                    canvas.set_notice(tr("copy-not-shared").to_owned());
                    fallback_stamp()
                });
                if self.writing == 0 {
                    self.ours = Some(stamp);
                }
            }
            #[cfg(target_os = "linux")]
            ClipboardEvent::StillOurs(generation) => {
                if generation == canvas.generation()
                    && let Some(clip) = &self.clip
                {
                    canvas.paste(clip);
                }
            }
            // Read for a document no longer shown.
            ClipboardEvent::Read(generation, _) if generation != canvas.generation() => {}
            ClipboardEvent::Read(_, Ok(Some((id, asset)))) => {
                #[cfg(target_os = "linux")]
                if self.writing == 0 {
                    self.ours = None;
                }
                canvas.place_image(id, asset);
            }
            ClipboardEvent::Read(_, Ok(None)) => {
                #[cfg(target_os = "linux")]
                if self.writing == 0 {
                    self.ours = None;
                }
                canvas.set_notice(tr("paste-nothing").to_owned());
            }
            ClipboardEvent::Read(_, Err(error)) => {
                tracing::warn!(%error, "the clipboard could not be read");
                canvas.set_notice(tr("paste-failed").to_owned());
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn fallback_stamp() -> Stamp {
    ugu_platform::clipboard::sequence()
}

#[cfg(target_os = "linux")]
fn fallback_stamp() -> Stamp {
    Stamp::Unshared
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
            #[cfg(target_os = "linux")]
            let sha256 = ugu_platform::clipboard::pixel_sha256(&image);
            let drawn = started.elapsed();
            let result = ugu_platform::clipboard::write_image(image);
            #[cfg(target_os = "linux")]
            let result = result.map(|_| Stamp::Shared { size, sha256 });
            tracing::debug!(
                drawn_ms = drawn.as_secs_f64() * 1000.0,
                ms = started.elapsed().as_secs_f64() * 1000.0,
                ?size,
                "copy put on the clipboard"
            );
            ClipboardEvent::Written(result)
        }
        Job::Read {
            fit,
            generation,
            #[cfg(target_os = "linux")]
            expected,
        } => {
            let limit = usize::try_from(ugu_io::import::ALLOCATION).unwrap_or(usize::MAX);
            let encoded =
                ugu_platform::clipboard::read_encoded(limit).map_err(|error| match error {
                    ReadError::TooLarge => ugu_io::import::ImportError::TooLarge.to_string(),
                    ReadError::System(reason) => reason,
                });
            #[cfg(target_os = "linux")]
            if let (Some((expected_size, expected_sha)), Ok(Some(Encoded::Png(bytes)))) =
                (expected, &encoded)
                && ugu_platform::clipboard::png_pixel_sha256(bytes, expected_size)
                    == Some(expected_sha)
            {
                tracing::debug!(
                    ms = started.elapsed().as_secs_f64() * 1000.0,
                    "clipboard holds our copy"
                );
                return ClipboardEvent::StillOurs(generation);
            }
            let result = encoded.and_then(|image| {
                image
                    .map(|image| {
                        match image {
                            Encoded::Png(file) | Encoded::Tiff(file) => {
                                ugu_io::import::decode(std::io::Cursor::new(file), fit)
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
            #[cfg(target_os = "linux")]
            expected: None,
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

    #[cfg(target_os = "linux")]
    #[test]
    fn external_clipboard_contents_clear_shared_ownership() {
        let mut clipboard = Clipboard::new(|_| {});
        let mut canvas = Canvas::new(Document::new([8, 8]), |_| {});
        let generation = canvas.generation();
        clipboard.writing = 1;
        clipboard.handle(
            ClipboardEvent::Written(Ok(Stamp::Shared {
                size: [8, 8],
                sha256: [0xAB; 32],
            })),
            &mut canvas,
        );
        assert!(matches!(clipboard.ours, Some(Stamp::Shared { .. })));

        // When the user copies non-image content (such as plain text) in
        // another app, `Read(_, Ok(None))` clears `ours` and shows `paste-nothing`.
        clipboard.handle(ClipboardEvent::Read(generation, Ok(None)), &mut canvas);
        assert_eq!(clipboard.ours, None);
        assert_eq!(canvas.notice(), Some(tr("paste-nothing")));
    }
}
