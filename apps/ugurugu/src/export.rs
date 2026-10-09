// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Exporting. Each export runs on a thread of its own, so saves and
//! recovery writes are not held up behind it, and editing goes on while it
//! runs. It renders the document as it was when the export started.
//! Cancelling answers at once; the thread stops at its next step and never
//! puts its file in place after that, so the target stays as it was.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use ugu_core::document::Document;
use ugu_io::image::Format;

use crate::cache::Renders;

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Done,
    Cancelled,
    Failed(String),
}

/// An export under way.
pub struct Exporting {
    /// Tells this export's answer from an earlier, cancelled one's.
    pub number: u64,
    cancel: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

impl Exporting {
    /// Exports `frame` of `document` to `path`, in the format its extension
    /// names (PNG for any other), and hands the outcome to `done`.
    pub fn start(
        number: u64,
        document: Arc<Document>,
        frame: i64,
        path: PathBuf,
        renders: Renders,
        done: impl FnOnce(Outcome) + Send + 'static,
    ) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let thread = std::thread::Builder::new()
            .name("export".to_owned())
            .spawn({
                let cancel = cancel.clone();
                move || {
                    let started = std::time::Instant::now();
                    let outcome = still(document, frame, &path, &renders, &cancel);
                    tracing::info!(
                        ms = started.elapsed().as_secs_f64() * 1000.0,
                        ?outcome,
                        "frame exported"
                    );
                    done(outcome);
                }
            })
            .expect("cannot start the export thread");
        Self {
            number,
            cancel,
            thread,
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_finished(&self) -> bool {
        self.thread.is_finished()
    }

    /// Waits for the thread, which removes its unfinished file.
    pub fn join(self) {
        let _ = self.thread.join();
    }
}

/// Renders `frame` without reference layers at the document's size, on the
/// render worker after the canvas's own work, and writes it to `path`.
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
    // Asked again just before the file is put in place.
    let replace = |written: &Path, target: &Path| {
        if cancelled() {
            return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
        }
        ugu_win::file::replace_file(written, target)
    };
    match ugu_io::image::export(pixmap.data_as_u8_slice(), size, format, path, &replace) {
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

    #[test]
    fn a_failed_export_says_why() {
        let target = folder("failed").join("no such folder").join("frame.png");
        assert!(matches!(
            export(document([200, 100, 50, 255]), &target, false),
            Outcome::Failed(_)
        ));
    }
}
