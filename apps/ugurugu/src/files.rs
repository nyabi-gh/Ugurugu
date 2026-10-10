// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! New, open, save, save as, close and inserting an image, and the
//! question before unsaved changes are dropped.
//!
//! Dialogs run on threads of their own; reading and writing run in order on
//! one file thread, so a later save never finishes before an earlier one to
//! the same file. A save writes the snapshot taken when it started, and the
//! document counts as saved only in that state. An export renders the frame
//! shown when it started, on a thread of its own (`export`).
//!
//! Unsaved work is also written for automatic recovery on the file thread,
//! in order with saves, so a write queued before a save or a clean close
//! never brings back what they removed.

use std::collections::VecDeque;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ugu_core::document::{Document, LayerId, limits};
use ugu_core::history::StateId;
use ugu_core::ops::{Affine, AssetId};
use ugu_core::store::Asset;
use ugu_session::{Session, ToolSettings};
use ugu_win::dialog::{Dialog, FileType};

use crate::canvas::Canvas;
use crate::export::{self, Animation, Exporting, Job as ExportJob, Outcome};
use crate::i18n::{tr, tr_with};
use crate::recovery::{self, Found, Meta};
use crate::settings::preset::{self, PresetError};

fn args_name(name: String) -> fluent_bundle::FluentArgs<'static> {
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("name", name);
    args
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

fn args_path(path: &Path) -> fluent_bundle::FluentArgs<'static> {
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("path", path.display().to_string());
    args
}

const DOCUMENT_TYPE: FileType = FileType {
    name: "Ugurugu document",
    extensions: &["ugurugu"],
};
const PNG_TYPE: FileType = FileType {
    name: "PNG image",
    extensions: &["png"],
};
const JPEG_TYPE: FileType = FileType {
    name: "JPEG image",
    extensions: &["jpg", "jpeg"],
};
/// What a frame is exported as, as 2.2.13 offers; the extension picks it.
const STILL_TYPES: &[FileType] = &[PNG_TYPE, JPEG_TYPE];
const GIF_TYPE: FileType = FileType {
    name: "GIF image",
    extensions: &["gif"],
};
const WEBP_TYPE: FileType = FileType {
    name: "WebP image",
    extensions: &["webp"],
};
const TOOLS_TYPE: FileType = FileType {
    name: "Ugurugu tool preset",
    extensions: &[preset::EXTENSION],
};
/// What an image can be inserted from, as 2.2.13 offers.
const IMAGE_TYPE: FileType = FileType {
    name: "Image",
    extensions: &["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"],
};
/// 2.2.13's new document.
const NEW_CANVAS: [u32; 2] = [1024, 768];
/// How long a save or export runs before the status bar says it is running.
const PROGRESS_DELAY: Duration = Duration::from_millis(300);

/// What finished off the render thread.
pub enum FileEvent {
    /// A dialog closed: the chosen path, or `None` when cancelled.
    Picked(Purpose, Option<PathBuf>),
    Opened(PathBuf, Result<(Box<Document>, [u8; 16]), String>),
    Saved {
        path: PathBuf,
        state: StateId,
        result: Result<(), String>,
    },
    /// How export `number` ended.
    Exported {
        number: u64,
        path: PathBuf,
        outcome: Outcome,
    },
    /// An image read to insert, as an asset.
    Inserted(PathBuf, Result<(AssetId, Asset), String>),
    /// A tool preset file read.
    ToolsRead(PathBuf, Result<Vec<u8>, PresetError>),
    ToolsWritten(PathBuf, Result<(), String>),
    /// This window's recovery folder is locked, and the folders of windows
    /// no longer running were found.
    RecoveryBegun(Result<(File, Vec<Found>), String>),
    RecoveryWritten(Result<(), String>),
    /// Work read back from a folder found at start.
    Recovered(Result<Box<Recovered>, String>),
}

/// Work read back, and the folder it came from.
pub struct Recovered {
    found: Found,
    document: Document,
    id: [u8; 16],
    meta: Meta,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Open,
    SaveAs,
    ExportImage,
    ExportAnimation,
    InsertImage,
    ImportTools,
    ExportTools,
    /// The default save folder, for the settings.
    SaveFolder,
}

/// The animation export options being chosen.
struct AnimationDialog {
    format: Animation,
    /// Into `export::sizes` of the document.
    size: usize,
    keep_transparency: bool,
    budget: u64,
}

/// What becomes of work left by a window no longer running.
#[derive(Clone, Copy)]
enum Answer {
    Recover,
    Discard,
    Later,
}

/// What waits for unsaved changes to be dealt with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    New,
    Open,
    Close,
    /// Recovers the work first in `Files::found`.
    Recover,
}

enum Job {
    Save {
        document: Arc<Document>,
        id: [u8; 16],
        path: PathBuf,
        state: StateId,
    },
    Open(PathBuf),
    /// Reads an image no larger than `fit`.
    Insert {
        path: PathBuf,
        fit: [u32; 2],
    },
    ReadTools(PathBuf),
    WriteTools {
        path: PathBuf,
        bytes: Vec<u8>,
    },
    BeginRecovery {
        root: PathBuf,
        folder: PathBuf,
    },
    WriteRecovery {
        folder: PathBuf,
        document: Arc<Document>,
        id: [u8; 16],
        meta: Meta,
    },
    ForgetRecovery(PathBuf),
    ReadRecovery(Box<Found>),
    DiscardRecovery(Box<Found>),
    /// Removes this window's folder as it closes.
    EndRecovery {
        folder: PathBuf,
        lock: File,
    },
    /// Ends the file thread once the jobs before it are done.
    Stop,
}

/// What a recovery write was of: the document's revision and whatever is
/// pending outside it, which changes without a new revision.
#[derive(Clone, Debug, PartialEq)]
struct Mark {
    revision: u64,
    transform: Option<(LayerId, Affine, bool)>,
    /// Where, which layout, and what the text is drawn with.
    text: Option<(LayerId, [f64; 2], usize, ToolSettings, bool)>,
}

impl Mark {
    fn of(session: &Session) -> Self {
        Self {
            revision: session.revision(),
            transform: session
                .pending()
                .map(|pending| (pending.layer, pending.transform, pending.keep_source)),
            text: session.placed_text().map(|placed| {
                (
                    placed.layer,
                    placed.at,
                    Arc::as_ptr(&placed.outline) as usize,
                    session.pen,
                    session.text.filled,
                )
            }),
        }
    }
}

/// This window's automatic recovery.
struct Recovering {
    folder: PathBuf,
    /// Held from when the folder is made until the window closes.
    lock: Option<File>,
    sequence: u64,
    /// Whether a generation may be on disk.
    stored: bool,
    /// The work as last written.
    written: Option<Mark>,
    /// Since when the work shown is not on disk, when it last changed, and
    /// how it is now.
    unwritten: Option<(Instant, Instant, Mark)>,
    writing: bool,
    /// Whether the last write failed, so the failure is said once.
    failing: bool,
}

