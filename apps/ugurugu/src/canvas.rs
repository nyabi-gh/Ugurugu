// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The document on screen: drawing, panning, zooming and turning the view.
//!
//! The image shown is the cached split put together. A stroke being drawn is
//! composited over it within the pixels it changes, and at pen-up the
//! committed stroke is added to the cached layer instead of rendering the
//! frame again. Anything else that changes the document asks the cache
//! worker for a new split and keeps showing the old one until it arrives.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ugu_core::document::DocumentError;
use ugu_core::document::{Document, LayerId, LayerKind};
use ugu_core::edit::{EditError, Outcome};
use ugu_core::motion::frame_in_cycle;
use ugu_core::ops::{MergeRefusal, Op};
use ugu_core::selection::Combine;
use ugu_core::store::StoreError;
use ugu_render::compose::{Split, Stamp, composite};
use ugu_render::document::{FULL_DETAIL, Purpose, TILE_EDGE, scaled_size, surface_estimate};
use ugu_render::live::LiveStroke;
use ugu_render::plan::{Reference, RenderPlan};
use ugu_render::raster::PixelRect;
use ugu_render::stroke::Pen;
use ugu_render::view::Placement;
use ugu_session::{Brushes, FillError, InputPoint, Reads, Session, StrokeRefused, Tool, Tools};
use ugu_win::clock::Ticks;
use ugu_win::pointer::{PointerKind, PointerSample};
use vello_cpu::Pixmap;

use crate::budget::Budget;
use crate::cache::{CacheWorker, Key, Preview, Rendered, Renders, Snapshot, Version};
use crate::i18n::tr;
use crate::input::{CanvasInput, Gesture};

mod clipboard;
mod cursor;
mod picking;
mod text;
mod transform;
mod view;

pub use cursor::BrushCursor;
pub use transform::{Grip, HANDLES};
pub use view::TURN_STEP;

const ZOOM_STEP: f64 = 1.25;
/// Up to 6400%, which fits 16×16 pixel art to a window (2.2.13: 1600%).
pub const ZOOM_RANGE: std::ops::RangeInclusive<f64> = 0.05..=64.0;
/// Fitting enlarges a document no larger than this, by a whole number of
/// times so each pixel stays square: pixel art. Larger ones fit at most at
/// 100%, as in 2.2.13.
const SMALL_EDGE: f64 = 256.0;

/// Frames play in order, none skipped. A frame not rendered yet holds the
/// one on screen, and the timing starts again from when it arrives, so a
/// cold start plays slowly instead of standing still until frames that were
/// due long ago come round again.
struct Playback {
    /// The next frame to show, as a frame number.
    next: i64,
    /// When it is due.
    due: Instant,
    /// Frames of one document state, by frame within the cycle.
    frames: HashMap<u32, Arc<Pixmap>>,
    version: Version,
    /// How the frames are drawn: smaller than the canvas, and with fewer
    /// stroke samples when shown smaller.
    preview: Preview,
    /// How many frames fit in the budget; 0 before it is worked out.
    ahead: u32,
    /// The frame on screen and how much smaller it is; at first the edited
    /// image, so that its pixels go once the next frame is shown.
    shown: (Arc<Pixmap>, u32),
    /// The first frame of the frames asked for, which run as far as the
    /// budget allows.
    window: Option<u32>,
}

enum Interaction {
    Idle,
    Drawing {
        live: Box<LiveStroke>,
        rect: Option<PixelRect>,
    },
    Panning {
        last: [f64; 2],
    },
    /// Dragging a shape with the selection tool; the session holds it.
    Selecting,
    /// Dragging the transform box from `start` (document pixels), which had
    /// `base` then.
    Transforming {
        grip: Grip,
        start: [f64; 2],
        base: ugu_core::ops::Affine,
    },
    /// Dragging placed text from `start` (document pixels), where its top
    /// left was at `base`.
    MovingText {
        start: [f64; 2],
        base: [f64; 2],
    },
    /// Picking up colours with the eyedropper.
    Picking,
    /// Turning the view by dragging sideways from client x `start`, where
    /// it was at `angle`.
    Turning {
        start: f64,
        angle: f64,
    },
    /// Zooming by dragging sideways from `start`, where the view was at
    /// `scale` and showed document point `anchor`.
    Zooming {
        start: [f64; 2],
        scale: f64,
        anchor: [f64; 2],
    },
    /// Two fingers, last `apart` physical pixels apart about `middle`, at
    /// `angle` degrees.
    Pinching {
        middle: [f64; 2],
        apart: f64,
        angle: f64,
    },
}

/// One open document and how it is shown: what each of the window's tabs
/// holds.
struct Tab {
    session: Session,
    /// Where it is kept and what it goes by.
    file: crate::files::DocumentFile,
    cache: CacheWorker,
    split: Option<(Key, Split)>,
    /// A pending transform shown on the split's layer.
    preview: Option<transform::Preview>,
    /// Placed text shown on the split's layer.
    text_preview: Option<text::TextPreview>,
    requested: Option<Key>,
    /// The edited image; empty from playback until the next split.
    display: Pixmap,
    /// The frame playback stopped on and how much smaller it is, shown
    /// until the split of that frame arrives.
    held: Option<(Arc<Pixmap>, u32)>,
    /// Playback paused for a stroke or a click, to go on once it is over,
    /// as 2.2.13's.
    resume_playback: bool,
    /// What the wand and the bucket last read, kept for the next click.
    reference: Option<Pixmap>,
    /// Physical pixels per document pixel.
    scale: f64,
    /// The document's top-left corner from the area's, in physical pixels.
    offset: [f64; 2],
    /// The view's turn, clockwise in degrees, and whether it is mirrored.
    angle: f64,
    mirrored: bool,
    placed: bool,
    playback: Option<Playback>,
    /// Tells this document's renders apart from those of the documents
    /// shown before it.
    generation: u64,
    /// The document as last handed to the worker, shared by its jobs.
    snapshot: Option<(u64, Snapshot)>,
    /// Small images of paint layers' own pixels, with the revision each
    /// shows.
    thumbnails: HashMap<LayerId, (u64, Arc<Pixmap>)>,
    /// Thumbnails asked for and not yet back.
    thumbnails_asked: bool,
    /// The canvas area when another tab was last shown instead, to move a
    /// placed view by as much as the area has changed since.
    left_area: Option<[i32; 4]>,
}

impl Tab {
    fn new(document: Document, saved: bool, cache: CacheWorker, generation: u64) -> Self {
        let [width, height] = document.canvas.map(|edge| edge as u16);
        Self {
            session: Session::new(document, saved),
            file: crate::files::DocumentFile::new(),
            cache,
            split: None,
            preview: None,
            text_preview: None,
            requested: None,
            display: Pixmap::new(width, height),
            held: None,
            resume_playback: false,
            reference: None,
            scale: 1.0,
            offset: [0.0, 0.0],
            angle: 0.0,
            mirrored: false,
            placed: false,
            playback: None,
            generation,
            snapshot: None,
            thumbnails: HashMap::new(),
            thumbnails_asked: false,
            left_area: None,
        }
    }

    /// Shows `document` instead, with an empty history, keeping the worker
    /// and the view's turn.
    fn reset(&mut self, document: Document, saved: bool, generation: u64) {
        let [width, height] = document.canvas.map(|edge| edge as u16);
        self.session = Session::new(document, saved);
        self.generation = generation;
        self.split = None;
        self.preview = None;
        self.text_preview = None;
        self.requested = None;
        self.display = Pixmap::new(width, height);
        self.held = None;
        self.resume_playback = false;
        self.playback = None;
        self.snapshot = None;
        self.thumbnails.clear();
        self.thumbnails_asked = false;
        self.placed = false;
    }
}

/// What the tab bar shows of a tab.
#[derive(Clone, Debug, PartialEq)]
pub struct TabInfo {
    pub name: String,
    /// Whether it has changes not saved.
    pub dirty: bool,
}

impl TabInfo {
    fn of(tab: &Tab) -> Self {
        Self {
            name: tab.file.display_name(),
            dirty: tab.session.is_dirty(),
        }
    }
}

pub struct Canvas {
    /// The document shown.
    tab: Tab,
    /// The other tabs, in order, without the one shown.
    others: Vec<Tab>,
    /// Where the tab shown is in the order.
    position: usize,
    /// Gets each tab's finished cache renders, on its worker thread.
    cache_done: Arc<dyn Fn(Rendered) + Send + Sync>,
    /// The window's tools, for whichever document it shows.
    tools: Tools,
    typesetter: text::Typesetter,
    /// Display pixels not yet uploaded.
    upload: Option<PixelRect>,
    /// The size, shrink and origin of the pixels shown when last uploaded.
    uploaded: Option<([u16; 2], u32, [u32; 2])>,
    stamp: Stamp,
    interaction: Interaction,
    /// Shift and Alt.
    modifiers: (bool, bool),
    /// The last stroke's coverage, cleared, for the next stroke.
    spare_coverage: Option<Pixmap>,
    /// Canvas area in client physical pixels: left, top, right, bottom.
    area: Option<[i32; 4]>,
    pixels_per_point: f32,
    /// Whether the pen over the canvas shows its eraser end.
    hover_eraser: bool,
    /// Where the canvas's own pointer last was, in client physical pixels.
    drawn_at: Option<[f64; 2]>,
    sample_count: usize,
    notice: Option<String>,
    /// Off in the settings keeps playback from starting.
    animation_allowed: bool,
    /// Counts documents opened, so renders of an earlier one are told apart.
    generations: u64,
}

/// A cache worker handing its renders to `done`.
fn worker(done: &Arc<dyn Fn(Rendered) + Send + Sync>) -> CacheWorker {
    let done = done.clone();
    CacheWorker::start(move |rendered| done(rendered))
}

fn union(a: Option<PixelRect>, b: Option<PixelRect>) -> Option<PixelRect> {
    match (a, b) {
        (Some(a), Some(b)) => Some([
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[2].max(b[2]),
            a[3].max(b[3]),
        ]),
        (a, None) => a,
        (None, b) => b,
    }
}

fn millis(ticks: Ticks) -> f64 {
    ticks.seconds_since(Ticks(0)) * 1000.0
}

impl Canvas {
    /// `cache_done` gets each finished cache render, on the worker thread.
    pub fn new(document: Document, cache_done: impl Fn(Rendered) + Send + Sync + 'static) -> Self {
        let [width, height] = document.canvas.map(|edge| edge as u16);
        let cache_done: Arc<dyn Fn(Rendered) + Send + Sync> = Arc::new(cache_done);
        Self {
            // Nothing to lose until edited, as in 2.2.13.
            tab: Tab::new(document, true, worker(&cache_done), 0),
            others: Vec::new(),
            position: 0,
            cache_done,
            tools: Tools::default(),
            typesetter: text::Typesetter::default(),
            upload: Some([0, 0, u32::from(width), u32::from(height)]),
            uploaded: None,
            stamp: Stamp::default(),
            interaction: Interaction::Idle,
            modifiers: (false, false),
            spare_coverage: None,
            area: None,
            pixels_per_point: 1.0,
            hover_eraser: false,
            drawn_at: None,
            sample_count: 0,
            notice: None,
            animation_allowed: true,
            generations: 0,
        }
    }

    /// Where the document shown is kept and what it goes by.
    pub fn file(&self) -> &crate::files::DocumentFile {
        &self.tab.file
    }

    pub fn file_mut(&mut self) -> &mut crate::files::DocumentFile {
        &mut self.tab.file
    }

    pub fn session(&self) -> &Session {
        &self.tab.session
    }

    /// The tools stay as they are from one document to the next, as in
    /// 2.2.13.
    pub fn tools(&self) -> &Tools {
        &self.tools
    }

    /// For settings that start nothing; changing the tool goes through
    /// `set_tool`.
    pub fn tools_mut(&mut self) -> &mut Tools {
        &mut self.tools
    }

    pub fn tool(&self) -> Tool {
        self.tools.tool
    }

