// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! New, open, save, save as and close, and the question before unsaved
//! changes are dropped.
//!
//! Dialogs run on threads of their own; reading and writing run in order on
//! one file thread, so a later save never finishes before an earlier one to
//! the same file. A save writes the snapshot taken when it started, and the
//! document counts as saved only in that state.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};

use ugu_core::document::Document;
use ugu_core::history::StateId;
use ugu_win::dialog::{Dialog, FileType};

use crate::canvas::Canvas;

const DOCUMENT_TYPE: FileType = FileType {
    name: "Ugurugu document",
    extension: "ugu2",
};
/// 2.2.13's new document.
const NEW_CANVAS: [u32; 2] = [1024, 768];

/// What finished off the render thread.
pub enum FileEvent {
    /// A dialog closed: the chosen path, or `None` when cancelled.
    Picked(Purpose, Option<PathBuf>),
    Opened(PathBuf, Result<(Document, [u8; 16]), String>),
    Saved {
        path: PathBuf,
        state: StateId,
        result: Result<(), String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Open,
    SaveAs,
}

/// What waits for unsaved changes to be dealt with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    New,
    Open,
    Close,
}

enum Job {
    Save {
        document: Arc<Document>,
        id: [u8; 16],
        path: PathBuf,
        state: StateId,
    },
    Open(PathBuf),
}

pub struct Files {
    to_worker: Sender<Job>,
    /// Sends events back to the render thread.
    events: Arc<dyn Fn(FileEvent) + Send + Sync>,
    owner: isize,
    path: Option<PathBuf>,
    id: [u8; 16],
    /// Asking whether to save before this action.
    confirm: Option<Action>,
    /// Runs once the save under way succeeds.
    after_save: Option<Action>,
    saving: bool,
    busy_dialog: bool,
    message: Option<String>,
    close: bool,
}

/// A fresh document id, from the same entropy as hash seeds.
fn new_id() -> [u8; 16] {
    use std::hash::{BuildHasher, RandomState};
    let state = RandomState::new();
    let high = state.hash_one(1u8).to_le_bytes();
    let low = state.hash_one(2u8).to_le_bytes();
    let mut id = [0; 16];
    id[..8].copy_from_slice(&high);
    id[8..].copy_from_slice(&low);
    // RFC 9562 version 4, variant 1.
    id[6] = (id[6] & 0x0f) | 0x40;
    id[8] = (id[8] & 0x3f) | 0x80;
    id
}

impl Files {
    /// `owner` is the window the dialogs belong to.
    pub fn new(owner: isize, events: impl Fn(FileEvent) + Send + Sync + 'static) -> Self {
        let events: Arc<dyn Fn(FileEvent) + Send + Sync> = Arc::new(events);
        let (to_worker, jobs) = channel::<Job>();
        let worker_events = events.clone();
        std::thread::Builder::new()
            .name("files".to_owned())
            .spawn(move || {
                for job in jobs {
                    worker_events(run(job));
                }
            })
            .expect("cannot start the file thread");
        Self {
            to_worker,
            events,
            owner,
            path: None,
            id: new_id(),
            confirm: None,
            after_save: None,
            saving: false,
            busy_dialog: false,
            message: None,
            close: false,
        }
    }

    pub fn new_canvas() -> Document {
        Document::new(NEW_CANVAS)
    }