impl Recovering {
    fn due(&self) -> Option<Instant> {
        let (since, last, _) = self.unwritten.as_ref()?;
        (!self.writing).then(|| (*last + recovery::QUIET).min(*since + recovery::LONGEST))
    }
}

pub struct Files {
    to_worker: Sender<Job>,
    worker: Option<JoinHandle<()>>,
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
    /// The default save folder from the settings, `None` for Documents.
    default_save_folder: Option<PathBuf>,
    /// A default save folder chosen, for the settings to take.
    chosen_save_folder: Option<PathBuf>,
    /// The long edge animations were last exported at, from the settings.
    export_edge: Option<u32>,
    /// One chosen since, for the settings.
    chosen_export_edge: Option<u32>,
    /// What the status bar says.
    message: Option<String>,
    /// What it says instead from then on, unless something else is said first.
    upcoming: Option<(String, Instant)>,
    close: bool,
    recovery: Option<Recovering>,
    /// Work of windows no longer running, to ask about in turn.
    found: VecDeque<Found>,
    /// Asking about recovered work is waiting for it to be read.
    reading_found: bool,
    /// The name recovered work goes by until it is saved, and where the
    /// work had been saved, if anywhere.
    recovered_name: Option<String>,
    recovered_from: Option<PathBuf>,
    exporting: Option<Exporting>,
    /// The animation export options asked for, while they are.
    animation_dialog: Option<AnimationDialog>,
    /// The new document's size, while it is asked for.
    new_document: Option<[i64; 2]>,
    /// The animation export chosen, waiting for where it goes.
    animation_job: Option<ExportJob>,
    /// Cancelled exports whose threads may still be finishing.
    cancelled: Vec<Exporting>,
    exports: u64,
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
        let worker = std::thread::Builder::new()
            .name("files".to_owned())
            .spawn(move || {
                for job in jobs {
                    if let Job::Stop = job {
                        break;
                    }
                    if let Some(event) = run(job) {
                        worker_events(event);
                    }
                }
            })
            .expect("cannot start the file thread");
        Self {
            to_worker,
            worker: Some(worker),
            events,
            owner,
            path: None,
            id: new_id(),
            confirm: None,
            after_save: None,
            saving: false,
            busy_dialog: false,
            default_save_folder: None,
            chosen_save_folder: None,
            export_edge: None,
            chosen_export_edge: None,
            message: None,
            upcoming: None,
            close: false,
            recovery: None,
            found: VecDeque::new(),
            reading_found: false,
            recovered_name: None,
            recovered_from: None,
            exporting: None,
            animation_dialog: None,
            new_document: None,
            animation_job: None,
            cancelled: Vec::new(),
            exports: 0,
        }
    }

    /// Starts keeping unsaved work under `root`, in a folder of this
    /// window's, and looks for work left by windows no longer running.
    pub fn start_recovery(&mut self, root: Option<PathBuf>) {
        let Some(root) = root else {
            tracing::warn!("no folder for automatic recovery; unsaved work is not kept");
            return;
        };
        let name: String = new_id().iter().map(|byte| format!("{byte:02x}")).collect();
        let folder = root.join(name);
        let _ = self.to_worker.send(Job::BeginRecovery {
            root,
            folder: folder.clone(),
        });
        self.recovery = Some(Recovering {
            folder,
            lock: None,
            sequence: 0,
            stored: false,
            written: None,
            unwritten: None,
            writing: false,
            failing: false,
        });
    }

    /// Writes the unsaved work once editing has paused, or has gone on for
    /// long, and removes what was written once the work is saved.
    pub fn keep_recovery(&mut self, canvas: &mut Canvas, now: Instant) {
        let Some(recovering) = self.recovery.as_mut() else {
            return;
        };
        let session = canvas.session();
        if !session.is_dirty() {
            recovering.unwritten = None;
            recovering.written = None;
            if std::mem::take(&mut recovering.stored) {
                let _ = self
                    .to_worker
                    .send(Job::ForgetRecovery(recovering.folder.clone()));
            }
            return;
        }
        let mark = Mark::of(session);
        if recovering.written.as_ref() == Some(&mark) {
            recovering.unwritten = None;
            return;
        }
        match &mut recovering.unwritten {
            Some((_, last, seen)) => {
                if *seen != mark {
                    *last = now;
                    *seen = mark;
                }
            }
            None => recovering.unwritten = Some((now, now, mark)),
        }
        if recovering.due().is_some_and(|due| due <= now) {
            self.write_recovery(canvas);
        }
    }

    /// When unsaved work is next due to be written.
    pub fn recovery_due(&self) -> Option<Instant> {
        self.recovery.as_ref()?.due()
    }

    /// Writes the work as saving would, with a pending transform or placed
    /// text in it, leaving them pending.
    fn write_recovery(&mut self, canvas: &mut Canvas) {
        let name = self.display_name();
        let path = self.path.clone();
        let Some(recovering) = self.recovery.as_mut() else {
            return;
        };
        let mark = Mark::of(canvas.session());
        let document = match canvas.session().settled_document() {
            Some(document) => Arc::new(document),
            None => canvas.snapshot_now(),
        };
        recovering.sequence += 1;
        let meta = Meta::new(recovering.sequence, name, path, document.canvas);
        let _ = self.to_worker.send(Job::WriteRecovery {
            folder: recovering.folder.clone(),
            document,
            id: self.id,
            meta,
        });
        recovering.writing = true;
        recovering.stored = true;
        recovering.written = Some(mark);
        recovering.unwritten = None;
    }

    /// Removes this window's recovery folder and waits for the file thread
    /// to finish, as the window closes normally.
    pub fn end(&mut self) {
        if let Some(Recovering {
            folder,
            lock: Some(lock),
            ..
        }) = self.recovery.take()
        {
            let _ = self.to_worker.send(Job::EndRecovery { folder, lock });
        }
        // Left for the next start.
        self.found.clear();
        // Their unfinished files are removed before the process ends.
        for export in self
            .exporting
            .take()
            .into_iter()
            .chain(self.cancelled.drain(..))
        {
            export.cancel();
            export.join();
        }
        let _ = self.to_worker.send(Job::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
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
        match &self.upcoming {
            Some((text, from)) if *from <= Instant::now() => Some(text),
            _ => self.message.as_deref(),
        }
    }

    /// When the upcoming message is due.
    pub fn message_due(&self) -> Option<Instant> {
        self.upcoming
            .as_ref()
            .map(|(_, from)| *from)
            .filter(|from| *from > Instant::now())
    }

    fn say(&mut self, text: String) {
        self.message = Some(text);
        self.upcoming = None;
    }

    fn clear_message(&mut self) {
        self.message = None;
        self.upcoming = None;
    }

    /// Says what is under way only if it is still under way after a moment,
    /// so a quick save does not flash its progress.
    fn say_if_slow(&mut self, text: String) {
        self.upcoming = Some((text, Instant::now() + PROGRESS_DELAY));
    }

    /// The window title: file name, a mark for unsaved changes.
    pub fn title(&self, canvas: &Canvas) -> String {
        let name = self.display_name();
        let mark = if canvas.session().is_dirty() { "*" } else { "" };
        format!("{name}{mark} - Ugurugu")
    }

    fn display_name(&self) -> String {
        match self.path.as_deref().and_then(Path::file_stem) {
            Some(name) => name.to_string_lossy().into_owned(),
            None => self
                .recovered_name
                .clone()
                .unwrap_or_else(|| "Untitled".to_owned()),
        }
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
            // As 2.2.13, the size is asked for after unsaved changes are,
            // starting from the document's.
            Action::New => {
                self.new_document = Some(canvas.session().document().canvas.map(i64::from));
            }
            Action::Open => self.pick(Purpose::Open, Dialog::Open(DOCUMENT_TYPE)),
            Action::Close => self.close = true,
            Action::Recover => {
                if let Some(found) = self.found.pop_front() {
                    self.reading_found = true;
                    let _ = self.to_worker.send(Job::ReadRecovery(Box::new(found)));
                }
            }
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
        self.pick_save(Purpose::SaveAs, &[DOCUMENT_TYPE]);
    }

    /// Where new documents are first saved and exported, from the settings.
    pub fn set_default_save_folder(&mut self, folder: Option<PathBuf>) {
        self.default_save_folder = folder;
    }

    /// Asks for a new default save folder, starting in the current one.
    pub fn choose_save_folder(&mut self) {
        self.pick(
            Purpose::SaveFolder,
            Dialog::Folder {
                title: tr("settings-choose-folder").to_owned(),
                start: None,
            },
        );
    }

    /// The default save folder the user chose since last asked.
    pub fn take_chosen_save_folder(&mut self) -> Option<PathBuf> {
        self.chosen_save_folder.take()
    }

    pub fn set_export_edge(&mut self, edge: Option<u32>) {
        self.export_edge = edge;
    }

    pub fn take_chosen_export_edge(&mut self) -> Option<u32> {
        self.chosen_export_edge.take()
    }

    /// Saves as `file_type` under the document's name, in its folder, or in
    /// the default save folder for a document never saved, as 2.2.13 does.
    fn pick_save(&mut self, purpose: Purpose, file_types: &'static [FileType]) {
        let name = format!("{}.{}", self.display_name(), file_types[0].extensions[0]);
        let folder = self
            .path
            .as_deref()
            .or(self.recovered_from.as_deref())
            .and_then(std::path::Path::parent)
            .map(std::path::Path::to_path_buf);
        self.pick(
            purpose,
            Dialog::Save {
                file_types,
                name,
                folder,
            },
        );
    }

    /// Shows a dialog on a thread of its own.
    pub fn pick(&mut self, purpose: Purpose, mut dialog: Dialog) {
        if self.busy_dialog {
            return;
        }
        self.busy_dialog = true;
        let owner = self.owner;
        let events = self.events.clone();
        let default_save_folder = self.default_save_folder.clone();
        std::thread::Builder::new()
            .name("file dialog".to_owned())
            .spawn(move || {
                // Looking at the disk is left to this thread.
                match &mut dialog {
                    Dialog::Save { folder, .. } if folder.is_none() => {
                        *folder = crate::settings::save_folder(default_save_folder.as_deref());
                    }
                    Dialog::Folder { start, .. } => {
                        *start = crate::settings::save_folder(default_save_folder.as_deref());
                    }
                    _ => {}
                }
                let path = ugu_win::dialog::pick(owner, &dialog).unwrap_or_else(|error| {
                    tracing::error!(%error, "the file dialog failed");
                    None
                });
                events(FileEvent::Picked(purpose, path));
            })
            .expect("cannot start the dialog thread");
    }

    pub fn insert_image(&mut self) {
        self.pick(Purpose::InsertImage, Dialog::Open(IMAGE_TYPE));
    }

    /// Asks which tool preset to take the tools and wobble from.
    pub fn import_tools(&mut self) {
        self.pick(Purpose::ImportTools, Dialog::Open(TOOLS_TYPE));
    }

    /// Asks where to write the tools and wobble as a preset.
    pub fn export_tools(&mut self) {
        self.pick_save(Purpose::ExportTools, &[TOOLS_TYPE]);
    }

    /// Asks where to export the frame shown, one export at a time.
    pub fn export_image(&mut self) {
        if self.exporting.is_none() {
            self.pick_save(Purpose::ExportImage, STILL_TYPES);
        }
    }

    /// Asks how to export the animation as `format`, then where; one export
    /// at a time.
    pub fn export_animation(&mut self, canvas: &Canvas, format: Animation) {
        if self.exporting.is_some() || self.busy_dialog {
            return;
        }
        let document = canvas.session().document();
        let budget = crate::budget::Budget::now(0).render as u64;
        // The size last chosen, or the next smaller one that fits.
        let sizes = export::sizes(document.canvas);
        let preferred = export::nearest(
            document.canvas,
            &sizes,
            self.export_edge.unwrap_or(export::FIRST_EDGE),
        );
        let size = (preferred..sizes.len())
            .find(|&index| {
                export::threads_within(
                    format,
                    document.canvas,
                    sizes[index],
                    document.frames,
                    budget,
                )
                .is_some()
            })
            .unwrap_or(sizes.len() - 1);
        self.animation_dialog = Some(AnimationDialog {
            format,
            size,
            keep_transparency: true,
            budget,
        });
    }

    /// Starts a new document of `size`.
    fn start_new(&mut self, canvas: &mut Canvas, size: [u32; 2]) {
        // Clean until edited, as the document at start.
        canvas.replace(Document::new(size), true);
        self.path = None;
        self.recovered_name = None;
        self.recovered_from = None;
        self.id = new_id();
        self.clear_message();
    }

    /// The new document's size, while it is asked for: any size from a pixel
    /// (2.2.13: from 64), with pixel art's usual squares a press away.
    pub fn ask_new_document(&mut self, ctx: &egui::Context, canvas: &mut Canvas) {
        let Some(size) = self.new_document.as_mut() else {
            return;
        };
        let edges = i64::from(*limits::CANVAS_EDGE.start())..=i64::from(*limits::CANVAS_EDGE.end());
        let mut chosen = None;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::new("new document")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.heading(tr("new-title"));
            ui.add_space(6.0);
            egui::Grid::new("new document size")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    for (axis, name) in [(0, "size-width"), (1, "size-height")] {
                        ui.label(tr(name));
                        crate::widgets::whole(ui, &mut size[axis], edges.clone(), false, tr(name));
                        ui.end_row();
                    }
                });
            ui.add_space(4.0);
            // Enter presses OK, unless it presses another focused button.
            let mut button_focused = false;
            ui.horizontal(|ui| {
                for edge in [16, 32, 64, 128] {
                    let button = ui.button(format!("{edge} × {edge}"));
                    button_focused |= button.has_focus();
                    if button.clicked() {
                        *size = [edge, edge];
                    }
                }
            });
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let back = ui.button(tr("dialog-cancel"));
                button_focused |= back.has_focus();
                if back.clicked() {
                    cancel = true;
                }
                let ok = ui.button(tr("dialog-ok"));
                let enter =
                    ui.input(|input| input.key_pressed(egui::Key::Enter)) && !button_focused;
                if ok.clicked() || enter {
                    chosen = Some(size.map(|edge| edge.clamp(*edges.start(), *edges.end()) as u32));
                }
            });
        });
        if let Some(size) = chosen {
            self.new_document = None;
            self.start_new(canvas, size);
        } else if cancel || modal.should_close() {
            self.new_document = None;
        }
    }

    /// The animation export options, while they are asked for.
    pub fn ask_animation(&mut self, ctx: &egui::Context, canvas: &Canvas) {
        let Some(dialog) = self.animation_dialog.as_mut() else {
            return;
        };
        let document = canvas.session().document();
        let transparent = document.background.0[3] < 255;
        let sizes = export::sizes(document.canvas);
        let threads = export::threads_within(
            dialog.format,
            document.canvas,
            sizes[dialog.size],
            document.frames,
            dialog.budget,
        );
        let fits = threads.is_some();
        let bytes = export::animation_bytes(
            dialog.format,
            document.canvas,
            sizes[dialog.size],
            document.frames,
            threads.unwrap_or(1),
        );
        let mebibytes = format!("{:.0}", bytes as f64 / (1024.0 * 1024.0));
        let original = export::original(document.canvas, &sizes);
        let label = |index: usize| {
            let [width, height] = sizes[index];
            if index == original {
                format!("{}  ({width} × {height})", tr("export-size-original"))
            } else if index < original {
                let percent = width / document.canvas[0] * 100;
                format!("{width} × {height}  ({percent}%)")
            } else {
                format!("{width} × {height}")
            }
        };
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::new("animation export")).show(ctx, |ui| {
            ui.heading(tr(match dialog.format {
                Animation::Gif => "export-gif-title",
                Animation::WebP => "export-webp-title",
            }));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(tr("export-size"));
                egui::ComboBox::from_id_salt("gif size")
                    .selected_text(label(dialog.size))
                    .show_ui(ui, |ui| {
                        for index in 0..sizes.len() {
                            crate::widgets::choice(ui, &mut dialog.size, index, label(index));
                        }
                    })
                    .response
                    .widget_info(|| {
                        let mut info = egui::WidgetInfo::labeled(
                            egui::WidgetType::ComboBox,
                            true,
                            tr("export-size"),
                        );
                        info.current_text_value = Some(label(dialog.size));
                        info
                    });
            });
            let mut keep = transparent && dialog.keep_transparency;
            let checkbox = ui.add_enabled(
                transparent,
                egui::Checkbox::new(&mut keep, tr("export-keep-transparency")),
            );
            if transparent {
                dialog.keep_transparency = keep;
            } else {
                checkbox.on_disabled_hover_text(tr("export-opaque-background"));
            }
            ui.label(if fits {
                tr_with(
                    "export-estimate",
                    &crate::i18n::args([
                        ("frames", document.frames.to_string()),
                        ("mebibytes", mebibytes.clone()),
                    ]),
                )
            } else {
                tr_with(
                    "export-over-budget",
                    &crate::i18n::args([("mebibytes", mebibytes.clone())]),
                )
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(fits, egui::Button::new(tr("export-confirm")))
                    .clicked()
                {
                    choice = Some(true);
                }
                if ui.button(tr("export-cancel")).clicked() {
                    choice = Some(false);
                }
            });
        });
        if modal.should_close() && choice.is_none() {
            choice = Some(false);
        }
        match choice {
            Some(true) => {
                let format = dialog.format;
                let [width, height] = sizes[dialog.size];
                self.chosen_export_edge = Some(width.max(height));
                self.animation_job = Some(ExportJob::Animation {
                    format,
                    size: sizes[dialog.size],
                    keep_transparency: transparent && dialog.keep_transparency,
                    threads: threads.unwrap_or(1),
                });
                self.animation_dialog = None;
                let types: &'static [FileType] = match format {
                    Animation::Gif => &[GIF_TYPE],
                    Animation::WebP => &[WEBP_TYPE],
                };
                self.pick_save(Purpose::ExportAnimation, types);
            }
            Some(false) => self.animation_dialog = None,
            None => {}
        }
    }

    /// Exports with a pending transform or placed text applied.
    fn start_export(&mut self, path: PathBuf, canvas: &mut Canvas, job: ExportJob) {
        if self.exporting.is_some() {
            return;
        }
        canvas.apply_pending();
        self.cancelled.retain(|export| !export.is_finished());
        self.exports += 1;
        let number = self.exports;
        let events = self.events.clone();
        let done_path = path.clone();
        self.exporting = Some(Exporting::start(
            number,
            canvas.snapshot_now(),
            job,
            path,
            canvas.renders(),
            move |outcome| {
                events(FileEvent::Exported {
                    number,
                    path: done_path,
                    outcome,
                });
            },
        ));
    }

    /// What the status bar shows while an export runs.
    pub fn export_status(&self) -> Option<String> {
        let export = self.exporting.as_ref()?;
        Some(if export.steps > 1 {
            format!(
                "{} {} / {}",
                tr("export-animation-running"),
                export.done(),
                export.steps
            )
        } else {
            tr("export-running").to_owned()
        })
    }

    /// Stops the export under way; says so at once.
    pub fn cancel_export(&mut self) {
        if let Some(export) = self.exporting.take() {
            export.cancel();
            self.cancelled.push(export);
            self.say(tr("export-canceled").to_owned());
        }
    }

    /// Saves with a pending transform or placed text applied, so what is
    /// saved is what is shown and the saved state is the one after it.
    fn start_save(&mut self, path: PathBuf, canvas: &mut Canvas) {
        canvas.apply_pending();
        self.saving = true;
        self.say_if_slow(format!("Saving {}...", path.display()));
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
                    self.animation_job = None;
                    return;
                };
                match purpose {
                    Purpose::Open => self.open_path(path),
                    Purpose::SaveAs => self.start_save(path, canvas),
                    Purpose::ExportImage => {
                        let frame = canvas.session().frame();
                        self.start_export(path, canvas, ExportJob::Still { frame });
                    }
                    Purpose::ExportAnimation => {
                        if let Some(job) = self.animation_job.take() {
                            self.start_export(path, canvas, job);
                        }
                    }
                    Purpose::InsertImage => {
                        let fit = canvas.session().document().canvas;
                        let _ = self.to_worker.send(Job::Insert { path, fit });
                    }
                    Purpose::ImportTools => {
                        let _ = self.to_worker.send(Job::ReadTools(path));
                    }
                    Purpose::ExportTools => {
                        let session = canvas.session();
                        let bytes = preset::write(&session.tools(), session.document().wobble);
                        let _ = self.to_worker.send(Job::WriteTools { path, bytes });
                    }
                    Purpose::SaveFolder => self.chosen_save_folder = Some(path),
                }
            }
            FileEvent::ToolsRead(path, read) => {
                let session = canvas.session();
                let taken = read.and_then(|bytes| {
                    preset::read(&bytes, session.tools(), session.document().wobble)
                });
                let failed = |reason: &str| format!("{}: {reason}", tr("preset-import-failed"));
                match taken {
                    Ok((tools, wobble)) => {
                        let applied = canvas.edit(|session| {
                            session.set_tools(tools);
                            let document = session.document();
                            let (frames, speed) = (document.frames, document.frames_per_second);
                            session.set_animation(frames, speed, wobble)
                        });
                        self.say(match applied {
                            Ok(_) => tr_with("preset-imported", &args_name(file_name(&path))),
                            Err(error) => failed(&format!("{error:?}")),
                        });
                    }
                    Err(PresetError::Unreadable(error)) => self.say(failed(&error)),
                    Err(PresetError::TooLarge) => self.say(failed(tr("preset-too-large"))),
                    Err(PresetError::NotPreset) => self.say(failed(tr("preset-not-preset"))),
                }
            }
            FileEvent::ToolsWritten(path, result) => self.say(match result {
                Ok(()) => tr_with("preset-exported", &args_name(file_name(&path))),
                Err(error) => format!("{}: {error}", tr("preset-export-failed")),
            }),
            FileEvent::Opened(path, result) => match result {
                Ok((document, id)) => {
                    canvas.replace(*document, true);
                    self.path = Some(path);
                    self.recovered_name = None;
                    self.recovered_from = None;
                    self.id = id;
                    self.clear_message();
                }
                Err(error) => self.say(format!("{}: {error}", path.display())),
            },
            FileEvent::Inserted(path, result) => match result {
                Ok((id, asset)) => {
                    canvas.place_image(id, asset);
                    self.say(tr_with("image-inserted", &args_name(file_name(&path))));
                }
                Err(error) => {
                    self.say(format!("{}: {error}", tr("insert-image-failed")));
                }
            },
            FileEvent::Exported {
                number,
                path,
                outcome,
            } => {
                // A cancelled one was answered when it was cancelled.
                if self
                    .exporting
                    .as_ref()
                    .is_none_or(|export| export.number != number)
                {
                    return;
                }
                self.exporting = None;
                self.say(match outcome {
                    Outcome::Done => tr_with("export-done", &args_path(&path)),
                    Outcome::Cancelled => tr("export-canceled").to_owned(),
                    Outcome::Failed(error) => format!("{}: {error}", tr("export-failed")),
                });
            }
            FileEvent::Saved {
                path,
                state,
                result,
            } => {
                self.saving = false;
                match result {
                    Ok(()) => {
                        canvas.mark_saved(state);
                        self.say(format!("Saved {}", path.display()));
                        self.path = Some(path);
                        if let Some(action) = self.after_save.take() {
                            self.proceed(action, canvas);
                        }
                    }
                    Err(error) => {
                        self.after_save = None;
                        self.say(format!("Not saved: {error}"));
                    }
                }
            }
            FileEvent::RecoveryBegun(result) => match result {
                Ok((lock, found)) => {
                    if let Some(recovering) = self.recovery.as_mut() {
                        recovering.lock = Some(lock);
                    }
                    self.found = found.into();
                }
                Err(error) => {
                    tracing::error!(%error, "cannot start automatic recovery");
                    self.recovery = None;
                    self.say(tr("recovery-not-saving").to_owned());
                }
            },
            FileEvent::RecoveryWritten(result) => {
                let Some(recovering) = self.recovery.as_mut() else {
                    return;
                };
                recovering.writing = false;
                match result {
                    Ok(()) => recovering.failing = false,
                    Err(error) => {
                        tracing::error!(%error, "cannot write the work for recovery");
                        // Tried again once the work changes.
                        recovering.written = None;
                        if !std::mem::replace(&mut recovering.failing, true) {
                            self.say(tr("recovery-not-saving").to_owned());
                        }
                    }
                }
            }
            FileEvent::Recovered(result) => {
                self.reading_found = false;
                match result {
                    Ok(recovered) => {
                        let Recovered {
                            found,
                            document,
                            id,
                            meta,
                        } = *recovered;
                        canvas.replace(document, false);
                        self.path = None;
                        self.id = id;
                        self.recovered_name = Some(if meta.name.ends_with("-recovered") {
                            meta.name
                        } else {
                            format!("{}-recovered", meta.name)
                        });
                        self.recovered_from = meta.path;
                        self.say(tr("recovery-done").to_owned());
                        // In this window's folder before the other one goes.
                        self.write_recovery(canvas);
                        let _ = self.to_worker.send(Job::DiscardRecovery(Box::new(found)));
                    }
                    Err(error) => self.say(format!("{}: {error}", tr("recovery-failed"))),
                }
            }
        }
    }

    /// The question about work left by a window no longer running, while
    /// there is some and nothing else is asked.
    pub fn ask_recovery(&mut self, ctx: &egui::Context, canvas: &mut Canvas) {
        if self.confirm.is_some() || self.reading_found || self.busy_dialog || self.saving {
            return;
        }
        let Some(found) = self.found.front() else {
            return;
        };
        let meta = &found.meta;
        let time = ugu_win::clock::local_text(meta.time).unwrap_or_default();
        let [width, height] = meta.canvas;
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::new("recovery")).show(ctx, |ui| {
            ui.heading(tr("recovery-title"));
            ui.label(tr("recovery-found"));
            ui.add_space(6.0);
            ui.strong(&meta.name);
            ui.label(format!("{time} · {width} × {height}"));
            if let Some(path) = &meta.path {
                ui.label(path.display().to_string());
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (choice_here, key) in [
                    (Answer::Recover, "recovery-recover"),
                    (Answer::Discard, "recovery-discard"),
                    (Answer::Later, "recovery-later"),
                ] {
                    if ui.button(tr(key)).clicked() {
                        choice = Some(choice_here);
                    }
                }
            });
        });
        if modal.should_close() && choice.is_none() {
            choice = Some(Answer::Later);
        }
        match choice {
            // Unsaved changes are asked about first, as before opening.
            Some(Answer::Recover) => self.request(Action::Recover, canvas),
            Some(Answer::Discard) => {
                if let Some(found) = self.found.pop_front() {
                    let _ = self.to_worker.send(Job::DiscardRecovery(Box::new(found)));
                }
            }
            // Unlocked, so the next start asks again.
            Some(Answer::Later) => drop(self.found.pop_front()),
            None => {}
        }
    }

    /// The question about unsaved changes, while one is open.
    pub fn confirm(&mut self, ctx: &egui::Context, canvas: &mut Canvas) {
        let Some(action) = self.confirm else {
            return;
        };
        let mut choice = None;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::new("unsaved changes")).show(ctx, |ui| {
            ui.heading(tr("unsaved-title"));
            ui.label(tr("unsaved-text"));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let save = ui.button(tr("unsaved-save"));
                let discard = ui.button(tr("unsaved-discard"));
                let back = ui.button(tr("unsaved-cancel"));
                // As 2.2.13: S and N press the buttons, and Enter presses
                // Save, the default, unless another button has the focus.
                let (s, n, enter) = ui.input_mut(|input| {
                    let none = egui::Modifiers::NONE;
                    (
                        input.consume_key(none, egui::Key::S),
                        input.consume_key(none, egui::Key::N),
                        input.key_pressed(egui::Key::Enter),
                    )
                });
                let elsewhere = !(discard.has_focus() || back.has_focus() || save.has_focus());
                if save.clicked() || s || (enter && elsewhere) {
                    choice = Some(true);
                } else if discard.clicked() || n {
                    choice = Some(false);
                } else if back.clicked() {
                    cancel = true;
                }
            });
        });
        if cancel || modal.should_close() {
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

fn run(job: Job) -> Option<FileEvent> {
    Some(match job {
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
        Job::ReadTools(path) => {
            let read = preset::load(&path);
            FileEvent::ToolsRead(path, read)
        }
        Job::WriteTools { path, bytes } => {
            let result =
                ugu_io::save::replace_with(&path, &ugu_win::file::replace_file, |mut file, _| {
                    std::io::Write::write_all(&mut file, &bytes)?;
                    Ok(())
                })
                .map_err(|error| error.to_string());
            FileEvent::ToolsWritten(path, result)
        }
        Job::Insert { path, fit } => {
            let started = std::time::Instant::now();
            let result = std::fs::read(&path)
                .map_err(|error| error.to_string())
                .and_then(|bytes| {
                    ugu_io::import::decode(&bytes, fit).map_err(|error| error.to_string())
                });
            tracing::info!(
                ms = started.elapsed().as_secs_f64() * 1000.0,
                ok = result.is_ok(),
                "image read to insert"
            );
            FileEvent::Inserted(path, result)
        }
        Job::Open(path) => {
            let started = std::time::Instant::now();
            let result = std::fs::File::open(&path)
                .map_err(|error| error.to_string())
                .and_then(|file| {
                    ugu_io::read::read(std::io::BufReader::new(file))
                        .map(|(document, id)| (Box::new(document), id))
                        .map_err(|error| error.to_string())
                });
            tracing::info!(
                ms = started.elapsed().as_secs_f64() * 1000.0,
                ok = result.is_ok(),
                "document opened"
            );
            FileEvent::Opened(path, result)
        }
        Job::BeginRecovery { root, folder } => FileEvent::RecoveryBegun(
            recovery::begin(&root, &folder).map_err(|error| error.to_string()),
        ),
        Job::WriteRecovery {
            folder,
            document,
            id,
            meta,
        } => {
            let started = std::time::Instant::now();
            let result = recovery::write(&folder, &document, id, &meta);
            tracing::info!(
                ms = started.elapsed().as_secs_f64() * 1000.0,
                ok = result.is_ok(),
                sequence = meta.sequence,
                "work written for recovery"
            );
            FileEvent::RecoveryWritten(result)
        }
        Job::ForgetRecovery(folder) => {
            if let Err(error) = recovery::forget(&folder) {
                tracing::warn!(%error, "cannot remove the work written for recovery");
            }
            return None;
        }
        Job::ReadRecovery(found) => {
            let found = *found;
            FileEvent::Recovered(match recovery::read(&found) {
                Ok((document, id, meta)) => Ok(Box::new(Recovered {
                    found,
                    document,
                    id,
                    meta,
                })),
                Err(error) => Err(match recovery::set_aside(found) {
                    Ok(aside) => format!("{error} ({})", aside.display()),
                    Err(_) => error,
                }),
            })
        }
        Job::DiscardRecovery(found) => {
            if let Err(error) = recovery::discard(*found) {
                tracing::warn!(%error, "cannot remove recovered work");
            }
            return None;
        }
        Job::EndRecovery { folder, lock } => {
            if let Err(error) = recovery::end(&folder, lock) {
                tracing::warn!(%error, "cannot remove the recovery folder");
            }
            return None;
        }
        Job::Stop => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{Receiver, channel};
    use std::time::Duration;
    use ugu_core::document::LayerKind;
    use ugu_session::Session;

    fn setup() -> (Files, Canvas, Receiver<FileEvent>) {
        let (sender, events) = channel();
        let files = Files::new(0, move |event| {
            let _ = sender.send(event);
        });
        let canvas = Canvas::new(Files::new_canvas(), |_| {});
        (files, canvas, events)
    }

    #[test]
    fn a_new_document_closes_without_asking_until_edited() {
        let (mut files, mut canvas, _events) = setup();
        assert_eq!(files.title(&canvas), "Untitled - Ugurugu");
        files.request(Action::Close, &mut canvas);
        assert!(files.should_close());
        files.close = false;
        canvas.edit(Session::add_layer).unwrap();
        files.request(Action::New, &mut canvas);
        assert_eq!(files.confirm, Some(Action::New));
        files.confirm = None;
        files.proceed(Action::New, &mut canvas);
        // Its size is asked for, starting from the document's.
        assert_eq!(files.new_document, Some([1024, 768]));
        files.start_new(&mut canvas, [64, 48]);
        assert!(!canvas.session().is_dirty());
        assert_eq!(canvas.session().document().canvas, [64, 48]);
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
        let path = folder("snapshot").join("drawing.ugurugu");
        canvas.edit(Session::add_layer).unwrap();
        files.path = Some(path.clone());
        files.save(&mut canvas);
        // A save says it is running only once it has taken a while.
        assert_eq!(files.message(), None);
        assert!(files.message_due().is_some());
        // An edit while the file thread writes.
        canvas.edit(Session::add_layer).unwrap();
        files.handle(next(&events), &mut canvas);
        assert!(files.message().unwrap().starts_with("Saved"));
        assert_eq!(layers_in(&path), 2);
        assert_eq!(canvas.session().document().layers.len(), 3);
        assert!(canvas.session().is_dirty());
        canvas.edit(Session::undo).unwrap();
        assert!(!canvas.session().is_dirty());
        assert_eq!(files.title(&canvas), "drawing - Ugurugu");
    }

    #[test]
    fn a_pending_transform_is_applied_before_saving_and_asked_about_before_replacing() {
        let (mut files, mut canvas, events) = setup();
        let path = folder("pending").join("drawing.ugurugu");
        canvas.edit(Session::select_all);
        canvas.begin_transform();
        canvas.edit(|session| session.set_transform(ugu_core::ops::Affine::translation(10.0, 0.0)));
        assert!(canvas.session().is_dirty());
        // New, open and close ask first.
        files.request(Action::New, &mut canvas);
        assert_eq!(files.confirm, Some(Action::New));
        files.confirm = None;
        files.path = Some(path.clone());
        files.save(&mut canvas);
        assert!(canvas.session().pending().is_none());
        files.handle(next(&events), &mut canvas);
        assert!(!canvas.session().is_dirty());
        let file = std::fs::File::open(&path).unwrap();
        let (saved, _) = ugu_io::read::read(std::io::BufReader::new(file)).unwrap();
        let LayerKind::Paint(paint) = &saved.layers[0].kind else {
            panic!("a paint layer");
        };
        assert!(matches!(
            paint.ops.last(),
            Some(ugu_core::ops::Op::TransformSelection { .. })
        ));
    }

    #[test]
    fn placed_text_is_applied_before_saving() {
        let (mut files, mut canvas, events) = setup();
        let path = folder("placed-text").join("drawing.ugurugu");
        canvas.edit(|session| {
            session.set_tool(ugu_session::Tool::Text);
            session.text_content = "Ug".to_owned();
        });
        canvas.edit(|session| {
            let outline = std::sync::Arc::new(ugu_core::text::Outline {
                contours: vec![vec![[0.0, 0.0], [20.0, 0.0], [20.0, 10.0]]],
                size: [20.0, 10.0],
            });
            session.place_text([10.0, 10.0], outline).unwrap();
        });
        assert!(canvas.session().is_dirty());
        files.request(Action::Close, &mut canvas);
        assert_eq!(files.confirm, Some(Action::Close));
        files.confirm = None;
        files.path = Some(path.clone());
        files.save(&mut canvas);
        assert!(canvas.session().placed_text().is_none());
        files.handle(next(&events), &mut canvas);
        assert!(!canvas.session().is_dirty());
        let file = std::fs::File::open(&path).unwrap();
        let (saved, _) = ugu_io::read::read(std::io::BufReader::new(file)).unwrap();
        let LayerKind::Paint(paint) = &saved.layers[0].kind else {
            panic!("a paint layer");
        };
        assert!(matches!(paint.ops[..], [ugu_core::ops::Op::Paint { .. }]));
    }

    #[test]
    fn a_tool_preset_carries_the_tools_and_wobble_to_another_window() {
        use ugu_core::ops::{MotionStyle, Wobble};
        use ugu_session::Tool;

        let (mut files, mut canvas, events) = setup();
        let path = folder("preset").join(format!("mine.{}", preset::EXTENSION));
        let mut wobble = Wobble::classic(6.0);
        wobble.motion.style = MotionStyle::Stepped;
        canvas.edit(|session| {
            let mut tools = session.tools();
            tools.tool = Tool::Fill;
            tools.fill.tolerance = 40;
            session.set_tools(tools);
            let document = session.document();
            let (frames, speed) = (document.frames, document.frames_per_second);
            session.set_animation(frames, speed, wobble).unwrap();
        });
        files.handle(
            FileEvent::Picked(Purpose::ExportTools, Some(path.clone())),
            &mut canvas,
        );
        files.handle(next(&events), &mut canvas);
        let exported = tr_with("preset-exported", &args_name(file_name(&path)));
        assert_eq!(files.message(), Some(exported.as_str()));

        let (mut files, mut canvas, events) = setup();
        let before = canvas.session().document().wobble;
        files.handle(
            FileEvent::Picked(Purpose::ImportTools, Some(path.clone())),
            &mut canvas,
        );
        files.handle(next(&events), &mut canvas);
        let imported = tr_with("preset-imported", &args_name(file_name(&path)));
        assert_eq!(files.message(), Some(imported.as_str()));
        let tools = canvas.session().tools();
        assert_eq!((tools.tool, tools.fill.tolerance), (Tool::Fill, 40));
        assert_eq!(canvas.session().document().wobble, wobble);
        // The wobble is the document's, so it is undone; the tools are not.
        assert!(canvas.edit(|session| session.undo()).unwrap());
        assert_eq!(canvas.session().document().wobble, before);
        assert_eq!(canvas.session().tools().tool, Tool::Fill);

        std::fs::write(&path, b"{}").unwrap();
        files.handle(
            FileEvent::Picked(Purpose::ImportTools, Some(path)),
            &mut canvas,
        );
        files.handle(next(&events), &mut canvas);
        assert!(
            files
                .message()
                .is_some_and(|message| message.ends_with(tr("preset-not-preset")))
        );
    }

    #[test]
    fn an_inserted_image_becomes_a_layer_ready_to_move_or_says_why_not() {
        let (mut files, mut canvas, events) = setup();
        let folder = folder("insert");
        let path = folder.join("picture.png");
        let mut pixels = Vec::new();
        for index in 0..(30 * 20) {
            pixels.extend([(index % 256) as u8, 40, 200, 255]);
        }
        ugu_io::image::export(
            &pixels,
            [30, 20],
            ugu_io::image::Format::Png,
            &path,
            &ugu_win::file::replace_file,
        )
        .unwrap();
        files.handle(
            FileEvent::Picked(Purpose::InsertImage, Some(path)),
            &mut canvas,
        );
        files.handle(next(&events), &mut canvas);
        let layer = canvas.session().current_layer();
        let Some(LayerKind::Paint(paint)) = canvas
            .session()
            .document()
            .layer(layer)
            .map(|layer| &layer.kind)
        else {
            panic!("a paint layer");
        };
        assert!(matches!(
            paint.ops[..],
            [ugu_core::ops::Op::PlaceImage { .. }]
        ));
        assert!(canvas.session().pending().is_some());
        let inserted = tr_with("image-inserted", &args_name("picture.png".to_owned()));
        assert_eq!(files.message(), Some(inserted.as_str()));

        let broken = folder.join("broken.png");
        std::fs::write(&broken, b"not an image").unwrap();
        files.handle(
            FileEvent::Picked(Purpose::InsertImage, Some(broken)),
            &mut canvas,
        );
        files.handle(next(&events), &mut canvas);
        assert!(
            files
                .message()
                .unwrap()
                .starts_with(tr("insert-image-failed"))
        );
    }

    #[test]
    fn a_failed_save_keeps_the_changes_unsaved_and_says_why() {
        let (mut files, mut canvas, events) = setup();
        canvas.edit(Session::add_layer).unwrap();
        files.path = Some(
            folder("missing")
                .join("no such folder")
                .join("drawing.ugurugu"),
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
        let good = folder.join("good.ugurugu");
        ugu_io::save::save(&document, [7; 16], &good, &ugu_win::file::replace_file).unwrap();
        let airbrush = ugu_core::store::Stroke {
            points: vec![ugu_core::store::Point {
                x: 4.0,
                y: 4.0,
                pressure: 1.0,
            }]
            .into(),
            color: ugu_core::ops::Rgba8([0, 0, 0, 255]),
            width: 6.0,
            brush: ugu_core::store::Brush {
                engine: ugu_core::store::BrushEngine::Airbrush,
                opacity: 1.0,
                hardness: 0.5,
                antialias: true,
                size_dynamics: 0.8,
                wobble_scale: 1.0,
                ..ugu_core::store::Brush::default()
            },
            seed: 1,
        };
        let layer = document.layers[0].id;
        let changes = ugu_core::command::draw(&document, layer, airbrush, false, None);
        ugu_core::edit::commit(&mut document, changes).unwrap();
        let airbrush_file = folder.join("airbrush.ugurugu");
        ugu_io::save::save(
            &document,
            [8; 16],
            &airbrush_file,
            &ugu_win::file::replace_file,
        )
        .unwrap();

        files.open_path(airbrush_file);
        files.handle(next(&events), &mut canvas);
        assert!(files.message().is_none());
        assert_eq!(canvas.session().document().store.strokes.len(), 1);

        files.open_path(good.clone());
        files.handle(next(&events), &mut canvas);
        assert_eq!(canvas.session().document().frames, 12);
        assert!(!canvas.session().is_dirty());
        assert_eq!(files.path, Some(good));
        assert_eq!(files.id, [7; 16]);
    }

    #[test]
    fn an_exported_png_is_the_rendered_frame_without_reference_layers() {
        use std::sync::Arc as Shared;
        use ugu_core::ops::{Op, Rgba8, StrokeId, Wobble};
        use ugu_core::store::{Brush, BrushEngine, Point, Stroke};

        let mut document = Document::new([64, 40]);
        document.background = Rgba8([0, 0, 0, 0]);
        document.wobble = Wobble::classic(3.0);
        let points: Vec<Point> = (0..20)
            .map(|step| Point {
                x: 4.0 + step as f32 * 3.0,
                y: 20.0 + (step as f32 * 0.5).sin() * 8.0,
                pressure: 0.6,
            })
            .collect();
        document.store.strokes.insert(
            StrokeId(0),
            Stroke {
                points: Shared::from(points),
                color: Rgba8([200, 60, 20, 160]),
                width: 7.0,
                brush: Brush {
                    engine: BrushEngine::Line,
                    opacity: 1.0,
                    hardness: 1.0,
                    antialias: true,
                    size_dynamics: 0.8,
                    wobble_scale: 1.0,
                    ..Brush::default()
                },
                seed: 9,
            },
        );
        if let LayerKind::Paint(paint) = &mut document.layers[0].kind {
            paint.ops.push(Op::Paint {
                stroke: StrokeId(0),
                clip: None,
            });
        }
        // The same stroke again on a reference layer, which the canvas shows
        // and an export leaves out.
        let mut reference = document.layers[0].clone();
        reference.id = ugu_core::document::LayerId(2);
        reference.reference = true;
        document.layers.push(reference);

        let path = folder("export").join("frame.png");
        let worker = crate::cache::CacheWorker::start(|_| {});
        let not_cancelled = std::sync::atomic::AtomicBool::new(false);
        let outcome = crate::export::still(
            Arc::new(document.clone()),
            5,
            &path,
            &worker.renders(),
            &not_cancelled,
        );
        assert_eq!(outcome, Outcome::Done);
        let file = std::fs::File::open(&path).unwrap();
        let mut decoder = png::Decoder::new(std::io::BufReader::new(file))
            .read_info()
            .unwrap();
        let mut decoded = vec![0; decoder.output_buffer_size().unwrap()];
        decoder.next_frame(&mut decoded).unwrap();

        use ugu_render::document::{DocumentRenderer, Purpose as RenderPurpose};
        let mut expected = vello_cpu::Pixmap::new(64, 40);
        DocumentRenderer::new(0).render(&document, 5, RenderPurpose::Export, &mut expected);
        let straight: Vec<u8> = expected
            .data_as_u8_slice()
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&pixel| ugu_io::image::unpremultiply(pixel))
            .collect();
        assert!(decoded == straight);
        let mut shown = vello_cpu::Pixmap::new(64, 40);
        DocumentRenderer::new(0).render(&document, 5, RenderPurpose::Display, &mut shown);
        assert!(shown.data_as_u8_slice() != expected.data_as_u8_slice());
        // One translucent stroke, not two over each other.
        assert!(
            decoded
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] == 160)
        );
        assert!(decoded.as_chunks::<4>().0.contains(&[0; 4]));
    }

    #[test]
    fn exports_run_one_at_a_time_and_a_cancelled_one_is_answered_at_once() {
        let (mut files, mut canvas, events) = setup();
        let folder = folder("export-flow");
        let first = folder.join("frame.jpg");
        files.handle(
            FileEvent::Picked(Purpose::ExportImage, Some(first.clone())),
            &mut canvas,
        );
        assert_eq!(files.export_status().as_deref(), Some(tr("export-running")));
        // Another waits for it: neither a dialog nor a second export.
        files.export_image();
        assert!(!files.busy_dialog);
        files.handle(
            FileEvent::Picked(Purpose::ExportImage, Some(folder.join("other.png"))),
            &mut canvas,
        );
        files.handle(next(&events), &mut canvas);
        assert_eq!(files.export_status(), None);
        assert_eq!(
            files.message(),
            Some(tr_with("export-done", &args_path(&first)).as_str())
        );
        assert!(first.exists());
        assert!(!folder.join("other.png").exists());

        // Cancelled: said at once; the thread's late answer changes nothing.
        let second = folder.join("second.png");
        files.handle(
            FileEvent::Picked(Purpose::ExportImage, Some(second)),
            &mut canvas,
        );
        files.cancel_export();
        assert_eq!(files.export_status(), None);
        assert_eq!(files.message(), Some(tr("export-canceled")));
        files.handle(next(&events), &mut canvas);
        assert_eq!(files.message(), Some(tr("export-canceled")));
        files.end();
        // Whether it got as far as writing or not, nothing is left half done.
        let left: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(left.is_empty(), "{left:?}");
    }

    #[test]
    fn a_new_id_is_a_version_4_uuid_and_differs_each_time() {
        let (a, b) = (new_id(), new_id());
        assert_ne!(a, b);
        assert_eq!(a[6] >> 4, 4);
        assert_eq!(a[8] >> 6, 0b10);
    }
}

#[cfg(test)]
mod end_to_end;

#[cfg(test)]
mod recovering;