    /// Changes the tool; a pending transform or placed text is applied
    /// first, as changing tools ends it.
    pub fn set_tool(&mut self, tool: Tool) -> Result<(), EditError> {
        if tool != self.tools.tool {
            self.edit(Session::settle)?;
            self.tools.tool = tool;
        }
        Ok(())
    }

    /// The brush and eraser the document shown draws with: the pixel ones in
    /// pixel mode.
    pub fn brushes(&self) -> &Brushes {
        self.tools.brushes(self.tab.session.pixel_mode())
    }

    pub fn brushes_mut(&mut self) -> &mut Brushes {
        self.tools.brushes_mut(self.tab.session.pixel_mode())
    }

    /// Makes `preset` the brush, or with `Tool::Eraser` the eraser, of the
    /// document's mode.
    pub fn choose_preset(&mut self, tool: Tool, preset: &'static ugu_core::brush::Preset) {
        let pixel_mode = self.tab.session.pixel_mode();
        self.tools.choose_preset(pixel_mode, tool, preset);
    }

    /// Takes over `tools`, as from the settings or a tool preset file; a
    /// pending transform or placed text is applied first.
    pub fn set_tools(&mut self, tools: Tools) -> Result<(), EditError> {
        self.edit(Session::settle)?;
        self.tools = tools;
        Ok(())
    }

    /// Puts `document` in place of the open one, with an empty history;
    /// `saved` when it is on disk as it is.
    pub fn replace(&mut self, document: Document, saved: bool) {
        self.generations += 1;
        self.tab.reset(document, saved, self.generations);
        self.interaction = Interaction::Idle;
        self.spare_coverage = None;
        self.upload_all();
        if let Some(area) = self.area {
            self.place(area);
        }
    }

    pub fn tab_count(&self) -> usize {
        self.others.len() + 1
    }

    /// Where the tab shown is in the order.
    pub fn tab_position(&self) -> usize {
        self.position
    }

    /// Each tab's name and whether it has unsaved changes, in order. Tabs
    /// of the same name are told apart by a number in the order they were
    /// opened, which stays when they are moved.
    pub fn tab_infos(&self) -> Vec<TabInfo> {
        let tabs: Vec<&Tab> = (0..self.tab_count())
            .map(|index| self.tab_at(index))
            .collect();
        let mut infos: Vec<TabInfo> = tabs.iter().map(|tab| TabInfo::of(tab)).collect();
        let names: Vec<String> = infos.iter().map(|info| info.name.clone()).collect();
        for (index, info) in infos.iter_mut().enumerate() {
            let earlier = (0..tabs.len())
                .filter(|&other| {
                    names[other] == names[index] && tabs[other].generation < tabs[index].generation
                })
                .count();
            if earlier > 0 {
                info.name = format!("{} ({})", info.name, earlier + 1);
            }
        }
        infos
    }

    /// The tab at `index` in the order.
    fn tab_at(&self, index: usize) -> &Tab {
        match index.cmp(&self.position) {
            std::cmp::Ordering::Equal => &self.tab,
            std::cmp::Ordering::Less => &self.others[index],
            std::cmp::Ordering::Greater => &self.others[index - 1],
        }
    }

    /// Whether the tab shown holds nothing to keep: never saved or named,
    /// and with no changes. Opening a document takes its place.
    pub fn is_pristine(&self) -> bool {
        let file = &self.tab.file;
        file.path.is_none() && file.unsaved_name.is_none() && !self.tab.session.is_dirty()
    }

    /// Opens `document` in a new tab after the one shown and shows it;
    /// `saved` when it is on disk as it is.
    pub fn open_tab(&mut self, document: Document, saved: bool) {
        self.generations += 1;
        let tab = Tab::new(document, saved, worker(&self.cache_done), self.generations);
        self.leave_tab();
        let left = std::mem::replace(&mut self.tab, tab);
        self.others.insert(self.position, left);
        self.position += 1;
        self.enter_tab();
    }

    /// Shows the tab at `index` in the order.
    pub fn show_tab(&mut self, index: usize) {
        if index == self.position || index >= self.tab_count() {
            return;
        }
        self.leave_tab();
        let (taken, back) = if index < self.position {
            (index, self.position - 1)
        } else {
            (index - 1, self.position)
        };
        let shown = self.others.remove(taken);
        let left = std::mem::replace(&mut self.tab, shown);
        self.others.insert(back, left);
        self.position = index;
        self.enter_tab();
    }

    /// Closes the tab at `index`, dropping its changes. Closing the last
    /// leaves `empty` in it, as a window starts.
    pub fn close_tab(&mut self, index: usize, empty: Document) {
        if index >= self.tab_count() {
            return;
        }
        if index != self.position {
            let taken = if index < self.position {
                self.position -= 1;
                index
            } else {
                index - 1
            };
            self.others.remove(taken);
            return;
        }
        if self.others.is_empty() {
            self.replace(empty, true);
            self.tab.file = crate::files::DocumentFile::new();
            return;
        }
        if let Interaction::Drawing { .. } =
            std::mem::replace(&mut self.interaction, Interaction::Idle)
        {
            self.tab.session.cancel_stroke();
        }
        // The one after it, or before it when it is the last.
        let next = self.position.min(self.others.len() - 1);
        let shown = self.others.remove(next);
        drop(std::mem::replace(&mut self.tab, shown));
        self.position = next;
        self.enter_tab();
    }

    /// Moves the tab at `from` to `to` in the order.
    pub fn move_tab(&mut self, from: usize, to: usize) {
        let count = self.tab_count();
        if from >= count || to >= count || from == to {
            return;
        }
        let mut order: Vec<usize> = (0..count).collect();
        let moved = order.remove(from);
        order.insert(to, moved);
        let position = self.position;
        let mut old: Vec<Option<Tab>> = std::mem::take(&mut self.others)
            .into_iter()
            .map(Some)
            .collect();
        self.others = order
            .iter()
            .filter(|&&index| index != position)
            .map(|&index| {
                let slot = if index < position { index } else { index - 1 };
                old[slot].take().expect("each tab once")
            })
            .collect();
        self.position = order
            .iter()
            .position(|&index| index == position)
            .expect("the tab shown is in the order");
    }

    /// Each open document's generation and session, in tab order.
    pub fn sessions(&self) -> Vec<(u64, &Session)> {
        (0..self.tab_count())
            .map(|index| {
                let tab = self.tab_at(index);
                (tab.generation, &tab.session)
            })
            .collect()
    }

    /// The session and file of the tab of document `generation`, when it is
    /// not the one shown.
    pub fn hidden_tab(&self, generation: u64) -> Option<(&Session, &crate::files::DocumentFile)> {
        self.others
            .iter()
            .find(|tab| tab.generation == generation)
            .map(|tab| (&tab.session, &tab.file))
    }

    /// The document of a tab not shown as it is now, shared: its last
    /// snapshot while that is current, as for the tab shown.
    pub fn hidden_document(&mut self, generation: u64) -> Option<Arc<Document>> {
        let tab = self
            .others
            .iter_mut()
            .find(|tab| tab.generation == generation)?;
        let revision = tab.session.revision();
        match &tab.snapshot {
            Some((at, snapshot)) if *at == revision => Some(snapshot.document.clone()),
            _ => {
                let snapshot = Snapshot {
                    document: Arc::new(tab.session.document().clone()),
                    layers: Arc::new(tab.session.layer_revisions().clone()),
                };
                let document = snapshot.document.clone();
                tab.snapshot = Some((revision, snapshot));
                Some(document)
            }
        }
    }

    /// Each open document's generation, in tab order.
    pub fn tab_generations(&self) -> Vec<u64> {
        (0..self.tab_count())
            .map(|index| self.tab_at(index).generation)
            .collect()
    }

    /// Where the tab showing the file at `path` is, if one does.
    pub fn tab_with_path(&self, path: &Path) -> Option<usize> {
        let same = |kept: &Path| {
            kept == path
                || std::fs::canonicalize(kept)
                    .ok()
                    .zip(std::fs::canonicalize(path).ok())
                    .is_some_and(|(a, b)| a == b)
        };
        (0..self.tab_count())
            .find(|&index| self.tab_at(index).file.path.as_deref().is_some_and(same))
    }

    /// The first tab, in order, with unsaved changes and not in `skipped`
    /// (by generation).
    pub fn dirty_tab(&self, skipped: &[u64]) -> Option<usize> {
        (0..self.tab_count()).find(|&index| {
            let tab = self.tab_at(index);
            tab.session.is_dirty() && !skipped.contains(&tab.generation)
        })
    }

    /// Records that document `generation`, not shown, was saved to `path`
    /// as it was at `state`.
    pub fn mark_saved_hidden(
        &mut self,
        generation: u64,
        state: ugu_core::history::StateId,
        path: PathBuf,
    ) {
        if let Some(tab) = self
            .others
            .iter_mut()
            .find(|tab| tab.generation == generation)
        {
            tab.session.mark_saved(state);
            tab.file.path = Some(path);
        }
    }

    /// Ends what the tab shown is doing before another is shown: a pending
    /// transform or placed text is applied, as changing tools does, and
    /// playback stops.
    fn leave_tab(&mut self) {
        if let Err(error) = self.edit(Session::settle) {
            tracing::warn!(?error, "what was pending could not be applied");
        }
        self.interaction = Interaction::Idle;
        self.tab.playback = None;
        self.tab.resume_playback = false;
        self.tab.left_area = self.area;
    }

    /// Shows the tab just made current: its pixels are uploaded again and
    /// what was asked of the worker while it was hidden is asked again.
    fn enter_tab(&mut self) {
        self.interaction = Interaction::Idle;
        self.spare_coverage = None;
        self.tab.requested = None;
        self.tab.thumbnails_asked = false;
        self.upload_all();
        if let Some(area) = self.area {
            match self.tab.left_area.take() {
                Some(old) if self.tab.placed => {
                    for axis in 0..2 {
                        let grown = (area[axis + 2] - area[axis]) - (old[axis + 2] - old[axis]);
                        self.tab.offset[axis] += f64::from(grown / 2);
                    }
                }
                _ if !self.tab.placed => self.place(area),
                _ => {}
            }
        }
    }

    /// The document as it is now, shared with work off this thread.
    pub fn snapshot_now(&mut self) -> Arc<Document> {
        self.snapshot().document
    }

    pub fn mark_saved(&mut self, state: ugu_core::history::StateId) {
        self.tab.session.mark_saved(state);
    }

    /// Changes the session outside drawing. A stroke being drawn is dropped
    /// first, as the change may move or remove what it is drawn on.
    pub fn edit<R>(&mut self, change: impl FnOnce(&mut Session) -> R) -> R {
        if let Interaction::Drawing { rect, .. } =
            std::mem::replace(&mut self.interaction, Interaction::Idle)
        {
            self.tab.session.cancel_stroke();
            self.recomposite(rect);
        }
        let before = self.tab.session.document().canvas;
        let key = self.key();
        self.tab.session.use_tools(&self.tools);
        let result = change(&mut self.tab.session);
        self.record_colors();
        if self.tab.session.document().canvas != before {
            // The split is of the canvas before; what it shows stays until
            // the split of the new one arrives.
            self.tab.split = None;
            self.spare_coverage = None;
            self.fit();
        }
        self.follow_transform(key);
        result
    }

    /// Zooms around the middle of the canvas area.
    pub fn zoom_in_place(&mut self, notches: f32) {
        if let Some([left, top, right, bottom]) = self.area {
            let middle = [f64::from(left + right) / 2.0, f64::from(top + bottom) / 2.0];
            self.zoom(middle, notches);
        }
    }

    /// Zooms to `scale` around the middle of the canvas area.
    pub fn zoom_to(&mut self, scale: f64) {
        let notches = (scale / self.tab.scale).ln() / ZOOM_STEP.ln();
        self.zoom_in_place(notches as f32);
    }

    /// The document point under `position`, in client physical pixels.
    pub fn document_point(&self, position: [f64; 2]) -> [f64; 2] {
        self.to_document(position)
    }