    /// Whether the window should close now.
    pub fn should_close(&self) -> bool {
        self.close
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// The window title: file name, a mark for unsaved changes.
    pub fn title(&self, canvas: &Canvas) -> String {
        let name = self.display_name();
        let mark = if canvas.session().is_dirty() { "*" } else { "" };
        format!("{name}{mark} - Ugurugu")
    }

    fn display_name(&self) -> String {
        self.path.as_deref().and_then(Path::file_stem).map_or_else(
            || "Untitled".to_owned(),
            |name| name.to_string_lossy().into_owned(),
        )
    }

    /// Starts `action`, first asking about unsaved changes.
    pub fn request(&mut self, action: Action, canvas: &mut Canvas) {
        if self.saving || self.busy_dialog {
            return;
        }
        if canvas.session().is_dirty() {
            self.confirm = Some(action);
        } else {
            self.proceed(action, canvas);
        }
    }

    fn proceed(&mut self, action: Action, canvas: &mut Canvas) {
        match action {
            Action::New => {
                canvas.replace(Self::new_canvas(), false);
                self.path = None;
                self.id = new_id();
                self.message = None;
            }
            Action::Open => self.pick(Purpose::Open, Dialog::Open),
            Action::Close => self.close = true,
        }
    }

    /// Opens `path` without a dialog, as for a file given at start.
    pub fn open_path(&mut self, path: PathBuf) {
        let _ = self.to_worker.send(Job::Open(path));
    }

    pub fn save(&mut self, canvas: &mut Canvas) {
        match self.path.clone() {
            Some(path) => self.start_save(path, canvas),
            None => self.save_as(),
        }
    }

    pub fn save_as(&mut self) {
        let name = format!("{}.{}", self.display_name(), DOCUMENT_TYPE.extension);
        self.pick(Purpose::SaveAs, Dialog::Save { name: &name });
    }

    /// Shows a dialog on a thread of its own.
    pub fn pick(&mut self, purpose: Purpose, dialog: Dialog<'_>) {
        if self.busy_dialog {
            return;
        }
        self.busy_dialog = true;
        let owner = self.owner;
        let events = self.events.clone();
        let (file_type, save_name) = match dialog {
            Dialog::Save { name } => (DOCUMENT_TYPE, Some(name.to_owned())),
            Dialog::Open => (DOCUMENT_TYPE, None),
        };
        std::thread::Builder::new()
            .name("file dialog".to_owned())
            .spawn(move || {
                let dialog = match &save_name {
                    Some(name) => Dialog::Save { name },
                    None => Dialog::Open,
                };
                let path =
                    ugu_win::dialog::pick(owner, dialog, file_type).unwrap_or_else(|error| {
                        tracing::error!(%error, "the file dialog failed");
                        None
                    });
                events(FileEvent::Picked(purpose, path));
            })
            .expect("cannot start the dialog thread");
    }

    fn start_save(&mut self, path: PathBuf, canvas: &mut Canvas) {
        self.saving = true;
        self.message = Some(format!("Saving {}...", path.display()));
        let _ = self.to_worker.send(Job::Save {
            document: canvas.snapshot_now(),
            id: self.id,
            path,
            state: canvas.session().state(),
        });
    }

    /// Takes an event from a dialog or the file thread.
    pub fn handle(&mut self, event: FileEvent, canvas: &mut Canvas) {
        match event {
            FileEvent::Picked(purpose, path) => {
                self.busy_dialog = false;
                let Some(path) = path else {
                    self.after_save = None;
                    return;
                };
                match purpose {
                    Purpose::Open => self.open_path(path),
                    Purpose::SaveAs => self.start_save(path, canvas),
                }
            }
            FileEvent::Opened(path, result) => match result {
                Ok((document, id)) => match ugu_render::document::check(&document) {
                    Ok(()) => {
                        canvas.replace(document, true);
                        self.path = Some(path);
                        self.id = id;
                        self.message = None;
                    }
                    Err(missing) => {
                        self.message = Some(format!(
                            "{} uses {missing}, which this build cannot show yet",
                            path.display()
                        ));
                    }
                },
                Err(error) => self.message = Some(format!("{}: {error}", path.display())),
            },
            FileEvent::Saved {
                path,
                state,
                result,
            } => {
                self.saving = false;
                match result {
                    Ok(()) => {
                        canvas.mark_saved(state);
                        self.message = Some(format!("Saved {}", path.display()));
                        self.path = Some(path);
                        if let Some(action) = self.after_save.take() {
                            self.proceed(action, canvas);
                        }
                    }
                    Err(error) => {
                        self.after_save = None;
                        self.message = Some(format!("Not saved: {error}"));
                    }
                }
            }
        }
    }

    /// The question about unsaved changes, while one is open.
    pub fn confirm(&mut self, ctx: &egui::Context, canvas: &mut Canvas) {
        let Some(action) = self.confirm else {
            return;
        };
        let name = self.display_name();
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::new("unsaved changes")).show(ctx, |ui| {
            ui.heading(format!("Save changes to {name}?"));
            ui.label("Your changes will be lost if you don't save them.");
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    choice = Some(true);
                }
                if ui.button("Don't save").clicked() {
                    choice = Some(false);
                }
                if ui.button("Cancel").clicked() {
                    self.confirm = None;
                }
            });
        });
        if modal.should_close() {
            self.confirm = None;
        }
        match choice {
            Some(true) => {
                self.confirm = None;
                self.after_save = Some(action);
                self.save(canvas);
            }
            Some(false) => {
                self.confirm = None;
                self.proceed(action, canvas);
            }
            None => {}
        }
    }
}