    /// Shows the whole document: at most at 100%, or for a small document as
    /// many whole times as fit.
    pub fn fit(&mut self) {
        if let Some(area) = self.area {
            self.place(area);
        }
    }

    pub fn sample_count(&self) -> usize {
        self.sample_count
    }

    /// Which document is shown, counting each one that replaced another.
    pub fn generation(&self) -> u64 {
        self.tab.generation
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    fn version(&self) -> Version {
        Version {
            document: self.tab.generation,
            revision: self.tab.session.revision(),
        }
    }

    pub fn scale(&self) -> f64 {
        self.tab.scale
    }

    fn key(&self) -> Key {
        let document = self.tab.session.document();
        Key {
            version: self.version(),
            layer: self.tab.session.current_layer(),
            frame: frame_in_cycle(self.tab.session.frame(), document.frames),
        }
    }

    pub fn contains(&self, position: [f64; 2]) -> bool {
        self.area.is_some_and(|[left, top, right, bottom]| {
            (f64::from(left)..f64::from(right)).contains(&position[0])
                && (f64::from(top)..f64::from(bottom)).contains(&position[1])
        })
    }

    fn input_point(&self, sample: &PointerSample) -> InputPoint {
        InputPoint {
            position: self.to_document(sample.position),
            pressure: sample.pressure,
            time: millis(sample.time),
        }
    }

    fn snapshot(&mut self) -> Snapshot {
        let revision = self.tab.session.revision();
        match &self.tab.snapshot {
            Some((at, snapshot)) if *at == revision => snapshot.clone(),
            _ => {
                let snapshot = Snapshot {
                    document: Arc::new(self.tab.session.document().clone()),
                    layers: Arc::new(self.tab.session.layer_revisions().clone()),
                };
                self.tab.snapshot = Some((revision, snapshot.clone()));
                snapshot
            }
        }
    }

    /// Where renders for export go, after the canvas's own.
    pub fn renders(&self) -> Renders {
        self.tab.cache.renders()
    }

    /// The thumbnail of paint layer `id` and the revision it shows.
    pub fn thumbnail(&self, id: LayerId) -> Option<(u64, &Arc<Pixmap>)> {
        self.tab
            .thumbnails
            .get(&id)
            .map(|(revision, pixels)| (*revision, pixels))
    }

    /// Asks for the thumbnails of layers whose pixels changed since theirs
    /// were drawn, unless some are already on their way.
    fn sync_thumbnails(&mut self) {
        if self.tab.thumbnails_asked || matches!(self.interaction, Interaction::Drawing { .. }) {
            return;
        }
        let revisions = self.tab.session.layer_revisions();
        let mut stale = Vec::new();
        thumbnail_keys(
            &self.tab.session.document().layers,
            revisions,
            &mut |id, key| {
                if self
                    .tab
                    .thumbnails
                    .get(&id)
                    .is_none_or(|(shown, _)| *shown != key)
                {
                    stale.push((id, key));
                }
            },
        );
        if stale.is_empty() {
            return;
        }
        let snapshot = self.snapshot();
        self.tab
            .cache
            .request_thumbnails(self.tab.generation, stale, snapshot.document);
        self.tab.thumbnails_asked = true;
    }

    /// Shows a pending transform as it is now, asks for a new split when the
    /// shown one no longer matches, and for thumbnails that are out of date.
    /// Playback shows whole frames instead of a split.
    pub fn sync(&mut self) {
        self.tab.session.use_tools(&self.tools);
        self.record_colors();
        self.refresh_preview();
        self.refresh_text_preview();
        self.sync_thumbnails();
        if self.tab.playback.is_some() {
            return;
        }
        let key = self.key();
        if self.tab.split.as_ref().map(|(shown, _)| *shown) != Some(key)
            && self.tab.requested != Some(key)
        {
            let budget = Budget::now(self.tab.cache.surface_bytes() as u64);
            self.tab.cache.set_surface_budget(budget.surfaces);
            let snapshot = self.snapshot();
            self.tab.cache.request(key, snapshot);
            self.tab.requested = Some(key);
        }
    }

    /// Whether renders asked for have not come back yet.
    #[cfg(test)]
    pub fn awaits_renders(&self) -> bool {
        self.tab.requested.is_some() || self.tab.thumbnails_asked
    }

    /// Takes a finished render if it is still wanted.
    pub fn adopt(&mut self, rendered: Rendered) {
        match rendered {
            Rendered::Split {
                key,
                split,
                display,
            } => {
                if key != self.key() || self.tab.playback.is_some() {
                    return;
                }
                self.tab.requested = None;
                self.tab.held = None;
                self.tab.display = display;
                self.tab.split = Some((key, split));
                self.tab.preview = None;
                self.tab.text_preview = None;
                self.upload_all();
                self.refresh_preview();
                self.refresh_text_preview();
                if let Interaction::Drawing { rect, .. } = &self.interaction {
                    let rect = *rect;
                    self.recomposite(rect);
                }
            }
            Rendered::Frame {
                version,
                frame,
                preview,
                pixels,
            } => {
                if let Some(playback) = self.tab.playback.as_mut()
                    && playback.version == version
                    && playback.preview == preview
                {
                    playback.frames.insert(frame, pixels);
                }
            }
            Rendered::Thumbnails {
                document,
                thumbnails,
            } => {
                if document != self.tab.generation {
                    return;
                }
                self.tab.thumbnails_asked = false;
                for (id, revision, pixels) in thumbnails {
                    self.tab.thumbnails.insert(id, (revision, Arc::new(pixels)));
                }
                let document = self.tab.session.document();
                self.tab
                    .thumbnails
                    .retain(|id, _| document.layer(*id).is_some());
            }
        }
    }

    pub fn is_playing(&self) -> bool {
        self.tab.playback.is_some()
    }

    /// Whether the wobble animation may play, from the settings.
    pub fn animation_allowed(&self) -> bool {
        self.animation_allowed
    }

    /// Allows the wobble animation to play or not; not stops it.
    pub fn allow_animation(&mut self, allowed: bool) {
        self.animation_allowed = allowed;
        if !allowed {
            self.tab.resume_playback = false;
            if self.is_playing() {
                self.toggle_playback();
            }
        }
    }

    /// Stops on the frame shown, which stays until the split of it arrives;
    /// false when not playing.
    fn stop_playback(&mut self) -> bool {
        let Some(playback) = self.tab.playback.take() else {
            return false;
        };
        self.tab.held = Some(playback.shown);
        self.upload_all();
        true
    }

    /// Stops on the frame shown while a stroke is drawn or an area clicked,
    /// and plays on once it is over.
    fn pause_playback(&mut self) {
        if self.stop_playback() {
            self.tab.resume_playback = true;
        }
    }

    /// Plays from the current frame, or stops on the frame shown.
    pub fn toggle_playback(&mut self) {
        // Paused for a stroke, it stays stopped.
        if std::mem::take(&mut self.tab.resume_playback) || self.stop_playback() {
            return;
        }
        if !self.animation_allowed {
            return;
        }
        // Playing moves through frames, which applies a pending transform or
        // placed text.
        if !self.apply_pending() {
            return;
        }
        // Stopping asks for a split of the frame it stops on. Until then the
        // old one's layer surfaces would only keep the renderer from reusing
        // them for playback.
        self.tab.split = None;
        self.tab.requested = None;
        self.spare_coverage = None;
        let shown = self.tab.held.take().unwrap_or_else(|| {
            let display = std::mem::replace(&mut self.tab.display, Pixmap::new(1, 1));
            (Arc::new(display), 1)
        });
        tracing::debug!("playback started");
        self.tab.playback = Some(Playback {
            next: self.tab.session.frame() + 1,
            due: Instant::now(),
            frames: HashMap::new(),
            version: self.version(),
            preview: preview(self.tab.scale),
            ahead: 0,
            shown,
            window: None,
        });
    }

    /// Shows the next frame if it is due and rendered, and returns when to
    /// look again; `None` when not playing.
    pub fn tick(&mut self, now: Instant) -> Option<Instant> {
        if self.tab.resume_playback && !matches!(self.interaction, Interaction::Drawing { .. }) {
            self.tab.resume_playback = false;
            self.toggle_playback();
        }
        let document = self.tab.session.document();
        let (frames, fps) = (document.frames, f64::from(document.frames_per_second));
        let period = Duration::from_secs_f64(1.0 / fps);
        let version = self.version();
        let preview = preview(self.tab.scale);
        let shrink = preview.shrink;
        let playback = self.tab.playback.as_ref()?;
        if playback.version != version || playback.preview != preview || playback.ahead == 0 {
            // The frames held are given back first, so they count as free.
            self.tab.playback.as_mut()?.frames.clear();
            let (ahead, surfaces) = frames_ahead(document, shrink);
            self.tab.cache.set_surface_budget(surfaces);
            let playback = self.tab.playback.as_mut()?;
            playback.version = version;
            playback.preview = preview;
            playback.ahead = ahead;
            playback.window = None;
        }
        let playback = self.tab.playback.as_mut()?;
        let ahead = playback.ahead;
        let cycle = frame_in_cycle(playback.next, frames);
        let covered = playback
            .window
            .is_some_and(|start| (cycle + frames - start) % frames < ahead);
        let mut shown = false;
        if now >= playback.due
            && let Some(pixels) = playback.frames.get(&cycle)
        {
            playback.shown = (pixels.clone(), shrink);
            playback.next += 1;
            // Keep the cadence unless a wait for rendering put it behind.
            playback.due = if now - playback.due < period {
                playback.due + period
            } else {
                now + period
            };
            shown = true;
        }
        let wake = playback.due.max(now + Duration::from_millis(1));

        if !covered {
            // From the next frame onwards, as far as the budget allows;
            // frames outside that window are let go.
            let wanted: Vec<u32> = (0..ahead).map(|offset| (cycle + offset) % frames).collect();
            playback.frames.retain(|at, _| wanted.contains(at));
            let missing: Vec<u32> = wanted
                .into_iter()
                .filter(|at| !playback.frames.contains_key(at))
                .collect();
            playback.window = Some(cycle);
            let snapshot = self.snapshot();
            self.tab
                .cache
                .request_frames(version, &missing, &snapshot, preview);
        }
        if shown {
            let playback = self.tab.playback.as_ref().expect("playing");
            let held: usize = playback
                .frames
                .values()
                .map(|pixels| pixels.data().len() * 4)
                .sum();
            tracing::debug!(
                frame = cycle,
                ahead,
                held_mib = held >> 20,
                surfaces_mib = self.tab.cache.surface_bytes() >> 20,
                "playback frame shown"
            );
            let next = playback.next;
            if self.tab.session.set_frame(next - 1).is_err() {
                self.notice = Some(tr("restyle-pending").to_owned());
                self.stop_playback();
                return None;
            }
            self.upload_all();
        }
        Some(wake)
    }

    /// Puts what the document was drawn, filled and written with in the
    /// colour history.
    fn record_colors(&mut self) {
        for color in self.tab.session.take_used_colors() {
            self.tools.colors.record(color);
        }
    }

    /// Puts `rect` of the display back together, with the stroke being drawn.
    fn recomposite(&mut self, rect: Option<PixelRect>) {
        let (Some(rect), Some((_, split))) = (rect, &self.tab.split) else {
            return;
        };
        let live = match &self.interaction {
            Interaction::Drawing { live, .. } => Some(live.as_ref()),
            _ => None,
        };
        composite(split, live, rect, &mut self.tab.display);
        self.upload = union(self.upload, Some(rect));
    }

    pub fn apply(&mut self, input: CanvasInput) {
        if let CanvasInput::Begin(_, _, sample)
        | CanvasInput::Extend(sample)
        | CanvasInput::End(sample) = &input
        {
            self.drawn_at = Some(sample.position);
        }
        match input {
            CanvasInput::Begin(Gesture::Pan, _, sample) => {
                self.interaction = Interaction::Panning {
                    last: sample.position,
                };
            }
            CanvasInput::Begin(Gesture::Turn, _, sample) => self.begin_turn_drag(sample.position),
            CanvasInput::Begin(Gesture::Zoom, _, sample) => self.begin_zoom_drag(sample.position),
            CanvasInput::Begin(Gesture::Draw, _, sample) if self.picks() => {
                self.sample_count += 1;
                self.begin_pick(sample.position);
            }
            CanvasInput::Begin(Gesture::Draw, _, sample)
                if self.tab.session.pending().is_some() =>
            {
                self.sample_count += 1;
                self.begin_transform_drag(sample.position);
            }
            CanvasInput::Begin(Gesture::Draw, _, sample) if self.tools.tool == Tool::Select => {
                self.sample_count += 1;
                self.notice = None;
                let point = self.to_document(sample.position);
                let combine = self.combine();
                self.tab
                    .session
                    .begin_selection(self.tools.selection_shape, point, combine);
                self.interaction = Interaction::Selecting;
            }
            CanvasInput::Begin(Gesture::Draw, _, sample) if self.tools.tool == Tool::Text => {
                self.sample_count += 1;
                self.press_text(sample.position);
            }
            CanvasInput::Begin(Gesture::Draw, _, sample)
                if matches!(self.tools.tool, Tool::Wand | Tool::Fill) =>
            {
                self.sample_count += 1;
                let point = self.to_document(sample.position);
                self.click_area(point);
            }
            CanvasInput::Begin(Gesture::Draw, kind, sample) => {
                self.sample_count += 1;
                self.begin_stroke(kind, &sample);
            }
            CanvasInput::Extend(sample) => {
                self.sample_count += 1;
                match &mut self.interaction {
                    Interaction::Panning { last } => {
                        self.tab.offset[0] += sample.position[0] - last[0];
                        self.tab.offset[1] += sample.position[1] - last[1];
                        *last = sample.position;
                    }
                    Interaction::Drawing { .. } => self.extend_stroke(&sample),
                    Interaction::Selecting => {
                        let point = self.to_document(sample.position);
                        self.tab.session.extend_selection(point);
                    }
                    Interaction::Transforming { .. } => self.drag_transform(sample.position),
                    Interaction::MovingText { .. } => self.drag_text(sample.position),
                    Interaction::Picking => self.pick(sample.position),
                    Interaction::Turning { .. } | Interaction::Zooming { .. } => {
                        self.drag_view(sample.position);
                    }
                    Interaction::Pinching { .. } | Interaction::Idle => {}
                }
            }
            CanvasInput::End(sample) => {
                self.sample_count += 1;
                match std::mem::replace(&mut self.interaction, Interaction::Idle) {
                    Interaction::Drawing { live, rect } => self.end_stroke(&sample, live, rect),
                    Interaction::Selecting => {
                        let point = self.to_document(sample.position);
                        let before = self.key();
                        let ended = self.tab.session.end_selection(&self.tools, point);
                        self.record_colors();
                        if self.tools.lasso_paints {
                            self.after_fill(before, ended);
                        }
                    }
                    Interaction::Transforming { .. } => self.drag_transform(sample.position),
                    moving @ Interaction::MovingText { .. } => {
                        self.interaction = moving;
                        self.drag_text(sample.position);
                        self.interaction = Interaction::Idle;
                    }
                    Interaction::Picking => self.pick(sample.position),
                    view @ (Interaction::Turning { .. } | Interaction::Zooming { .. }) => {
                        self.interaction = view;
                        self.drag_view(sample.position);
                        self.interaction = Interaction::Idle;
                    }
                    Interaction::Panning { .. }
                    | Interaction::Pinching { .. }
                    | Interaction::Idle => {}
                }
            }
            CanvasInput::Cancel => {
                self.cancel_gesture();
            }
            CanvasInput::Zoom { position, notches } => self.zoom(position, notches),
            CanvasInput::Turn { notches } => self.turn_by(TURN_STEP * f64::from(notches)),
            CanvasInput::Pinch { fingers, fresh } => self.pinch(fingers, fresh),
            CanvasInput::PinchEnd => {
                if matches!(self.interaction, Interaction::Pinching { .. }) {
                    self.interaction = Interaction::Idle;
                }
            }
        }
    }

    /// Ends a drawing, panning or selecting gesture without its result;
    /// returns whether there was one.
    fn cancel_gesture(&mut self) -> bool {
        match std::mem::replace(&mut self.interaction, Interaction::Idle) {
            Interaction::Drawing { rect, .. } => {
                self.tab.session.cancel_stroke();
                self.recomposite(rect);
            }
            Interaction::Selecting => {
                self.tab.session.cancel_selection();
            }
            Interaction::Transforming { base, .. } => {
                self.tab.session.set_transform(base);
                self.refresh_preview();
            }
            Interaction::MovingText { base, .. } => {
                if let Some(outline) = self
                    .tab
                    .session
                    .placed_text()
                    .map(|placed| placed.outline.clone())
                {
                    let _ = self.tab.session.place_text(&self.tools, base, outline);
                }
                self.refresh_text_preview();
            }
            Interaction::Panning { .. }
            | Interaction::Picking
            | Interaction::Turning { .. }
            | Interaction::Zooming { .. }
            | Interaction::Pinching { .. } => {}
            Interaction::Idle => return false,
        }
        true
    }

    /// Esc: ends the gesture under way without its result, else deselects.
    pub fn escape(&mut self) {
        if !self.cancel_gesture() {
            self.edit(Session::escape);
        }
    }

    /// The modifier keys held, which decide how a dragged shape selects.
    pub fn set_modifiers(&mut self, shift: bool, alt: bool) {
        self.modifiers = (shift, alt);
    }

    /// As in 2.2.13: Shift adds, Alt takes away, both replace.
    fn combine(&self) -> Combine {
        match self.modifiers {
            (true, false) => Combine::Add,
            (false, true) => Combine::Subtract,
            _ => Combine::Replace,
        }
    }

    /// A click with the wand or the bucket at `point`.
    fn click_area(&mut self, point: [f64; 2]) {
        let started = Instant::now();
        self.notice = None;
        self.pause_playback();
        // The area is read from the document as the fill will see it.
        if let Err(error) = self.tab.session.settle() {
            return self.fill_notice(&FillError::Edit(error));
        }
        let read = self.read_reference();
        let reference = self
            .tab
            .reference
            .as_ref()
            .filter(|_| read)
            .map(|pixels| pixels.data_as_u8_slice().as_chunks::<4>().0);
        let before = self.key();
        if self.tools.tool == Tool::Wand {
            let combine = self.combine();
            if let Err(error) = self
                .tab
                .session
                .wand(&self.tools, point, combine, reference)
            {
                self.fill_notice(&error);
            }
        } else {
            let filled = self.tab.session.bucket(&self.tools, point, reference);
            self.record_colors();
            self.after_fill(before, filled.map(committed));
        }
        tracing::debug!(
            ms = started.elapsed().as_secs_f64() * 1000.0,
            "area click shown"
        );
    }

    /// Fills the selection on the current layer.
    pub fn fill_selection(&mut self) {
        self.edit(|_| ());
        self.notice = None;
        let before = self.key();
        let filled = self.tab.session.fill_selection(&self.tools);
        self.record_colors();
        self.after_fill(before, filled.map(committed));
    }

    /// Draws `reference`'s pixels as the wand and the bucket read them into
    /// `self.tab.reference`: put together from the split when it shows this
    /// frame and holds the layers, else drawn by the worker. `false` when
    /// there is nothing to read.
    fn read_reference(&mut self) -> bool {
        let document = self.tab.session.document();
        let reference = match self.tools.fill.reads {
            Reads::Current => Reference::Layer(self.tab.session.current_layer()),
            Reads::Marked => Reference::Marked,
            Reads::Visible => Reference::Visible,
        };
        let plan = RenderPlan::reference(document, reference);
        if plan.layers.is_empty() {
            return false;
        }
        let [width, height] = document.canvas.map(|edge| edge as u16);
        let pixels = match self.tab.reference.take() {
            Some(pixels) if pixels.width() == width && pixels.height() == height => pixels,
            _ => Pixmap::new(width, height),
        };
        let mut pixels = pixels;
        let key = self.key();
        let threads = std::thread::available_parallelism().map_or(1, usize::from);
        let from_split = self.tab.split.as_ref().is_some_and(|(shown, split)| {
            *shown == key && split.reference(&plan, &mut pixels, threads)
        });
        if from_split {
            self.tab.reference = Some(pixels);
            return true;
        }
        let document = self.snapshot().document;
        self.tab.reference =
            self.tab
                .cache
                .renders()
                .render(document, plan, self.tab.session.frame());
        self.tab.reference.is_some()
    }

    /// Shows a fill just committed (`Ok(true)`), added to the split's layer
    /// when the split was of the state before; reports why there was none.
    fn after_fill(&mut self, before: Key, result: Result<bool, FillError>) {
        match result {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => return self.fill_notice(&error),
        }
        let after = self.key();
        let document = self.tab.session.document();
        let added = match self.tab.split.as_mut() {
            Some((key, split)) if *key == before && after.layer == before.layer => {
                last_fill(document, after.layer).and_then(|(coverage, clip, antialias, color)| {
                    let changed = split.stamp_fill(coverage, clip, antialias, color);
                    *key = after;
                    changed
                })
            }
            _ => None,
        };
        self.recomposite(added);
    }

    /// Why the current layer cannot be drawn on when it is no paint layer,
    /// in 2.2.13's words.
    fn no_paint_layer(&self) -> &'static str {
        if self.tab.session.document().layers.is_empty() {
            tr("draw-no-layer")
        } else {
            tr("draw-on-group")
        }
    }

    /// Why the current layer cannot be drawn on when it is not shown: it,
    /// or a group holding it, is hidden.
    fn hidden_layer(&self) -> &'static str {
        let layer = self.tab.session.current_layer();
        if self
            .tab
            .session
            .document()
            .layer(layer)
            .is_some_and(|layer| layer.visible)
        {
            tr("draw-group-hidden")
        } else {
            tr("draw-layer-hidden")
        }
    }

    fn fill_notice(&mut self, error: &FillError) {
        let reads = self.tools.fill.reads;
        self.notice = Some(match error {
            FillError::NoLayer => self.no_paint_layer().to_owned(),
            FillError::HiddenLayer => self.hidden_layer().to_owned(),
            FillError::NoReference if reads == Reads::Marked => tr("fill-no-reference").to_owned(),
            FillError::NoReference => self.no_paint_layer().to_owned(),
            FillError::NothingThere if self.tools.tool == Tool::Wand => {
                tr("wand-nothing").to_owned()
            }
            FillError::NothingThere => tr("fill-nothing").to_owned(),
            FillError::OutsideSelection => tr("fill-outside").to_owned(),
            FillError::NoSelection => tr("fill-no-selection").to_owned(),
            FillError::Pending => tr("restyle-pending").to_owned(),
            FillError::Edit(error) => failed_because(tr("fill-failed"), error),
        });
    }

    fn begin_stroke(&mut self, kind: PointerKind, sample: &PointerSample) {
        self.notice = None;
        self.pause_playback();
        let point = self.input_point(sample);
        // The eraser end of a pen erases whatever tool is chosen. A pending
        // transform or placed text is applied first, as changing tools does.
        let begun =
            if kind == PointerKind::Pen && sample.inverted && self.tools.tool != Tool::Eraser {
                let eraser = Tools {
                    tool: Tool::Eraser,
                    ..self.tools.clone()
                };
                self.tab.session.begin_stroke(&eraser, point)
            } else {
                self.tab.session.begin_stroke(&self.tools, point)
            };
        match begun {
            Ok(()) => {}
            Err(StrokeRefused::Pending) => {
                self.notice = Some(tr("restyle-pending").to_owned());
                return;
            }
            Err(StrokeRefused::HiddenLayer) => {
                self.notice = Some(self.hidden_layer().to_owned());
                return;
            }
            Err(StrokeRefused::NoLayer) => {
                self.notice = Some(self.no_paint_layer().to_owned());
                return;
            }
        }
        let live = self.tab.session.live().expect("a stroke has begun");
        let document = self.tab.session.document();
        let pen = Pen::new(
            &live.template,
            wobble_of(document, live.layer),
            document.frames,
        );
        let size = document.canvas.map(|edge| edge as u16);
        let mut stroke = LiveStroke::new(
            size,
            pen,
            self.key().frame,
            live.template.color.0,
            live.erase,
            self.spare_coverage.take(),
        )
        .with_clip(live.clip.as_ref().map(|selection| selection.mask().clone()));
        let rect = stroke.update(&live.points);
        self.interaction = Interaction::Drawing {
            live: Box::new(stroke),
            rect,
        };
        self.recomposite(rect);
    }

    fn extend_stroke(&mut self, sample: &PointerSample) {
        let point = self.input_point(sample);
        if !self.tab.session.extend_stroke(point) {
            return;
        }
        let points = &self.tab.session.live().expect("drawing").points;
        let Interaction::Drawing { live, rect } = &mut self.interaction else {
            return;
        };
        let changed = live.update(points);
        *rect = union(*rect, changed);
        self.recomposite(changed);
    }

    fn end_stroke(
        &mut self,
        sample: &PointerSample,
        live: Box<LiveStroke>,
        rect: Option<PixelRect>,
    ) {
        let started = Instant::now();
        let before = self.key();
        let point = self.input_point(sample);
        let outcome = self.tab.session.end_stroke(point);
        self.record_colors();
        self.spare_coverage = Some(live.into_spare());
        match outcome {
            Ok(Outcome::Committed(_)) => {}
            Ok(Outcome::NoChange) => return self.recomposite(rect),
            Err(error) => {
                self.notice = Some(if is_point_limit(&error) {
                    tracing::warn!(%error, "the stroke could not be committed");
                    tr("stroke-point-limit").to_owned()
                } else {
                    failed_because(tr("stroke-failed"), &error)
                });
                return self.recomposite(rect);
            }
        }
        let after = self.key();
        let document = self.tab.session.document();
        let added = match self.tab.split.as_mut() {
            Some((key, split)) if *key == before && after.layer == before.layer => {
                last_stroke(document, after.layer).and_then(|(stroke, erase, clip)| {
                    let wobble = wobble_of(document, after.layer);
                    let frames = document.frames;
                    let changed = split.stamp(&mut self.stamp, stroke, erase, wobble, frames, clip);
                    *key = after;
                    changed
                })
            }
            _ => None,
        };
        self.recomposite(union(rect, added));
        tracing::debug!(
            ms = started.elapsed().as_secs_f64() * 1000.0,
            "pen-up shown"
        );
    }

    fn zoom(&mut self, position: [f64; 2], notches: f32) {
        let anchor = self.to_document(position);
        let scale = (self.tab.scale * ZOOM_STEP.powf(f64::from(notches)))
            .clamp(*ZOOM_RANGE.start(), *ZOOM_RANGE.end());
        self.tab.scale = scale;
        // Keep the document point under the pointer where it is.
        self.tab.offset = [0.0, 0.0];
        let at = self.to_client(anchor);
        self.tab.offset = std::array::from_fn(|axis| (position[axis] - at[axis]).round());
    }

    /// Takes the rest of `ui` and returns the canvas area in client physical
    /// pixels, snapped to whole pixels so that canvas pixels map 1:1.
    pub fn layout(&mut self, ui: &mut egui::Ui) -> [i32; 4] {
        let rect = ui.max_rect();
        let pixels_per_point = ui.ctx().pixels_per_point();
        self.pixels_per_point = pixels_per_point;
        let edge = |value: f32| (value * pixels_per_point).round() as i32;
        let area = [
            edge(rect.left()),
            edge(rect.top()),
            edge(rect.right()),
            edge(rect.bottom()),
        ];
        self.set_area(area);
        area
    }

    /// A placed view keeps the document point at the area's centre there
    /// when the area grows or shrinks, as a resized or maximized window does.
    fn set_area(&mut self, area: [i32; 4]) {
        if let Some(old) = self.area.replace(area)
            && self.tab.placed
        {
            for axis in 0..2 {
                let grown = (area[axis + 2] - area[axis]) - (old[axis + 2] - old[axis]);
                self.tab.offset[axis] += f64::from(grown / 2);
            }
        }
        if !self.tab.placed {
            self.place(area);
        }
    }

    /// 100% if the document fits, otherwise small enough to fit, centred.
    /// A turned view fits the turned document's bounds, as in 2.2.13.
    fn place(&mut self, area: [i32; 4]) {
        let size = self.tab.session.document().canvas.map(f64::from);
        let room = [f64::from(area[2] - area[0]), f64::from(area[3] - area[1])];
        if room[0] <= 0.0 || room[1] <= 0.0 {
            return;
        }
        let [cos, sin] = self.turn().map(f64::abs);
        let bounds = [size[0] * cos + size[1] * sin, size[0] * sin + size[1] * cos];
        let fits = (room[0] / bounds[0]).min(room[1] / bounds[1]);
        self.tab.scale = if fits >= 2.0 && size[0].max(size[1]) <= SMALL_EDGE {
            fits.floor().min(*ZOOM_RANGE.end())
        } else {
            fits.min(1.0)
        };
        self.tab.offset = [0.0, 0.0];
        let middle = self.to_client(self.middle());
        let origin = [f64::from(area[0]), f64::from(area[1])];
        self.tab.offset =
            std::array::from_fn(|axis| (origin[axis] + room[axis] / 2.0 - middle[axis]).round());
        self.tab.placed = true;
    }

    /// Marks the whole display for upload, as a new GPU device needs.
    pub fn upload_all(&mut self) {
        let shown = self.display();
        let [width, height] = [shown.width(), shown.height()];
        self.upload = Some([0, 0, u32::from(width), u32::from(height)]);
    }

    /// The display pixels changed since the last call; all of them when
    /// other pixels are shown than at the last call.
    pub fn take_upload(&mut self) -> Option<PixelRect> {
        let (pixels, shrink, origin) = self.shown();
        let size = [pixels.width(), pixels.height()];
        let now = Some((size, shrink, origin));
        let rect = self.upload.take();
        if self.uploaded != now {
            self.uploaded = now;
            return Some([0, 0, u32::from(size[0]), u32::from(size[1])]);
        }
        rect
    }

    /// What the canvas shows: the playback frame, the frame playback
    /// stopped on, or the edited image.
    pub fn display(&self) -> &Pixmap {
        self.shown().0
    }

    /// The pixels shown, how much smaller than the canvas they are, and
    /// where on the canvas they start.
    fn shown(&self) -> (&Pixmap, u32, [u32; 2]) {
        if let Some((pixels, shrink)) = &self.tab.playback.as_ref().map(|playback| &playback.shown)
        {
            return (pixels, *shrink, [0, 0]);
        }
        if let Some(glance) = self
            .tab
            .preview
            .as_ref()
            .and_then(transform::Preview::glance)
        {
            return (glance.pixels(), glance.shrink(), glance.origin());
        }
        match &self.tab.held {
            Some((pixels, shrink)) => (pixels, *shrink, [0, 0]),
            None => (&self.tab.display, 1, [0, 0]),
        }
    }

    /// Where the pixels shown are drawn: pixels drawn smaller spread over
    /// more screen pixels, and a part of the canvas starts where it is.
    pub fn shown_placement(&self) -> Placement {
        let (_, shrink, origin) = self.shown();
        let document = self.placement();
        let [a, b, _, d, e, _] = document.affine().map(|value| value as f32);
        let [x, y] = origin.map(|value| value as f32);
        Placement {
            offset: [
                document.offset[0] + (x * a + y * b),
                document.offset[1] + (x * d + y * e),
            ],
            scale: document.scale * shrink as f32,
            ..document
        }
    }
}