fn run(job: Job) -> FileEvent {
    match job {
        Job::Save {
            document,
            id,
            path,
            state,
        } => {
            let started = std::time::Instant::now();
            let result = ugu_io::save::save(&document, id, &path, &ugu_win::file::replace_file)
                .map_err(|error| error.to_string());
            tracing::info!(
                ms = started.elapsed().as_secs_f64() * 1000.0,
                ok = result.is_ok(),
                "document saved"
            );
            FileEvent::Saved {
                path,
                state,
                result,
            }
        }
        Job::Open(path) => {
            let started = std::time::Instant::now();
            let result = std::fs::File::open(&path)
                .map_err(|error| error.to_string())
                .and_then(|file| {
                    ugu_io::read::read(std::io::BufReader::new(file))
                        .map_err(|error| error.to_string())
                });
            tracing::info!(
                ms = started.elapsed().as_secs_f64() * 1000.0,
                ok = result.is_ok(),
                "document opened"
            );
            FileEvent::Opened(path, result)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{Receiver, channel};
    use std::time::Duration;
    use ugu_core::document::LayerKind;
    use ugu_core::ops::Blend;
    use ugu_session::Session;

    fn setup() -> (Files, Canvas, Receiver<FileEvent>) {
        let (sender, events) = channel();
        let files = Files::new(0, move |event| {
            let _ = sender.send(event);
        });
        let canvas = Canvas::new(Files::new_canvas(), |_| {});
        (files, canvas, events)
    }

    fn next(events: &Receiver<FileEvent>) -> FileEvent {
        events
            .recv_timeout(Duration::from_secs(10))
            .expect("the file thread answers")
    }

    fn folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("ugurugu-files-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn layers_in(path: &Path) -> usize {
        let file = std::fs::File::open(path).unwrap();
        ugu_io::read::read(std::io::BufReader::new(file))
            .unwrap()
            .0
            .layers
            .len()
    }

    #[test]
    fn a_save_writes_the_document_as_it_was_when_it_started() {
        let (mut files, mut canvas, events) = setup();
        let path = folder("snapshot").join("drawing.ugu2");
        canvas.edit(Session::add_layer).unwrap();
        files.path = Some(path.clone());
        files.save(&mut canvas);
        // An edit while the file thread writes.
        canvas.edit(Session::add_layer).unwrap();
        files.handle(next(&events), &mut canvas);
        assert_eq!(layers_in(&path), 2);
        assert_eq!(canvas.session().document().layers.len(), 3);
        assert!(canvas.session().is_dirty());
        canvas.edit(Session::undo).unwrap();
        assert!(!canvas.session().is_dirty());
        assert_eq!(files.title(&canvas), "drawing - Ugurugu");
    }

    #[test]
    fn a_failed_save_keeps_the_changes_unsaved_and_says_why() {
        let (mut files, mut canvas, events) = setup();
        canvas.edit(Session::add_layer).unwrap();
        files.path = Some(
            folder("missing")
                .join("no such folder")
                .join("drawing.ugu2"),
        );
        files.after_save = Some(Action::New);
        files.save(&mut canvas);
        files.handle(next(&events), &mut canvas);
        assert!(canvas.session().is_dirty());
        assert!(files.message().unwrap().starts_with("Not saved"));
        // The new document that waited for the save did not replace this one.
        assert_eq!(canvas.session().document().layers.len(), 2);
    }

    #[test]
    fn opening_replaces_the_document_or_says_what_cannot_be_shown() {
        let (mut files, mut canvas, events) = setup();
        let folder = folder("open");
        let mut document = Files::new_canvas();
        document.frames = 12;
        let good = folder.join("good.ugu2");
        ugu_io::save::save(&document, [7; 16], &good, &ugu_win::file::replace_file).unwrap();
        if let LayerKind::Paint(paint) = &mut document.layers[0].kind {
            paint.blend = Blend::Multiply;
        }
        let unsupported = folder.join("multiply.ugu2");
        ugu_io::save::save(
            &document,
            [8; 16],
            &unsupported,
            &ugu_win::file::replace_file,
        )
        .unwrap();

        files.open_path(unsupported);
        files.handle(next(&events), &mut canvas);
        assert!(files.message().unwrap().contains("cannot show yet"));
        assert_eq!(canvas.session().document().frames, 30);

        files.open_path(good.clone());
        files.handle(next(&events), &mut canvas);
        assert_eq!(canvas.session().document().frames, 12);
        assert!(!canvas.session().is_dirty());
        assert_eq!(files.path, Some(good));
        assert_eq!(files.id, [7; 16]);
    }

    #[test]
    fn a_new_id_is_a_version_4_uuid_and_differs_each_time() {
        let (a, b) = (new_id(), new_id());
        assert_ne!(a, b);
        assert_eq!(a[6] >> 4, 4);
        assert_eq!(a[8] >> 6, 0b10);
    }
}