/// How many playback frames of `document` drawn at 1/`shrink` fit in the
/// budget for memory as it is now beside the layers' own surfaces, which the
/// renderer keeps; at least 2. Also the surfaces' budget.
fn frames_ahead(document: &Document, shrink: u32) -> (u32, usize) {
    let plan = RenderPlan::new(document, Purpose::Display);
    let estimate = surface_estimate(document, &plan, shrink, TILE_EDGE);
    let budget = Budget::now(estimate as u64);
    let surfaces = estimate.min(budget.surfaces);
    let [width, height] = scaled_size(document.canvas, shrink);
    let frame = width as usize * height as usize * 4;
    let fit = (budget.render - surfaces) / frame.max(1);
    (
        fit.clamp(2, document.frames as usize) as u32,
        budget.surfaces,
    )
}

/// How playback frames are drawn at `scale` (screen pixels per document
/// pixel): as small as `preview_shrink` allows, with stroke samples as far
/// apart on screen as when editing at 100%. The spacing is kept in 16ths so
/// that small zoom changes during playback reuse the frames drawn.
fn preview(scale: f64) -> Preview {
    let detail = (f64::from(FULL_DETAIL) / scale).floor();
    Preview {
        shrink: preview_shrink(scale),
        detail: detail.clamp(f64::from(FULL_DETAIL), f64::from(FULL_DETAIL * 64)) as u32,
    }
}

/// How much smaller playback frames can be drawn at `scale` (screen pixels
/// per document pixel) and still have a pixel for every screen pixel: the
/// largest such power of two up to 8.
fn preview_shrink(scale: f64) -> u32 {
    let mut shrink = 1;
    while shrink < 8 && scale * f64::from(shrink * 2) <= 1.0 {
        shrink *= 2;
    }
    shrink
}

/// The wobble of strokes on `layer`.
fn wobble_of(document: &Document, layer: ugu_core::document::LayerId) -> ugu_core::ops::Wobble {
    match document.layer(layer).map(|layer| &layer.kind) {
        Some(LayerKind::Paint(paint)) => paint.wobble.unwrap_or(document.wobble),
        _ => document.wobble,
    }
}

/// The stroke the last operation of `layer` draws, and whether it erases.
fn last_stroke(
    document: &Document,
    layer: ugu_core::document::LayerId,
) -> Option<(
    &ugu_core::store::Stroke,
    bool,
    Option<&ugu_core::store::Mask>,
)> {
    let Some(LayerKind::Paint(paint)) = document.layer(layer).map(|layer| &layer.kind) else {
        return None;
    };
    let (id, erase, clip) = match paint.ops.last()? {
        Op::Paint { stroke, clip } => (stroke, false, clip),
        Op::Erase { stroke, clip } => (stroke, true, clip),
        _ => return None,
    };
    let clip = match clip {
        Some(mask) => Some(document.store.masks.get(mask)?),
        None => None,
    };
    document
        .store
        .strokes
        .get(id)
        .map(|stroke| (stroke, erase, clip))
}

fn committed(outcome: Outcome) -> bool {
    matches!(outcome, Outcome::Committed(_))
}

/// The coverage, clip, antialiasing and colour of the fill on top of
/// `layer`'s operations.
fn last_fill(
    document: &Document,
    layer: ugu_core::document::LayerId,
) -> Option<(
    &ugu_core::store::Mask,
    Option<&ugu_core::store::Mask>,
    bool,
    [u8; 4],
)> {
    let Some(LayerKind::Paint(paint)) = document.layer(layer).map(|layer| &layer.kind) else {
        return None;
    };
    let Op::Fill {
        coverage,
        color,
        antialias,
        clip,
    } = paint.ops.last()?
    else {
        return None;
    };
    let masks = &document.store.masks;
    let clip = match clip {
        Some(mask) => Some(masks.get(mask)?),
        None => None,
    };
    Some((masks.get(coverage)?, clip, *antialias, color.0))
}

/// Calls `each` with every paint layer's id, inside groups too.
/// Each layer with what its thumbnail shows: a paint layer's pixels, or what
/// a group's children are and how they are blended, clipped and shown.
fn thumbnail_keys(
    layers: &[ugu_core::document::Layer],
    revisions: &ugu_core::history::LayerRevisions,
    each: &mut impl FnMut(LayerId, u64),
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut all = std::collections::hash_map::DefaultHasher::new();
    for layer in layers {
        let key = match &layer.kind {
            LayerKind::Paint(_) => revisions.of(layer.id),
            LayerKind::Group(group) => {
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                thumbnail_keys(&group.children, revisions, each).hash(&mut hasher);
                hasher.finish()
            }
        };
        each(layer.id, key);
        let (opacity, blend, clip) = match &layer.kind {
            LayerKind::Paint(paint) => (paint.opacity, paint.blend, paint.clip_to_below),
            LayerKind::Group(group) => (group.opacity, group.blend, group.clip_to_below),
        };
        (
            layer.id.0,
            key,
            layer.visible,
            layer.reference,
            opacity.to_bits(),
            std::mem::discriminant(&blend),
            clip,
        )
            .hash(&mut all);
    }
    all.finish()
}

/// Why an edit was refused, in the interface's words: a limit the user can
/// act on, or else where the details are.
pub(crate) fn edit_reason(error: &EditError) -> &'static str {
    let store = match error {
        EditError::Store(store) | EditError::Document(DocumentError::Store(store)) => Some(store),
        _ => None,
    };
    match (error, store) {
        (
            _,
            Some(
                StoreError::TooManyPoints(_)
                | StoreError::TooManyBytes(_)
                | StoreError::TooManyPixels(_),
            ),
        )
        | (EditError::Document(DocumentError::TooManyOperations(_)), _) => tr("edit-data-limit"),
        (EditError::Document(DocumentError::TooManyLayers(_)), _) => tr("edit-layer-limit"),
        (EditError::Document(DocumentError::TooDeep(_)), _) => tr("edit-too-deep"),
        (
            EditError::CannotMerge | EditError::Merge(MergeRefusal::Blend | MergeRefusal::Clipping),
            _,
        ) => tr("merge-properties"),
        (EditError::Merge(MergeRefusal::CanvasChange), _) => tr("merge-canvas-change"),
        (EditError::Merge(MergeRefusal::CanvasEpoch), _) => tr("merge-epoch"),
        _ => tr("edit-internal"),
    }
}

/// `failed`, then why; the error itself goes to the log.
pub(crate) fn failed_because(failed: &str, error: &EditError) -> String {
    tracing::warn!(%error, failed, "an edit was refused");
    format!("{failed} {}", edit_reason(error))
}

/// Whether `error` is the project's point limit, which 2.2.13 names.
pub(crate) fn is_point_limit(error: &EditError) -> bool {
    matches!(
        error,
        EditError::Store(StoreError::TooManyPoints(_))
            | EditError::Document(DocumentError::Store(StoreError::TooManyPoints(_)))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each tab's width, in order.
    fn widths(canvas: &Canvas) -> Vec<u32> {
        (0..canvas.tab_count())
            .map(|index| canvas.tab_at(index).session.document().canvas[0])
            .collect()
    }

    #[test]
    fn tabs_keep_their_documents_and_changes_apart() {
        let mut canvas = Canvas::new(Document::new([64, 64]), |_| {});
        canvas.open_tab(Document::new([32, 16]), true);
        assert_eq!((canvas.tab_count(), canvas.tab_position()), (2, 1));
        assert_eq!(canvas.session().document().canvas, [32, 16]);
        canvas.edit(Session::add_layer).unwrap();
        let edited = canvas.generation();
        canvas.show_tab(0);
        assert_eq!(canvas.session().document().canvas, [64, 64]);
        assert!(!canvas.session().is_dirty());
        assert_ne!(canvas.generation(), edited);
        assert_eq!(canvas.dirty_tab(&[]), Some(1));
        assert_eq!(canvas.dirty_tab(&[edited]), None);
        assert_eq!(canvas.tab_generations()[1], edited);
        canvas.show_tab(1);
        assert!(canvas.session().is_dirty());
        assert_eq!(canvas.session().document().layers.len(), 2);
    }

    #[test]
    fn tabs_of_the_same_name_are_numbered_in_the_order_opened() {
        let mut canvas = Canvas::new(Document::new([10, 10]), |_| {});
        canvas.open_tab(Document::new([20, 20]), true);
        canvas.file_mut().unsaved_name = Some("Cat".to_owned());
        canvas.open_tab(Document::new([30, 30]), true);
        canvas.open_tab(Document::new([40, 40]), true);
        canvas.file_mut().unsaved_name = Some("Cat".to_owned());
        let names = |canvas: &Canvas| -> Vec<String> {
            canvas
                .tab_infos()
                .into_iter()
                .map(|info| info.name)
                .collect()
        };
        let untitled = crate::i18n::tr("untitled");
        assert_eq!(
            names(&canvas),
            [
                untitled.to_owned(),
                "Cat".to_owned(),
                format!("{untitled} (2)"),
                "Cat (2)".to_owned()
            ]
        );
        canvas.move_tab(3, 0);
        assert_eq!(names(&canvas)[0], "Cat (2)");
        assert_eq!(names(&canvas)[2], "Cat");
    }

    #[test]
    fn moving_and_closing_tabs_keeps_the_order_and_the_one_shown() {
        let mut canvas = Canvas::new(Document::new([10, 10]), |_| {});
        canvas.open_tab(Document::new([20, 20]), true);
        canvas.open_tab(Document::new([30, 30]), true);
        assert_eq!(widths(&canvas), [10, 20, 30]);
        canvas.move_tab(2, 0);
        assert_eq!(
            (widths(&canvas), canvas.tab_position()),
            (vec![30, 10, 20], 0)
        );
        canvas.move_tab(1, 2);
        assert_eq!(
            (widths(&canvas), canvas.tab_position()),
            (vec![30, 20, 10], 0)
        );
        canvas.show_tab(2);
        canvas.move_tab(0, 1);
        assert_eq!(
            (widths(&canvas), canvas.tab_position()),
            (vec![20, 30, 10], 2)
        );
        // Closing one not shown keeps the one shown.
        canvas.close_tab(0, Document::new([1, 1]));
        assert_eq!((widths(&canvas), canvas.tab_position()), (vec![30, 10], 1));
        // Closing the last one shows the one before it.
        canvas.close_tab(1, Document::new([1, 1]));
        assert_eq!((widths(&canvas), canvas.tab_position()), (vec![30], 0));
        assert_eq!(canvas.session().document().canvas, [30, 30]);
        // And closing the only one leaves an empty document.
        canvas.close_tab(0, Document::new([5, 5]));
        assert_eq!(widths(&canvas), [5]);
        assert!(canvas.is_pristine());
    }

    #[test]
    fn playback_frames_keep_a_pixel_for_every_screen_pixel() {
        let shrinks = [1.5, 1.0, 0.6, 0.5, 0.3, 0.25, 0.2, 0.05].map(super::preview_shrink);
        assert_eq!(shrinks, [1, 1, 1, 2, 2, 4, 4, 8]);
    }

    #[test]
    fn playback_samples_are_as_far_apart_on_screen_as_at_100_percent() {
        let details = [2.0, 1.0, 0.73, 0.5, 0.3, 0.001].map(|scale| super::preview(scale).detail);
        assert_eq!(details, [16, 16, 21, 32, 53, 1024]);
    }

    #[test]
    fn fitting_enlarges_only_small_documents_by_whole_times() {
        let scale = |canvas: [u32; 2]| {
            let mut canvas = super::Canvas::new(Document::new(canvas), |_| {});
            canvas.set_area([0, 0, 1000, 700]);
            canvas.fit();
            canvas.placement().scale
        };
        assert_eq!(scale([16, 16]), 43.0);
        assert_eq!(scale([8, 8]), 64.0, "no further than the zoom goes");
        assert_eq!(scale([256, 200]), 3.0);
        assert_eq!(scale([300, 200]), 1.0, "a larger one at most at 100%");
        assert_eq!(scale([400, 300]), 1.0);
        assert!(scale([2000, 1000]) < 1.0);
    }

    #[test]
    fn a_resized_area_keeps_the_document_point_at_its_centre() {
        let mut canvas = super::Canvas::new(Document::new([320, 200]), |_| {});
        canvas.set_area([0, 0, 400, 300]);
        assert_eq!(canvas.document_point([200.0, 150.0]), [160.0, 100.0]);
        canvas.set_area([40, 30, 1240, 830]);
        assert_eq!(canvas.document_point([640.0, 430.0]), [160.0, 100.0]);
        canvas.set_area([0, 0, 300, 200]);
        assert_eq!(canvas.document_point([150.0, 100.0]), [160.0, 100.0]);
    }

    #[test]
    fn a_playback_frame_drawn_smaller_is_uploaded_whole_and_no_more() {
        let (to_test, renders) = std::sync::mpsc::channel();
        let mut canvas = super::Canvas::new(Document::new([320, 200]), move |rendered| {
            let _ = to_test.send(rendered);
        });
        canvas.tab.scale = 0.5;
        canvas.toggle_playback();
        canvas.take_upload();
        let mut now = Instant::now();
        while canvas.display().width() != 160 {
            canvas.tick(now);
            now += Duration::from_millis(500);
            if let Ok(rendered) = renders.recv_timeout(Duration::from_secs(10)) {
                canvas.adopt(rendered);
            }
        }
        assert_eq!(canvas.take_upload(), Some([0, 0, 160, 100]));

        // Stopped, the frame stays until the split of it arrives.
        canvas.toggle_playback();
        assert_eq!(canvas.display().width(), 160);
        assert_eq!(canvas.shown_placement().scale, 1.0);
        canvas.sync();
        while canvas.tab.split.is_none() {
            let rendered = renders
                .recv_timeout(Duration::from_secs(10))
                .expect("the split renders");
            canvas.adopt(rendered);
        }
        assert_eq!(canvas.display().width(), 320);
        assert_eq!(canvas.shown_placement().scale, 0.5);
    }

    #[test]
    fn drawing_or_filling_pauses_playback_on_the_frame_shown_and_plays_on() {
        for tool in [Tool::Pen, Tool::Fill, Tool::Wand] {
            let (to_test, renders) = std::sync::mpsc::channel();
            let mut canvas = super::Canvas::new(Document::new([320, 200]), move |rendered| {
                let _ = to_test.send(rendered);
            });
            canvas.tab.scale = 0.5;
            canvas.set_tool(tool).unwrap();
            canvas.toggle_playback();
            let mut now = Instant::now();
            while canvas.display().width() != 160 {
                canvas.tick(now);
                now += Duration::from_millis(500);
                if let Ok(rendered) = renders.recv_timeout(Duration::from_secs(10)) {
                    canvas.adopt(rendered);
                }
            }
            canvas.apply(CanvasInput::Begin(
                Gesture::Draw,
                PointerKind::Mouse,
                press(60.0, 40.0),
            ));
            assert!(!canvas.is_playing());
            assert_eq!(canvas.display().width(), 160, "{tool:?}");
            assert_eq!(canvas.shown_placement().scale, 1.0, "{tool:?}");
            if tool == Tool::Pen {
                canvas.apply(CanvasInput::Extend(press(90.0, 50.0)));
                canvas.tick(now);
                assert!(!canvas.is_playing(), "still drawing");
            }
            canvas.apply(CanvasInput::End(press(90.0, 50.0)));
            canvas.tick(now);
            assert!(canvas.is_playing(), "{tool:?}");
        }
    }

    #[test]
    fn refusals_are_told_in_the_interface_language() {
        let mut canvas = super::Canvas::new(Document::new([64, 64]), |_| {});
        let layer = canvas.session().current_layer();
        canvas
            .edit(|session| session.update_layer(layer, "Hide", |layer| layer.visible = false))
            .unwrap();
        canvas.apply(CanvasInput::Begin(
            Gesture::Draw,
            PointerKind::Mouse,
            press(10.0, 10.0),
        ));
        assert_eq!(canvas.notice(), Some(tr("draw-layer-hidden")));

        let limit = EditError::Document(DocumentError::TooManyLayers(1000));
        assert_eq!(super::edit_reason(&limit), tr("edit-layer-limit"));
        let points = EditError::Store(StoreError::TooManyPoints(9));
        assert!(super::is_point_limit(&points));
        assert_eq!(super::edit_reason(&points), tr("edit-data-limit"));
        assert_eq!(
            super::edit_reason(&EditError::Merge(MergeRefusal::CanvasEpoch)),
            tr("merge-epoch")
        );
        assert_eq!(
            super::edit_reason(&EditError::NoSuchLayer(LayerId(9))),
            tr("edit-internal")
        );
        assert_eq!(
            super::failed_because(tr("fill-failed"), &limit),
            format!("{} {}", tr("fill-failed"), tr("edit-layer-limit"))
        );
    }

    #[test]
    fn stopping_playback_during_a_stroke_keeps_it_stopped() {
        let mut canvas = super::Canvas::new(Document::new([64, 64]), |_| {});
        canvas.toggle_playback();
        canvas.apply(CanvasInput::Begin(
            Gesture::Draw,
            PointerKind::Mouse,
            press(10.0, 10.0),
        ));
        canvas.toggle_playback();
        canvas.apply(CanvasInput::End(press(20.0, 20.0)));
        canvas.tick(Instant::now());
        assert!(!canvas.is_playing());
    }

    #[test]
    fn the_wobble_animation_setting_stops_and_blocks_playback() {
        let mut canvas = super::Canvas::new(Document::new([64, 64]), |_| {});
        canvas.toggle_playback();
        assert!(canvas.is_playing());
        canvas.allow_animation(false);
        assert!(!canvas.is_playing());
        canvas.toggle_playback();
        assert!(!canvas.is_playing());
        canvas.allow_animation(true);
        canvas.toggle_playback();
        assert!(canvas.is_playing());
    }

    #[test]
    fn playback_follows_a_canvas_change() {
        let (to_test, renders) = std::sync::mpsc::channel();
        let mut canvas = super::Canvas::new(Document::new([320, 200]), move |rendered| {
            let _ = to_test.send(rendered);
        });
        canvas.tab.scale = 0.5;
        canvas.toggle_playback();
        let mut now = Instant::now();
        let mut play_until = |canvas: &mut super::Canvas, width: u16| {
            while canvas.display().width() != width {
                canvas.tick(now);
                now += Duration::from_millis(500);
                if let Ok(rendered) = renders.recv_timeout(Duration::from_secs(10)) {
                    canvas.adopt(rendered);
                }
            }
        };
        play_until(&mut canvas, 160);
        canvas
            .edit(|session| session.crop_canvas([-20, 10], [200, 150]))
            .unwrap();
        canvas.take_upload();
        play_until(&mut canvas, 100);
        assert_eq!(canvas.display().height(), 75);
        assert_eq!(canvas.take_upload(), Some([0, 0, 100, 75]));
        canvas
            .edit(|session| session.resample_image([300, 225], ugu_core::ops::Sampling::Smooth))
            .unwrap();
        play_until(&mut canvas, 150);
        assert_eq!(canvas.display().height(), 113);
    }

    /// Waits for the split of the state shown.
    fn settle(canvas: &mut super::Canvas, renders: &std::sync::mpsc::Receiver<Rendered>) {
        canvas.sync();
        while canvas.tab.split.as_ref().map(|(key, _)| *key) != Some(canvas.key()) {
            let rendered = renders
                .recv_timeout(Duration::from_secs(10))
                .expect("the split renders");
            canvas.adopt(rendered);
        }
    }

    #[test]
    fn a_bucket_fill_shows_at_once_as_a_full_render_and_the_wand_reads_alike_without_a_split() {
        let (to_test, renders) = std::sync::mpsc::channel();
        let mut canvas = super::Canvas::new(Document::new([320, 200]), move |rendered| {
            let _ = to_test.send(rendered);
        });
        // A closed box of pen strokes.
        let corners = [
            [60.0, 40.0],
            [260.0, 40.0],
            [260.0, 160.0],
            [60.0, 160.0],
            [60.0, 40.0],
        ];
        let tools = canvas.tools().clone();
        canvas.edit(|session| {
            let point = |position, time| ugu_session::InputPoint {
                position,
                pressure: None,
                time,
            };
            session
                .begin_stroke(&tools, point(corners[0], 0.0))
                .unwrap();
            let mut time = 0.0;
            for pair in corners.windows(2) {
                for step in 1..=40 {
                    let t = f64::from(step) / 40.0;
                    time += 4.0;
                    let at = [0, 1].map(|axis| pair[0][axis] + (pair[1][axis] - pair[0][axis]) * t);
                    session.extend_stroke(point(at, time));
                }
            }
            session.end_stroke(point(corners[4], time + 4.0)).unwrap();
        });
        settle(&mut canvas, &renders);

        canvas.set_tool(Tool::Wand).unwrap();
        canvas.click_area([160.0, 100.0]);
        let from_split = canvas
            .session()
            .selection()
            .cloned()
            .expect("the box's inside");
        canvas.edit(Session::deselect).unwrap();
        canvas.tab.split = None;
        canvas.click_area([160.0, 100.0]);
        assert_eq!(canvas.session().selection(), Some(&from_split));
        canvas.edit(Session::deselect).unwrap();
        settle(&mut canvas, &renders);

        canvas.set_tool(Tool::Fill).unwrap();
        canvas.tools.color = ugu_core::ops::Rgba8([30, 120, 200, 255]);
        canvas.click_area([160.0, 100.0]);
        assert_eq!(canvas.notice(), None);
        // Added to the split, not drawn again.
        assert_eq!(
            canvas.tab.split.as_ref().map(|(key, _)| *key),
            Some(canvas.key())
        );
        let document = canvas.session().document();
        let mut full = Pixmap::new(320, 200);
        ugu_render::document::DocumentRenderer::new(0).render(
            document,
            canvas.session().frame(),
            Purpose::Display,
            &mut full,
        );
        let most = canvas
            .display()
            .data_as_u8_slice()
            .iter()
            .zip(full.data_as_u8_slice())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(most <= 2, "shown differs by {most}");
        let middle = (100 * 320 + 160) * 4;
        assert_eq!(
            &full.data_as_u8_slice()[middle..middle + 4],
            &[30, 120, 200, 255]
        );
    }

    fn max_difference(a: &Pixmap, b: &Pixmap) -> u8 {
        a.data_as_u8_slice()
            .iter()
            .zip(b.data_as_u8_slice())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap()
    }

    #[test]
    fn a_dragged_transform_shows_at_once_and_applying_keeps_it_while_cancelling_puts_back() {
        let (to_test, renders) = std::sync::mpsc::channel();
        let mut canvas = super::Canvas::new(Document::new([320, 200]), move |rendered| {
            let _ = to_test.send(rendered);
        });
        let tools = canvas.tools().clone();
        canvas.edit(|session| {
            let point = |position, time| ugu_session::InputPoint {
                position,
                pressure: None,
                time,
            };
            session
                .begin_stroke(&tools, point([40.0, 40.0], 0.0))
                .unwrap();
            for step in 1..=60 {
                let t = f64::from(step) / 60.0;
                let at = [40.0 + 200.0 * t, 40.0 + 100.0 * t + (t * 12.0).sin() * 20.0];
                session.extend_stroke(point(at, f64::from(step) * 4.0));
            }
            session.end_stroke(point([240.0, 140.0], 250.0)).unwrap();
            session.begin_selection(
                ugu_session::ShapeKind::Rectangle,
                [60.0, 50.0],
                Combine::Replace,
            );
            session.extend_selection([180.0, 130.0]);
            session.end_selection(&tools, [180.0, 130.0]).unwrap();
        });
        settle(&mut canvas, &renders);
        let before = canvas.display().clone();

        canvas.begin_transform();
        assert!(canvas.tab.preview.is_some());
        assert_eq!(canvas.grip_at([120.0, 90.0]), Some(Grip::Move));
        assert_eq!(canvas.grip_at([180.0, 130.0]), Some(Grip::Scale([1, 1])));
        assert_eq!(canvas.grip_at([300.0, 10.0]), Some(Grip::Rotate));
        canvas.begin_transform_drag([120.0, 90.0]);
        canvas.drag_transform([150.5, 97.25]);
        canvas.sync();
        canvas.interaction = Interaction::Idle;
        canvas.begin_transform_drag([300.0, 10.0]);
        canvas.drag_transform([310.0, 40.0]);
        canvas.sync();
        canvas.interaction = Interaction::Idle;
        assert_ne!(
            canvas.display().data_as_u8_slice(),
            before.data_as_u8_slice()
        );
        let key = canvas.key();
        canvas.apply_transform();
        assert!(canvas.session().pending().is_none());
        // Kept, not drawn again.
        assert_ne!(canvas.key(), key);
        assert_eq!(
            canvas.tab.split.as_ref().map(|(key, _)| *key),
            Some(canvas.key())
        );
        let mut full = Pixmap::new(320, 200);
        ugu_render::document::DocumentRenderer::new(0).render(
            canvas.session().document(),
            canvas.session().frame(),
            Purpose::Display,
            &mut full,
        );
        let most = max_difference(canvas.display(), &full);
        assert!(most <= 2, "shown differs by {most}");

        let applied = canvas.display().clone();
        canvas.begin_transform();
        canvas.begin_transform_drag([150.0, 100.0]);
        canvas.drag_transform([100.0, 60.0]);
        canvas.sync();
        canvas.interaction = Interaction::Idle;
        assert_ne!(
            canvas.display().data_as_u8_slice(),
            applied.data_as_u8_slice()
        );
        canvas.escape();
        assert!(canvas.session().pending().is_none());
        assert!(canvas.session().selection().is_some());
        assert_eq!(
            canvas.display().data_as_u8_slice(),
            applied.data_as_u8_slice()
        );
    }

    #[test]
    fn a_canvas_change_during_a_shown_transform_applies_it_and_shows_the_new_canvas() {
        let (to_test, renders) = std::sync::mpsc::channel();
        let mut canvas = super::Canvas::new(Document::new([320, 200]), move |rendered| {
            let _ = to_test.send(rendered);
        });
        let tools = canvas.tools().clone();
        canvas.edit(|session| {
            let point = |position, time| ugu_session::InputPoint {
                position,
                pressure: None,
                time,
            };
            session
                .begin_stroke(&tools, point([40.0, 40.0], 0.0))
                .unwrap();
            for step in 1..=60 {
                let t = f64::from(step) / 60.0;
                session.extend_stroke(point([40.0 + 200.0 * t, 40.0 + 100.0 * t], t * 250.0));
            }
            session.end_stroke(point([240.0, 140.0], 250.0)).unwrap();
            session.begin_selection(
                ugu_session::ShapeKind::Rectangle,
                [60.0, 50.0],
                Combine::Replace,
            );
            session.end_selection(&tools, [180.0, 130.0]).unwrap();
        });
        settle(&mut canvas, &renders);
        let full = |canvas: &super::Canvas| {
            let [width, height] = canvas.session().document().canvas.map(|edge| edge as u16);
            let mut full = Pixmap::new(width, height);
            ugu_render::document::DocumentRenderer::new(0).render(
                canvas.session().document(),
                canvas.session().frame(),
                Purpose::Display,
                &mut full,
            );
            full
        };
        for change in 0..2 {
            canvas.begin_transform();
            canvas.begin_transform_drag([120.0, 90.0]);
            canvas.drag_transform([150.0, 100.0]);
            canvas.sync();
            canvas.interaction = Interaction::Idle;
            assert!(canvas.tab.preview.is_some());
            if change == 0 {
                canvas
                    .edit(|session| session.crop_canvas([-20, 10], [300, 220]))
                    .unwrap();
            } else {
                canvas
                    .edit(|session| {
                        session.resample_image([150, 110], ugu_core::ops::Sampling::Smooth)
                    })
                    .unwrap();
            }
            assert!(canvas.tab.preview.is_none() && canvas.session().pending().is_none());
            settle(&mut canvas, &renders);
            let most = max_difference(canvas.display(), &full(&canvas));
            assert!(most <= 2, "shown differs by {most}");
        }
        for _ in 0..4 {
            canvas.edit(Session::undo).unwrap();
            settle(&mut canvas, &renders);
            let most = max_difference(canvas.display(), &full(&canvas));
            assert!(most <= 2, "shown differs by {most} after undo");
        }
        assert_eq!(canvas.session().document().canvas, [320, 200]);
    }

    #[test]
    fn placed_text_shows_as_applying_it_draws_and_cancelling_puts_back() {
        let (to_test, renders) = std::sync::mpsc::channel();
        let mut canvas = super::Canvas::new(Document::new([320, 200]), move |rendered| {
            let _ = to_test.send(rendered);
        });
        let tools = canvas.tools().clone();
        canvas.edit(|session| {
            let point = |position, time| ugu_session::InputPoint {
                position,
                pressure: None,
                time,
            };
            session
                .begin_stroke(&tools, point([20.0, 150.0], 0.0))
                .unwrap();
            session.end_stroke(point([300.0, 60.0], 100.0)).unwrap();
        });
        canvas.set_tool(Tool::Text).unwrap();
        canvas.brushes_mut().pen.width = 3.0;
        canvas.set_text(|text, settings| {
            *text = "Ug\n우글".to_owned();
            settings.size = 64.0;
            settings.filled = true;
        });
        settle(&mut canvas, &renders);
        let before = canvas.display().clone();
        let full = |canvas: &super::Canvas| {
            let mut full = Pixmap::new(320, 200);
            ugu_render::document::DocumentRenderer::new(0).render(
                canvas.session().document(),
                canvas.session().frame(),
                Purpose::Display,
                &mut full,
            );
            full
        };

        canvas.press_text([40.0, 30.0]);
        canvas.drag_text([60.0, 40.0]);
        canvas.interaction = Interaction::Idle;
        canvas.sync();
        assert!(canvas.tab.text_preview.is_some());
        assert_eq!(canvas.session().placed_text().unwrap().at, [60.0, 40.0]);
        assert_ne!(
            canvas.display().data_as_u8_slice(),
            before.data_as_u8_slice()
        );
        // A press on the text moves it rather than placing it anew.
        canvas.press_text([70.0, 50.0]);
        assert!(matches!(canvas.interaction, Interaction::MovingText { .. }));
        canvas.interaction = Interaction::Idle;
        canvas.cancel_text();
        assert_eq!(
            canvas.display().data_as_u8_slice(),
            before.data_as_u8_slice()
        );

        canvas.press_text([40.0, 30.0]);
        canvas.interaction = Interaction::Idle;
        canvas.sync();
        let key = canvas.key();
        canvas.apply_text();
        assert!(canvas.session().placed_text().is_none());
        // Kept, not drawn again.
        assert_ne!(canvas.key(), key);
        assert_eq!(
            canvas.tab.split.as_ref().map(|(key, _)| *key),
            Some(canvas.key())
        );
        let most = max_difference(canvas.display(), &full(&canvas));
        assert!(most <= 2, "shown differs by {most}");
        canvas.edit(Session::undo).unwrap();
        settle(&mut canvas, &renders);
        assert_eq!(
            canvas.display().data_as_u8_slice(),
            before.data_as_u8_slice()
        );
    }

    fn layer_render(document: &Document, layer: LayerId, frame: i64) -> Pixmap {
        let plan = RenderPlan::reference(document, Reference::Layer(layer));
        let [width, height] = document.canvas.map(|edge| edge as u16);
        let mut pixels = Pixmap::new(width, height);
        ugu_render::document::DocumentRenderer::new(0).render_plan(
            document,
            &plan,
            frame,
            None,
            &mut pixels,
        );
        pixels
    }

    #[test]
    fn pasted_strokes_keep_moving_inside_the_selection_on_any_canvas() {
        let (to_test, renders) = std::sync::mpsc::channel();
        let mut canvas = super::Canvas::new(Document::new([200, 120]), move |rendered| {
            let _ = to_test.send(rendered);
        });
        let tools = canvas.tools().clone();
        canvas.edit(|session| {
            let point = |position, time| ugu_session::InputPoint {
                position,
                pressure: None,
                time,
            };
            session
                .begin_stroke(&tools, point([10.0, 60.0], 0.0))
                .unwrap();
            for step in 1..=60 {
                let x = 10.0 + 3.0 * f64::from(step);
                session.extend_stroke(point(
                    [x, 60.0 + (x / 9.0).sin() * 20.0],
                    f64::from(step) * 4.0,
                ));
            }
            session.end_stroke(point([190.0, 60.0], 250.0)).unwrap();
            session.begin_selection(
                ugu_session::ShapeKind::Rectangle,
                [50.0, 20.0],
                Combine::Replace,
            );
            session.extend_selection([120.0, 100.0]);
            session.end_selection(&tools, [120.0, 100.0]).unwrap();
        });
        settle(&mut canvas, &renders);
        let source = canvas.session().current_layer();
        let (clip, image) = canvas.copy().expect("copied");
        assert_eq!(image.size, [70, 80]);
        assert_eq!(image.straight.len(), 70 * 80 * 4);
        assert!(image.straight.chunks(4).any(|pixel| pixel[3] == 255));

        canvas.paste(&clip);
        let pasted = canvas.session().current_layer();
        assert_ne!(pasted, source);
        assert!(canvas.session().pending().is_some());
        canvas.apply_transform();
        let document = canvas.session().document();
        let inside = |x: usize, y: usize| (50..120).contains(&x) && (20..100).contains(&y);
        let mut frames = Vec::new();
        for frame in [0, 3] {
            let original = layer_render(document, source, frame);
            let copy = layer_render(document, pasted, frame);
            for (index, (a, b)) in original
                .data_as_u8_slice()
                .chunks(4)
                .zip(copy.data_as_u8_slice().chunks(4))
                .enumerate()
            {
                let (x, y) = (index % 200, index / 200);
                if inside(x, y) {
                    assert_eq!(a, b, "frame {frame} at {x}, {y}");
                } else {
                    assert_eq!(b, [0; 4], "frame {frame} outside at {x}, {y}");
                }
            }
            frames.push(copy);
        }
        assert_ne!(
            frames[0].data_as_u8_slice(),
            frames[1].data_as_u8_slice(),
            "the pasted strokes move"
        );

        // Into a larger document: the strokes stay where they were.
        canvas.replace(Document::new([300, 200]), false);
        canvas.paste(&clip);
        canvas.apply_transform();
        let document = canvas.session().document();
        assert!(document.validate().is_ok());
        let wider = layer_render(document, canvas.session().current_layer(), 0);
        let shown = |pixels: &Pixmap, width: usize| {
            pixels
                .data_as_u8_slice()
                .chunks(4)
                .enumerate()
                .filter(|(_, pixel)| pixel[3] > 0)
                .map(|(index, _)| (index % width, index / width))
                .collect::<Vec<_>>()
        };
        assert_eq!(shown(&wider, 300), shown(&frames[0], 200));
    }

    fn press(x: f64, y: f64) -> PointerSample {
        PointerSample {
            position: [x, y],
            pressure: None,
            tilt: None,
            twist: None,
            buttons: ugu_win::pointer::Buttons::empty(),
            in_contact: true,
            inverted: false,
            time: Ticks(0),
            frame_id: 0,
        }
    }

    // Synthetic pen input cannot carry the eraser end (Windows drops the pen
    // flags it is injected with), so this is where it is checked.
    #[test]
    fn a_pens_eraser_end_erases_whatever_tool_is_chosen() {
        let mut canvas = super::Canvas::new(Document::new([320, 200]), |_| {});
        let eraser_end = |x, y| PointerSample {
            pressure: Some(0.5),
            inverted: true,
            ..press(x, y)
        };
        canvas.apply(CanvasInput::Begin(
            Gesture::Draw,
            PointerKind::Pen,
            eraser_end(20.0, 100.0),
        ));
        canvas.apply(CanvasInput::Extend(eraser_end(150.0, 100.0)));
        canvas.apply(CanvasInput::End(eraser_end(300.0, 100.0)));
        let layer = canvas.session().current_layer();
        let LayerKind::Paint(paint) = &canvas.session().document().layer(layer).unwrap().kind
        else {
            unreachable!()
        };
        assert!(matches!(paint.ops.last(), Some(Op::Erase { .. })));
        assert_eq!(canvas.tool(), Tool::Pen);
    }

    #[test]
    fn the_eyedropper_picks_what_is_shown_and_alt_picks_only_with_painting_tools() {
        let (to_test, renders) = std::sync::mpsc::channel();
        let mut canvas = super::Canvas::new(Document::new([320, 200]), move |rendered| {
            let _ = to_test.send(rendered);
        });
        let red = ugu_core::ops::Rgba8([200, 30, 30, 255]);
        canvas.tools.color = red;
        canvas.brushes_mut().pen.width = 20.0;
        let tools = canvas.tools().clone();
        canvas.edit(|session| {
            let point = |position, time| InputPoint {
                position,
                pressure: None,
                time,
            };
            session
                .begin_stroke(&tools, point([20.0, 100.0], 0.0))
                .unwrap();
            session.end_stroke(point([300.0, 100.0], 100.0)).unwrap();
        });
        canvas.tools.color = ugu_core::ops::Rgba8([0, 0, 0, 255]);
        settle(&mut canvas, &renders);
        let white = ugu_core::ops::Rgba8([255, 255, 255, 255]);
        let pen = |canvas: &super::Canvas| canvas.tools().color;
        let ops = |canvas: &super::Canvas| {
            let layer = canvas.session().current_layer();
            match &canvas.session().document().layer(layer).unwrap().kind {
                LayerKind::Paint(paint) => paint.ops.len(),
                LayerKind::Group(_) => unreachable!(),
            }
        };

        // Alt with the brush picks, and keeps picking while dragged.
        canvas.set_modifiers(false, true);
        let draw = |at| CanvasInput::Begin(Gesture::Draw, PointerKind::Mouse, at);
        canvas.apply(draw(press(150.0, 100.0)));
        assert_eq!(pen(&canvas), red);
        canvas.apply(CanvasInput::Extend(press(150.0, 20.0)));
        assert_eq!(pen(&canvas), white, "the background");
        canvas.apply(CanvasInput::End(press(150.0, 100.0)));
        assert_eq!(pen(&canvas), red);
        assert_eq!(ops(&canvas), 1, "nothing drawn");
        // Off the canvas, nothing changes.
        canvas.tools.color = white;
        canvas.apply(draw(press(400.0, 100.0)));
        canvas.apply(CanvasInput::End(press(400.0, 100.0)));
        assert_eq!(pen(&canvas), white);

        // Alt with the selection tool takes away instead.
        canvas.set_tool(Tool::Select).unwrap();
        canvas.apply(draw(press(150.0, 100.0)));
        assert!(matches!(canvas.interaction, Interaction::Selecting));
        canvas.apply(CanvasInput::Cancel);
        assert_eq!(pen(&canvas), white);

        // The eyedropper needs no Alt, and reads the frame shown during
        // playback, drawn smaller.
        canvas.set_modifiers(false, false);
        canvas.set_tool(Tool::Eyedropper).unwrap();
        canvas.apply(draw(press(150.0, 100.0)));
        canvas.apply(CanvasInput::End(press(150.0, 100.0)));
        assert_eq!(pen(&canvas), red);
        canvas.tools.color = white;
        let mut small = Pixmap::new(160, 100);
        small.data_as_u8_slice_mut()[(50 * 160 + 75) * 4..][..4].copy_from_slice(&[0, 0, 255, 255]);
        canvas.tab.held = Some((Arc::new(small), 2));
        canvas.apply(draw(press(151.0, 101.0)));
        canvas.apply(CanvasInput::End(press(151.0, 101.0)));
        assert_eq!(pen(&canvas), ugu_core::ops::Rgba8([0, 0, 255, 255]));
    }
}
