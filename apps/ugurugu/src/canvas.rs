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
use ugu_render::live::{LiveStroke, Spare};
use ugu_render::plan::{Reference, RenderPlan};
use ugu_render::raster::PixelRect;
use ugu_render::stroke::Pen;
use ugu_render::view::Placement;
use ugu_session::{
    Brushes, Drag, FillError, InputPoint, Reads, Session, StrokeRefused, Tool, Tools,
};
use ugu_win::clock::Ticks;
use ugu_win::pointer::{PointerKind, PointerSample};
use vello_cpu::Pixmap;

use crate::budget::Budget;
use crate::cache::{CacheWorker, Key, Preview, Rendered, Renders, Snapshot, Version};
use crate::i18n::tr;
use crate::input::{CanvasInput, Gesture};

mod clipboard;
mod cursor;
mod onion;
mod picking;
mod posing;
mod sizing;
mod symmetry;
mod text;
mod transform;
mod view;

pub use clipboard::CopiedPixels;
pub use cursor::BrushCursor;
pub use onion::{OnionKey, OnionSettings, tint as tint_onion};
pub use sizing::{CanvasSize, clamp_edge};
pub use symmetry::RulerGrip;
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
/// A wand or bucket click waiting for the worker to draw what it reads.
struct AreaClick {
    number: u64,
    /// What it was clicked on; anything else drops it.
    key: Key,
    point: [f64; 2],
    tools: Tools,
    combine: Combine,
    started: Instant,
}

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
        /// The symmetry ruler's copies of `live`, drawn after it.
        copies: Vec<LiveStroke>,
        rect: Option<PixelRect>,
        /// Dragged out with the shape tool, its points all replaced as it
        /// changes.
        shape: bool,
        /// The shape was dragged since `live` was made; it is made again
        /// once per frame, as only the last drag shows.
        moved: bool,
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
    /// Dragging the current layer's pose with the move tool from `start`
    /// (document pixels), where it had `base`.
    Posing {
        grip: Grip,
        start: [f64; 2],
        base: ugu_core::keys::Pose,
    },
    /// Placing the symmetry ruler with its tool from `start` (client
    /// physical pixels), where it was `base`.
    Ruling {
        grip: symmetry::RulerGrip,
        start: [f64; 2],
        base: ugu_session::Symmetry,
    },
    /// Dragging the frame of the canvas being sized from `start` (document
    /// pixels), where it was at `base`.
    Sizing {
        grip: Grip,
        start: [f64; 2],
        base: [i64; 4],
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
    /// The click whose reference the worker draws, and how many were made.
    area_click: Option<AreaClick>,
    clicks: u64,
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
    /// Counts the poses dragged out, so each is rendered.
    posed: u64,
    onion: onion::Onion,
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
            area_click: None,
            clicks: 0,
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
            posed: 0,
            onion: onion::Onion::default(),
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
        self.onion = onion::Onion::default();
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
    /// What the symmetry ruler's strokes are drawn with side by side.
    helpers: Vec<Stamp>,
    interaction: Interaction,
    /// Shift and Alt.
    modifiers: (bool, bool),
    /// What the last stroke drew into, for the next stroke.
    spare_coverage: Vec<Spare>,
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
    /// Whether the frames around are shown faintly.
    onion_skin: bool,
    onion_settings: OnionSettings,
    /// A key copied to paste on another frame or layer.
    copied_key: Option<ugu_core::keys::Key>,
    /// The canvas size being changed on the canvas, if it is.
    sizing: Option<sizing::Sizing>,
}

/// A cache worker handing its renders to `done`.
fn worker(done: &Arc<dyn Fn(Rendered) + Send + Sync>) -> std::io::Result<CacheWorker> {
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
            tab: Tab::new(
                document,
                true,
                worker(&cache_done).expect("cannot start the canvas cache thread"),
                0,
            ),
            others: Vec::new(),
            position: 0,
            cache_done,
            tools: Tools::default(),
            typesetter: text::Typesetter::default(),
            upload: Some([0, 0, u32::from(width), u32::from(height)]),
            uploaded: None,
            stamp: Stamp::default(),
            helpers: Vec::new(),
            interaction: Interaction::Idle,
            modifiers: (false, false),
            spare_coverage: Vec::new(),
            area: None,
            pixels_per_point: 1.0,
            hover_eraser: false,
            drawn_at: None,
            sample_count: 0,
            notice: None,
            animation_allowed: true,
            generations: 0,
            onion_skin: false,
            onion_settings: OnionSettings::DEFAULT,
            copied_key: None,
            sizing: None,
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
        self.sizing = None;
        self.spare_coverage.clear();
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
    /// `saved` when it is on disk as it is. `false`, and no tab, when the
    /// tab shown cannot be left (see `leave_tab`); an error when no thread
    /// could be started to draw it.
    pub fn open_tab(&mut self, document: Document, saved: bool) -> std::io::Result<bool> {
        let worker = worker(&self.cache_done)?;
        if !self.leave_tab() {
            return Ok(false);
        }
        self.generations += 1;
        let tab = Tab::new(document, saved, worker, self.generations);
        let left = std::mem::replace(&mut self.tab, tab);
        self.others.insert(self.position, left);
        self.position += 1;
        self.enter_tab();
        Ok(true)
    }

    /// Shows the tab at `index` in the order; whether it is shown, which it
    /// is not when the tab shown cannot be left (see `leave_tab`).
    #[must_use]
    pub fn show_tab(&mut self, index: usize) -> bool {
        if index == self.position {
            return true;
        }
        if index >= self.tab_count() || !self.leave_tab() {
            return false;
        }
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
        true
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
        self.cancel_gesture();
        self.sizing = None;
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
    /// playback stops. When the document cannot take what is pending, says
    /// so and returns `false`, and the tab stays shown as it was.
    fn leave_tab(&mut self) -> bool {
        // Before `edit`, which would cut off a gesture without ending it.
        self.cancel_sizing();
        if self.edit(Session::settle).is_err() {
            self.notice = Some(tr("restyle-pending").to_owned());
            return false;
        }
        self.interaction = Interaction::Idle;
        self.tab.playback = None;
        self.tab.cache.drop_frames();
        self.tab.resume_playback = false;
        // Its answer would come while the tab is hidden, and be dropped.
        self.tab.area_click = None;
        self.tab.left_area = self.area;
        true
    }

    /// Shows the tab just made current: its pixels are uploaded again and
    /// what was asked of the worker while it was hidden is asked again.
    fn enter_tab(&mut self) {
        self.interaction = Interaction::Idle;
        self.spare_coverage.clear();
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
        // A transform or text to place is an edit of its own, which the
        // canvas size would apply first.
        let session = &self.tab.session;
        if session.pending().is_some() || session.placed_text().is_some() {
            self.cancel_sizing();
        }
        self.record_colors();
        if self.tab.session.document().canvas != before {
            // A frame is of the canvas before.
            self.sizing = None;
            // The split is of the canvas before; what it shows stays until
            // the split of the new one arrives.
            self.tab.split = None;
            self.spare_coverage.clear();
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
            posed: if self.tab.session.posing().is_some() {
                self.tab.posed
            } else {
                0
            },
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
        // A pose being dragged out is shown; once keyed it is the revision.
        if self.tab.session.posing().is_some() {
            return Snapshot {
                document: Arc::new(self.tab.session.shown_document().into_owned()),
                layers: Arc::new(self.tab.session.layer_revisions().clone()),
            };
        }
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
        self.sync_onion();
        let key = self.key();
        if self
            .tab
            .area_click
            .as_ref()
            .is_some_and(|click| click.key != key)
        {
            self.tab.area_click = None;
        }
        if self.tab.playback.is_some() {
            return;
        }
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
                let [width, height] = self.tab.session.document().canvas;
                self.tab
                    .onion
                    .cover(&mut self.tab.display, [0, 0, width, height]);
                self.tab.split = Some((key, *split));
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
            Rendered::Onion { key, pixels } => {
                if key.version.document == self.tab.generation {
                    self.adopt_onion(key, pixels);
                }
            }
            Rendered::Reference { number, pixels } => {
                let key = self.key();
                let Some(click) = self
                    .tab
                    .area_click
                    .take_if(|click| click.number == number && click.key == key)
                else {
                    return;
                };
                let read = pixels.is_some();
                if read {
                    self.tab.reference = pixels;
                }
                self.fill_area(click, read);
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
        self.tab.cache.drop_frames();
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
        self.cancel_sizing();
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
        self.spare_coverage.clear();
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
        if self.tab.resume_playback
            && !matches!(self.interaction, Interaction::Drawing { .. })
            && self.tab.area_click.is_none()
        {
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

    /// Puts back together the part of the display that pixels `rect` of
    /// the split's layer reach where the frame shows it, with the stroke
    /// being drawn and the onion skin.
    fn recomposite(&mut self, rect: Option<PixelRect>) {
        let (Some(rect), Some((_, split))) = (rect, &self.tab.split) else {
            return;
        };
        let canvas = self.tab.session.document().canvas;
        let Some(rect) = split.shown(rect, canvas) else {
            return;
        };
        let drawn = drawn(&self.interaction);
        composite(split, &drawn, rect, &mut self.tab.display);
        self.tab.onion.cover(&mut self.tab.display, rect);
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
            CanvasInput::Begin(Gesture::Draw, _, sample) if self.sizing.is_some() => {
                self.sample_count += 1;
                self.begin_sizing_drag(sample.position);
            }
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
            CanvasInput::Begin(Gesture::Draw, _, sample) if self.tools.tool == Tool::Move => {
                self.sample_count += 1;
                self.begin_pose_drag(sample.position);
            }
            CanvasInput::Begin(Gesture::Draw, _, sample) if self.tools.tool == Tool::Symmetry => {
                self.sample_count += 1;
                self.begin_ruler_drag(sample.position);
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
            // The eraser end of a pen erases, as with any tool.
            CanvasInput::Begin(Gesture::Draw, kind, sample)
                if self.tools.tool == Tool::Shape
                    && !(kind == PointerKind::Pen && sample.inverted) =>
            {
                self.sample_count += 1;
                self.begin_shape(&sample);
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
                    Interaction::Drawing { shape: true, .. } => self.drag_shape(&sample),
                    Interaction::Drawing { .. } => self.extend_stroke(&sample),
                    Interaction::Selecting => {
                        let point = self.to_document(sample.position);
                        self.tab.session.extend_selection(point);
                    }
                    Interaction::Transforming { .. } => self.drag_transform(sample.position),
                    Interaction::Posing { .. } => self.drag_pose(sample.position),
                    Interaction::Ruling { .. } => self.drag_ruler(sample.position),
                    Interaction::Sizing { .. } => self.drag_sizing(sample.position),
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
                    Interaction::Drawing {
                        live,
                        copies,
                        rect,
                        shape,
                        ..
                    } => {
                        let before = self.key();
                        let had = layer_ops(self.tab.session.document(), before.layer).len();
                        let point = self.input_point(&sample);
                        let outcome = if shape {
                            let drag = self.shape_drag();
                            self.tab.session.end_shape(point.position, drag)
                        } else {
                            self.tab.session.end_stroke(point)
                        };
                        self.end_drawing(before, had, outcome, live, copies, rect);
                    }
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
                    posing @ Interaction::Posing { .. } => {
                        self.interaction = posing;
                        self.end_pose_drag(sample.position);
                    }
                    sizing @ Interaction::Sizing { .. } => {
                        self.interaction = sizing;
                        self.drag_sizing(sample.position);
                        self.interaction = Interaction::Idle;
                    }
                    ruling @ Interaction::Ruling { .. } => {
                        self.interaction = ruling;
                        self.end_ruler_drag(sample.position);
                    }
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
            Interaction::Posing { .. } => {
                self.tab.session.cancel_pose();
            }
            Interaction::Sizing { base, .. } => self.undrag_sizing(base),
            Interaction::Ruling { base, .. } => self.set_symmetry(base),
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

    /// Esc: ends the gesture under way without its result, else stops
    /// sizing the canvas, else deselects.
    pub fn escape(&mut self) {
        if self.cancel_gesture() {
            return;
        }
        if self.sizing.is_some() {
            self.cancel_sizing();
        } else {
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

    /// A click with the wand or the bucket at `point`. What it reads is put
    /// together from the split when it can be; else the worker draws it and
    /// the click ends when that comes, so the render thread does not wait.
    fn click_area(&mut self, point: [f64; 2]) {
        let started = Instant::now();
        self.notice = None;
        self.pause_playback();
        // The area is read from the document as the fill will see it.
        if let Err(error) = self.tab.session.settle() {
            return self.fill_notice(&FillError::Edit(error));
        }
        self.tab.clicks += 1;
        let click = AreaClick {
            number: self.tab.clicks,
            key: self.key(),
            point,
            tools: self.tools.clone(),
            combine: self.combine(),
            started,
        };
        self.tab.area_click = None;
        match self.read_reference(click.number) {
            Some(read) => self.fill_area(click, read),
            None => self.tab.area_click = Some(click),
        }
    }

    /// Ends `click` with the wand or the bucket, on what is in
    /// `self.tab.reference` when `read`.
    fn fill_area(&mut self, click: AreaClick, read: bool) {
        let reference = self
            .tab
            .reference
            .as_ref()
            .filter(|_| read)
            .map(|pixels| pixels.data_as_u8_slice().as_chunks::<4>().0);
        let before = self.key();
        if click.tools.tool == Tool::Wand {
            if let Err(error) =
                self.tab
                    .session
                    .wand(&click.tools, click.point, click.combine, reference)
            {
                self.fill_notice(&error);
            }
        } else {
            let filled = self
                .tab
                .session
                .bucket(&click.tools, click.point, reference);
            self.record_colors();
            self.after_fill(before, filled.map(committed));
        }
        tracing::debug!(
            ms = click.started.elapsed().as_secs_f64() * 1000.0,
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
    /// `self.tab.reference` when the split shows this frame and holds the
    /// layers; `false` when there is nothing to read, `None` when the worker
    /// draws them instead, for click `number`.
    fn read_reference(&mut self, number: u64) -> Option<bool> {
        let document = self.tab.session.document();
        let reference = match self.tools.fill.reads {
            Reads::Current => Reference::Layer(self.tab.session.current_layer()),
            Reads::Marked => Reference::Marked,
            Reads::Visible => Reference::Visible,
        };
        let frame = self.tab.session.frame();
        let plan = RenderPlan::reference(document, reference, frame);
        if plan.layers.is_empty() {
            return Some(false);
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
        self.tab.reference = Some(pixels);
        if from_split {
            return Some(true);
        }
        let snapshot = self.snapshot();
        self.tab
            .cache
            .request_reference(number, snapshot, plan, frame);
        None
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
                last_fill(document, after.layer).and_then(
                    |(coverage, clip, antialias, color, behind)| {
                        let changed = split.stamp_fill(coverage, clip, antialias, color, behind);
                        *key = after;
                        changed
                    },
                )
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
        self.start_drawing(begun, false);
    }

    /// Starts the shape the shape tool drags out.
    fn begin_shape(&mut self, sample: &PointerSample) {
        self.notice = None;
        self.pause_playback();
        let point = self.to_document(sample.position);
        let begun = self.tab.session.begin_shape(&self.tools, point);
        self.start_drawing(begun, true);
    }

    /// The modifier keys' say in how a shape is dragged.
    fn shape_drag(&self) -> Drag {
        Drag {
            constrain: self.modifiers.0,
            centred: self.modifiers.1,
        }
    }

    /// Shows the stroke the session has `begun`, or says why there is none.
    fn start_drawing(&mut self, begun: Result<(), StrokeRefused>, shape: bool) {
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
            Err(StrokeRefused::OutsideSelection) => {
                self.notice = Some(tr("draw-outside-selection").to_owned());
                return;
            }
        }
        let spare = self.spare_coverage.pop();
        let mut stroke = self.live_stroke(spare);
        let live = self.tab.session.live().expect("a stroke has begun");
        let mut rect = stroke.update(&live.points);
        let mut copies = Vec::with_capacity(live.copies.len());
        for (index, copy) in live.copies.iter().enumerate() {
            let spare = self.spare_coverage.pop();
            let mut shown = self.copy_stroke(index, spare);
            rect = union(rect, shown.update(&copy.points));
            copies.push(shown);
        }
        self.interaction = Interaction::Drawing {
            live: Box::new(stroke),
            copies,
            rect,
            shape,
            moved: false,
        };
        self.recomposite(rect);
    }

    /// The display of the session's stroke, with none of its points drawn.
    fn live_stroke(&self, spare: Option<Spare>) -> LiveStroke {
        let live = self.tab.session.live().expect("a stroke has begun");
        self.shown_stroke(&live.template, spare)
    }

    /// The display of the session stroke's copy `index`, with none of its
    /// points drawn.
    fn copy_stroke(&self, index: usize, spare: Option<Spare>) -> LiveStroke {
        let live = self.tab.session.live().expect("a stroke has begun");
        self.shown_stroke(&live.copies[index].template, spare)
    }

    /// The display of a stroke like `template` on the session stroke's
    /// layer, cut to its selection.
    fn shown_stroke(&self, template: &ugu_core::store::Stroke, spare: Option<Spare>) -> LiveStroke {
        let live = self.tab.session.live().expect("a stroke has begun");
        let document = self.tab.session.document();
        let pen = Pen::new(template, wobble_of(document, live.layer), document.frames);
        let size = document.canvas.map(|edge| edge as u16);
        LiveStroke::new(
            size,
            pen,
            self.key().frame,
            template.color.0,
            live.erase,
            spare,
        )
        .with_clip(live.clip.as_ref().map(|selection| selection.mask().clone()))
    }

    /// Drags the shape to `sample`; the next frame shows it.
    fn drag_shape(&mut self, sample: &PointerSample) {
        let point = self.to_document(sample.position);
        let drag = self.shape_drag();
        if self.tab.session.drag_shape(point, drag)
            && let Interaction::Drawing { moved, .. } = &mut self.interaction
        {
            *moved = true;
        }
    }

    /// Shows the dragged shape anew where it was and is.
    fn show_shape(&mut self) {
        let Interaction::Drawing {
            shape: true,
            moved: true,
            ..
        } = self.interaction
        else {
            return;
        };
        let Interaction::Drawing {
            live: old,
            rect: was,
            ..
        } = std::mem::replace(&mut self.interaction, Interaction::Idle)
        else {
            unreachable!("dragging a shape");
        };
        let mut stroke = self.live_stroke(Some(old.into_spare()));
        let live = self.tab.session.live().expect("dragging");
        let antialias = live.template.brush.antialias && !self.tab.session.pixel_mode();
        let filled = live
            .fill
            .as_ref()
            .and_then(|outline| stroke.fill(outline, antialias));
        let outlined = if live.outlined {
            stroke.update(&live.points)
        } else {
            None
        };
        let rect = union(filled, outlined);
        self.interaction = Interaction::Drawing {
            live: Box::new(stroke),
            copies: Vec::new(),
            rect,
            shape: true,
            moved: false,
        };
        self.recomposite(union(was, rect));
    }

    fn extend_stroke(&mut self, sample: &PointerSample) {
        let point = self.input_point(sample);
        if !self.tab.session.extend_stroke(point) {
            return;
        }
        let session = self.tab.session.live().expect("drawing");
        let Interaction::Drawing {
            live, copies, rect, ..
        } = &mut self.interaction
        else {
            return;
        };
        // The ruler's strokes change pixels far apart: each is put together
        // on its own, not the box around them all.
        let changed: Vec<_> = std::iter::once(live.update(&session.points))
            .chain(
                copies
                    .iter_mut()
                    .zip(&session.copies)
                    .map(|(shown, copy)| shown.update(&copy.points)),
            )
            .flatten()
            .collect();
        for &changed in &changed {
            *rect = union(*rect, Some(changed));
        }
        for changed in changed {
            self.recomposite(Some(changed));
        }
    }

    /// Shows how the stroke the session has ended, from state `before`,
    /// came out.
    /// Shows what a stroke or a shape committed: the operations after the
    /// layer's first `had`, a filled shape's fill under its stroke.
    fn end_drawing(
        &mut self,
        before: Key,
        had: usize,
        outcome: Result<Outcome, ugu_core::edit::EditError>,
        live: Box<LiveStroke>,
        copies: Vec<LiveStroke>,
        rect: Option<PixelRect>,
    ) {
        let started = Instant::now();
        self.record_colors();
        self.spare_coverage.push(live.into_spare());
        self.spare_coverage
            .extend(copies.into_iter().map(LiveStroke::into_spare));
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
                let wobble = wobble_of(document, after.layer);
                // The symmetry ruler's strokes are drawn side by side.
                let threads =
                    std::thread::available_parallelism().map_or(1, |count| count.get().min(8));
                let mut changed = None;
                let mut strokes = Vec::new();
                let ops = layer_ops(document, after.layer)
                    .get(had..)
                    .unwrap_or_default();
                for (index, op) in ops.iter().enumerate() {
                    if let Some(stroke) = op_stroke(document, op) {
                        strokes.push(stroke);
                    } else if let Some((coverage, clip, antialias, color, behind)) =
                        op_fill(document, op)
                    {
                        changed = union(
                            changed,
                            split.stamp_fill(coverage, clip, antialias, color, behind),
                        );
                    }
                    let next_is_stroke = ops
                        .get(index + 1)
                        .is_some_and(|next| op_stroke(document, next).is_some());
                    if !strokes.is_empty() && !next_is_stroke {
                        changed = union(
                            changed,
                            split.stamp_strokes(
                                &mut self.stamp,
                                &mut self.helpers,
                                &strokes,
                                wobble,
                                document.frames,
                                threads,
                            ),
                        );
                        strokes.clear();
                    }
                }
                *key = after;
                changed
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
        self.place_sizing(area);
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
        self.show_shape();
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
    let plan = RenderPlan::new(document, Purpose::Display, 0);
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
/// A paint layer's operations; none for any other layer.
fn layer_ops(document: &Document, layer: ugu_core::document::LayerId) -> &[Op] {
    match document.layer(layer).map(|layer| &layer.kind) {
        Some(LayerKind::Paint(paint)) => &paint.ops,
        _ => &[],
    }
}

/// The stroke `op` draws or erases with, and its clip.
fn op_stroke<'a>(
    document: &'a Document,
    op: &Op,
) -> Option<(
    &'a ugu_core::store::Stroke,
    bool,
    Option<&'a ugu_core::store::Mask>,
)> {
    let (id, erase, clip) = match op {
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

/// A fill's coverage, clip, antialiasing, colour and whether it goes behind.
type FillParts<'a> = (
    &'a ugu_core::store::Mask,
    Option<&'a ugu_core::store::Mask>,
    bool,
    [u8; 4],
    bool,
);

/// The fill on top of `layer`'s operations.
fn last_fill(document: &Document, layer: ugu_core::document::LayerId) -> Option<FillParts<'_>> {
    op_fill(document, layer_ops(document, layer).last()?)
}

/// What a fill `op` fills.
fn op_fill<'a>(document: &'a Document, op: &Op) -> Option<FillParts<'a>> {
    let Op::Fill {
        coverage,
        color,
        antialias,
        clip,
        behind,
    } = op
    else {
        return None;
    };
    let masks = &document.store.masks;
    let clip = match clip {
        Some(mask) => Some(masks.get(mask)?),
        None => None,
    };
    Some((masks.get(coverage)?, clip, *antialias, color.0, *behind))
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

/// The strokes being drawn, the hand-drawn one first, as they are committed.
fn drawn(interaction: &Interaction) -> Vec<&LiveStroke> {
    match interaction {
        Interaction::Drawing { live, copies, .. } => {
            std::iter::once(live.as_ref()).chain(copies).collect()
        }
        _ => Vec::new(),
    }
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
        canvas.open_tab(Document::new([32, 16]), true).unwrap();
        assert_eq!((canvas.tab_count(), canvas.tab_position()), (2, 1));
        assert_eq!(canvas.session().document().canvas, [32, 16]);
        canvas.edit(Session::add_layer).unwrap();
        let edited = canvas.generation();
        assert!(canvas.show_tab(0));
        assert_eq!(canvas.session().document().canvas, [64, 64]);
        assert!(!canvas.session().is_dirty());
        assert_ne!(canvas.generation(), edited);
        assert_eq!(canvas.dirty_tab(&[]), Some(1));
        assert_eq!(canvas.dirty_tab(&[edited]), None);
        assert_eq!(canvas.tab_generations()[1], edited);
        assert!(canvas.show_tab(1));
        assert!(canvas.session().is_dirty());
        assert_eq!(canvas.session().document().layers.len(), 2);
    }

    #[test]
    fn a_tab_whose_pending_transform_cannot_be_applied_stays_shown() {
        use ugu_core::ops::{Affine, Op, PaintLayer};
        let mut full = Document::new([8, 8]);
        if let LayerKind::Paint(PaintLayer { ops, .. }) = &mut full.layers[0].kind {
            *ops = vec![
                Op::Crop {
                    offset: [0, 0],
                    size: [8, 8]
                };
                ugu_core::document::limits::OPERATIONS
            ];
        }
        let mut canvas = Canvas::new(Document::new([64, 64]), |_| {});
        assert!(canvas.open_tab(full.clone(), true).unwrap());
        canvas.edit(Session::select_all).unwrap();
        canvas.edit(Session::begin_transform).unwrap();
        canvas.edit(|session| session.set_transform(Affine::translation(1.0, 0.0)));
        assert!(!canvas.show_tab(0));
        assert!(!canvas.open_tab(Document::new([4, 4]), true).unwrap());
        assert_eq!((canvas.tab_count(), canvas.tab_position()), (2, 1));
        assert_eq!(canvas.notice(), Some(tr("restyle-pending")));
        assert!(canvas.session().pending().is_some());
        assert_eq!(canvas.session().document(), &full);
        canvas.cancel_transform();
        assert!(canvas.show_tab(0));
    }

    #[test]
    fn tabs_of_the_same_name_are_numbered_in_the_order_opened() {
        let mut canvas = Canvas::new(Document::new([10, 10]), |_| {});
        canvas.open_tab(Document::new([20, 20]), true).unwrap();
        canvas.file_mut().unsaved_name = Some("Cat".to_owned());
        canvas.open_tab(Document::new([30, 30]), true).unwrap();
        canvas.open_tab(Document::new([40, 40]), true).unwrap();
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
        canvas.open_tab(Document::new([20, 20]), true).unwrap();
        canvas.open_tab(Document::new([30, 30]), true).unwrap();
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
        assert!(canvas.show_tab(2));
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
            // A click plays on once what it reads has come.
            canvas.tick(now);
            while canvas.tab.area_click.is_some() {
                assert!(!canvas.is_playing());
                let rendered = renders
                    .recv_timeout(Duration::from_secs(10))
                    .expect("the reference renders");
                canvas.adopt(rendered);
            }
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
    fn a_click_during_playback_reads_before_the_frames_waiting() {
        let (to_test, renders) = std::sync::mpsc::channel();
        let mut canvas = super::Canvas::new(Document::new([640, 400]), move |rendered| {
            let _ = to_test.send(rendered);
        });
        canvas.set_tool(Tool::Fill).unwrap();
        canvas.toggle_playback();
        canvas.tick(Instant::now());
        let revision = canvas.session().revision();
        canvas.click_area([100.0, 100.0]);
        let mut frames = 0;
        while canvas.session().revision() == revision {
            let rendered = renders
                .recv_timeout(Duration::from_secs(10))
                .expect("the fill comes in");
            frames += usize::from(matches!(rendered, Rendered::Frame { .. }));
            canvas.adopt(rendered);
        }
        while let Ok(rendered) = renders.recv_timeout(Duration::from_secs(1)) {
            frames += usize::from(matches!(rendered, Rendered::Frame { .. }));
        }
        // At most the one under way when it was clicked: the rest are no
        // longer shown.
        assert!(frames <= 1, "{frames} frames drawn after the click");
        assert!(!canvas.is_playing());
    }

    /// How long a bucket click right after playback stops and right after
    /// an undo holds the render thread, and how long until the fill is in,
    /// on the document at `UGU_FILL_FIXTURE` (`ugu-doc fixture 2`):
    /// `UGU_WORKER_THREADS=8 cargo test --release -p ugurugu fill_wait --
    /// --ignored --nocapture`.
    #[test]
    #[ignore = "measurement"]
    fn fill_wait_after_playback_and_undo() {
        let path = std::env::var("UGU_FILL_FIXTURE").expect("UGU_FILL_FIXTURE");
        let file = std::fs::File::open(path).unwrap();
        let (document, _) = ugu_io::read::read(std::io::BufReader::new(file)).unwrap();
        let centre = document.canvas.map(|edge| f64::from(edge) / 2.0);
        let (to_test, renders) = std::sync::mpsc::channel();
        let mut canvas = super::Canvas::new(document, move |rendered| {
            let _ = to_test.send(rendered);
        });
        canvas.set_tool(Tool::Fill).unwrap();
        let quiet = |canvas: &mut super::Canvas| {
            settle(canvas, &renders);
            while let Ok(rendered) = renders.recv_timeout(Duration::from_millis(500)) {
                canvas.adopt(rendered);
            }
        };
        // Blocked, then until the fill is in.
        let click = |canvas: &mut super::Canvas| {
            let revision = canvas.session().revision();
            let started = Instant::now();
            canvas.click_area(centre);
            let blocked = started.elapsed().as_secs_f64() * 1000.0;
            while canvas.session().revision() == revision && canvas.notice().is_none() {
                let rendered = renders
                    .recv_timeout(Duration::from_secs(60))
                    .expect("the fill comes in");
                canvas.adopt(rendered);
            }
            if let Some(notice) = canvas.notice() {
                println!("notice: {notice}");
            }
            (blocked, started.elapsed().as_secs_f64() * 1000.0)
        };
        let report = |what: &str, mut times: Vec<(f64, f64)>| {
            times.sort_by(|a, b| a.1.total_cmp(&b.1));
            let blocked: Vec<String> = times.iter().map(|t| format!("{:.0}", t.0)).collect();
            let done: Vec<String> = times.iter().map(|t| format!("{:.0}", t.1)).collect();
            println!(
                "{what}: blocked {} ms, filled {} ms",
                blocked.join("/"),
                done.join("/")
            );
        };
        // It reads the current layer, as it does unless set otherwise.
        let mut played = Vec::new();
        let mut undone = Vec::new();
        for _ in 0..5 {
            quiet(&mut canvas);
            canvas.toggle_playback();
            let mut now = Instant::now();
            let first = canvas.session().frame();
            while canvas.session().frame() < first + 3 {
                canvas.tick(now);
                now += Duration::from_millis(100);
                if let Ok(rendered) = renders.recv_timeout(Duration::from_millis(100)) {
                    canvas.adopt(rendered);
                }
            }
            played.push(click(&mut canvas));
            canvas.tab.resume_playback = false;
            canvas.edit(Session::undo).unwrap();

            quiet(&mut canvas);
            click(&mut canvas);
            quiet(&mut canvas);
            canvas.edit(Session::undo).unwrap();
            canvas.sync();
            undone.push(click(&mut canvas));
            canvas.edit(Session::undo).unwrap();
        }
        report("after playback", played);
        report("after undo", undone);
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
        // The worker draws what it reads, and the click ends then.
        assert_eq!(canvas.session().selection(), None);
        while canvas.tab.area_click.is_some() {
            let rendered = renders
                .recv_timeout(Duration::from_secs(10))
                .expect("the reference renders");
            canvas.adopt(rendered);
        }
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

    /// Fills the inside of a box drawn in black with the pen `width` wide,
    /// and returns over every frame the most pixels of the line the fill
    /// does not show behind, and the most outside the box it changed,
    /// against the same frame before the fill. Behind a black line over
    /// white paper, a line pixel either stays or becomes the fill colour
    /// darkened by as much as the line darkened the paper.
    fn bucket_in_a_box(width: f32, antialias: bool) -> (usize, usize) {
        let (to_test, renders) = std::sync::mpsc::channel();
        let mut canvas = super::Canvas::new(Document::new([320, 200]), move |rendered| {
            let _ = to_test.send(rendered);
        });
        let corners = [
            [60.0, 40.0],
            [260.0, 40.0],
            [260.0, 160.0],
            [60.0, 160.0],
            [60.0, 40.0],
        ];
        let mut tools = canvas.tools().clone();
        tools.plain.pen.width = width;
        tools.plain.pen.antialias = antialias;
        canvas.set_tools(tools.clone()).unwrap();
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
        let frames = canvas.session().document().frames as i64;
        let before: Vec<Pixmap> = (0..frames)
            .map(|frame| full_frame(&canvas, frame))
            .collect();
        canvas.set_tool(Tool::Fill).unwrap();
        canvas.tools.color = ugu_core::ops::Rgba8([30, 120, 200, 255]);
        canvas.click_area([160.0, 100.0]);
        assert_eq!(canvas.notice(), None);
        let (mut on_line, mut outside) = (0, 0);
        for frame in 0..frames {
            let after = full_frame(&canvas, frame);
            let (a, b) = (
                after.data_as_u8_slice(),
                before[frame as usize].data_as_u8_slice(),
            );
            let (mut line, mut out) = (0, 0);
            for (index, (a, b)) in a.chunks(4).zip(b.chunks(4)).enumerate() {
                let changed = a.iter().zip(b).any(|(a, b)| a.abs_diff(*b) > 8);
                if !changed {
                    continue;
                }
                let inked = b[..3].iter().any(|&channel| channel < 250);
                let [x, y] = [index % 320, index / 320];
                let beyond = !(50..=270).contains(&x) || !(30..=170).contains(&y);
                let behind =
                    [30u32, 120, 200]
                        .iter()
                        .zip(b)
                        .zip(a)
                        .all(|((&fill, &paper), &shown)| {
                            (fill * u32::from(paper) / 255).abs_diff(u32::from(shown)) <= 12
                        });
                if inked && !behind {
                    line += 1;
                } else if !inked && beyond {
                    out += 1;
                }
            }
            on_line = on_line.max(line);
            outside = outside.max(out);
        }
        (on_line, outside)
    }

    fn full_frame(canvas: &super::Canvas, frame: i64) -> Pixmap {
        let mut full = Pixmap::new(320, 200);
        ugu_render::document::DocumentRenderer::new(0).render(
            canvas.session().document(),
            frame,
            Purpose::Display,
            &mut full,
        );
        full
    }

    #[test]
    fn a_bucket_fill_goes_behind_the_lines_on_every_wobbling_frame() {
        // Before, the fill took the soft inner edge of a line and showed on
        // it, and past it where the line wobbled in.
        for (width, antialias) in [(6.0, false), (6.0, true), (4.0, true)] {
            assert_eq!(
                bucket_in_a_box(width, antialias),
                (0, 0),
                "a {width} px line, antialiased {antialias}"
            );
        }
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
        let plan = RenderPlan::reference(document, Reference::Layer(layer), frame);
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
        let (clip, pixels) = canvas.copy().expect("copied");
        // Drawn later, as on the clipboard thread, from the copied state.
        canvas.delete_selected();
        let image = pixels.image();
        canvas.edit(Session::undo).unwrap();
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

    #[test]
    fn the_ruler_tool_lays_a_line_where_dragged_and_moves_by_its_centre() {
        let (to_test, _renders) = std::sync::mpsc::channel();
        let mut canvas = super::Canvas::new(Document::new([120, 90]), move |rendered| {
            let _ = to_test.send(rendered);
        });
        canvas.set_area([0, 0, 120, 90]);
        canvas.set_tool(Tool::Symmetry).unwrap();
        let revision = canvas.session().revision();
        let at = |canvas: &super::Canvas, x: f64, y: f64| {
            let [x, y] = canvas.to_client([x, y]);
            press(x, y)
        };
        let drag = |canvas: &mut super::Canvas, from: [f64; 2], to: [f64; 2]| {
            let begin = at(canvas, from[0], from[1]);
            canvas.apply(CanvasInput::Begin(Gesture::Draw, PointerKind::Mouse, begin));
            let end = at(canvas, to[0], to[1]);
            canvas.apply(CanvasInput::Extend(end));
            canvas.apply(CanvasInput::End(end));
        };
        // A new line from where the drag starts, on a half pixel, towards
        // where it ends: to the right is 90° from upright.
        drag(&mut canvas, [30.2, 40.7], [70.0, 40.7]);
        let ruler = canvas.symmetry();
        assert!(ruler.on);
        assert_eq!(ruler.centre, [30.0, 40.5]);
        assert!((ruler.angle - 90.0).abs() < 0.1, "{}", ruler.angle);
        // From the centre it moves, keeping its angle.
        drag(&mut canvas, [30.0, 40.5], [50.0, 50.5]);
        let ruler = canvas.symmetry();
        assert_eq!(ruler.centre, [50.0, 50.5]);
        assert!((ruler.angle - 90.0).abs() < 0.1, "{}", ruler.angle);
        // With Shift the line snaps to 15° steps.
        canvas.set_modifiers(true, false);
        drag(&mut canvas, [80.0, 60.0], [90.0, 71.0]);
        canvas.set_modifiers(false, false);
        assert_eq!(canvas.symmetry().angle, 135.0);
        // Not past the canvas, where its lines would not show.
        drag(&mut canvas, [80.0, 60.0], [80.0, 60.0]);
        drag(&mut canvas, [80.0, 60.0], [200.0, 300.0]);
        assert_eq!(canvas.symmetry().centre, [120.0, 90.0]);
        // Esc during a drag leaves it where it was.
        let before = canvas.symmetry();
        let begin = at(&canvas, 20.0, 20.0);
        canvas.apply(CanvasInput::Begin(Gesture::Draw, PointerKind::Mouse, begin));
        canvas.apply(CanvasInput::Extend(at(&canvas, 60.0, 30.0)));
        canvas.escape();
        assert_eq!(canvas.symmetry(), before);
        // Placing the ruler draws nothing and is not an edit.
        assert_eq!(canvas.session().revision(), revision);
    }

    #[test]
    fn a_dragged_shape_shows_as_it_goes_and_ends_as_the_export_draws_it() {
        let (to_test, renders) = std::sync::mpsc::channel();
        let mut canvas = super::Canvas::new(Document::new([120, 90]), move |rendered| {
            let _ = to_test.send(rendered);
        });
        canvas.set_area([0, 0, 120, 90]);
        settle(&mut canvas, &renders);
        let blank = canvas.display().clone();
        canvas.set_tool(Tool::Shape).unwrap();
        canvas.tools_mut().shape.shape = ugu_session::Shape::Ellipse;
        let at = |canvas: &super::Canvas, x: f64, y: f64| {
            let [x, y] = canvas.to_client([x, y]);
            press(x, y)
        };
        let begin = at(&canvas, 10.0, 10.0);
        canvas.apply(CanvasInput::Begin(Gesture::Draw, PointerKind::Mouse, begin));
        let begun = canvas.display().clone();
        // Drags between two frames are drawn once, at the frame (issue #51).
        for x in [60.0, 90.0, 110.0] {
            let sample = at(&canvas, x, 80.0);
            canvas.apply(CanvasInput::Extend(sample));
        }
        assert!(canvas.display().data_as_u8_slice() == begun.data_as_u8_slice());
        canvas.take_upload();
        assert!(canvas.display().data_as_u8_slice() != begun.data_as_u8_slice());
        // Shrunk, it leaves nothing where it was.
        let near = at(&canvas, 40.0, 30.0);
        canvas.apply(CanvasInput::Extend(near));
        canvas.take_upload();
        let shown = canvas.display();
        for y in 50..90u16 {
            for x in 60..120u16 {
                let index = (usize::from(y) * 120 + usize::from(x)) * 4;
                assert_eq!(
                    shown.data_as_u8_slice()[index..index + 4],
                    blank.data_as_u8_slice()[index..index + 4],
                    "{x}, {y}"
                );
            }
        }
        canvas.set_modifiers(true, false);
        canvas.apply(CanvasInput::End(near));
        canvas.set_modifiers(false, false);
        let layer = canvas.session().current_layer();
        let LayerKind::Paint(paint) = &canvas.session().document().layer(layer).unwrap().kind
        else {
            panic!("a paint layer");
        };
        assert_eq!(paint.ops.len(), 1);
        assert_eq!(canvas.session().undo_label(), Some("Draw"));
        let mut full = Pixmap::new(120, 90);
        ugu_render::document::DocumentRenderer::new(0).render(
            canvas.session().document(),
            canvas.session().frame(),
            ugu_render::document::Purpose::Display,
            &mut full,
        );
        assert!(canvas.display().data_as_u8_slice() == full.data_as_u8_slice());
    }

    #[test]
    fn a_filled_shape_shows_its_fill_as_it_goes_and_ends_as_the_export_draws_it() {
        let (to_test, renders) = std::sync::mpsc::channel();
        let mut canvas = super::Canvas::new(Document::new([120, 90]), move |rendered| {
            let _ = to_test.send(rendered);
        });
        canvas.set_area([0, 0, 120, 90]);
        settle(&mut canvas, &renders);
        let blank = canvas.display().clone();
        canvas.set_tool(Tool::Shape).unwrap();
        for paint in [ugu_session::ShapePaint::Fill, ugu_session::ShapePaint::Both] {
            let shape = &mut canvas.tools_mut().shape;
            shape.shape = ugu_session::Shape::Moon;
            shape.paint = paint;
            let at = |canvas: &super::Canvas, x: f64, y: f64| {
                let [x, y] = canvas.to_client([x, y]);
                press(x, y)
            };
            let begin = at(&canvas, 10.0, 10.0);
            canvas.apply(CanvasInput::Begin(Gesture::Draw, PointerKind::Mouse, begin));
            let end = at(&canvas, 90.0, 80.0);
            canvas.apply(CanvasInput::Extend(end));
            canvas.take_upload();
            let pixel = |canvas: &super::Canvas, x: usize, y: usize| {
                canvas.display().data_as_u8_slice()[(y * 120 + x) * 4..][..4].to_vec()
            };
            // Inside the thick part of the moon, away from its outline.
            let inside = (24, 45);
            assert_ne!(
                pixel(&canvas, inside.0, inside.1),
                pixel_of(&blank, inside),
                "{paint:?}"
            );
            canvas.apply(CanvasInput::End(end));
            let mut full = Pixmap::new(120, 90);
            ugu_render::document::DocumentRenderer::new(0).render(
                canvas.session().document(),
                canvas.session().frame(),
                ugu_render::document::Purpose::Display,
                &mut full,
            );
            assert!(
                canvas.display().data_as_u8_slice() == full.data_as_u8_slice(),
                "{paint:?}"
            );
            canvas.edit(Session::undo).unwrap();
            settle(&mut canvas, &renders);
        }
    }

    fn pixel_of(pixmap: &Pixmap, (x, y): (usize, usize)) -> Vec<u8> {
        pixmap.data_as_u8_slice()[(y * usize::from(pixmap.width()) + x) * 4..][..4].to_vec()
    }

    /// Issue #51: the time a frame of a large wobbly shape drag takes to
    /// show, with one pointer sample a frame or 16 (a 1000 Hz mouse at
    /// 60 Hz), beside the same drag drawn freehand.
    #[test]
    #[ignore = "measurement; run with --release --ignored --nocapture"]
    fn shape_drag_frame_cost() {
        let (to_test, renders) = std::sync::mpsc::channel();
        let mut canvas = super::Canvas::new(Document::new([1920, 1080]), move |rendered| {
            let _ = to_test.send(rendered);
        });
        canvas.set_area([0, 0, 1920, 1080]);
        settle(&mut canvas, &renders);
        canvas.brushes_mut().pen.width = 40.0;
        let at = |canvas: &super::Canvas, x: f64, y: f64| {
            let [x, y] = canvas.to_client([x, y]);
            press(x, y)
        };
        for tool in [Tool::Shape, Tool::Pen] {
            for per_frame in [1u32, 16] {
                canvas.set_tool(tool).unwrap();
                canvas.tools_mut().shape.shape = ugu_session::Shape::Ellipse;
                let begin = at(&canvas, 100.0, 100.0);
                canvas.apply(CanvasInput::Begin(Gesture::Draw, PointerKind::Mouse, begin));
                let frames = 120;
                let mut times = Vec::new();
                for frame in 0..frames {
                    let started = std::time::Instant::now();
                    for sample in 1..=per_frame {
                        let t =
                            f64::from(frame * per_frame + sample) / f64::from(frames * per_frame);
                        let sample = at(&canvas, 100.0 + 1700.0 * t, 100.0 + 850.0 * t);
                        canvas.apply(CanvasInput::Extend(sample));
                    }
                    canvas.take_upload();
                    times.push(started.elapsed().as_secs_f64() * 1000.0);
                }
                canvas.apply(CanvasInput::End(at(&canvas, 1800.0, 950.0)));
                let total: f64 = times.iter().sum();
                times.sort_by(f64::total_cmp);
                println!(
                    "{tool:?} {per_frame}/frame: {frames} frames, total {total:.0} ms, \
                     median {:.2} ms, max {:.2} ms",
                    times[times.len() / 2],
                    times[times.len() - 1]
                );
                canvas.edit(|session| session.undo()).unwrap();
            }
        }
    }

    #[test]
    fn the_symmetry_rulers_strokes_show_as_they_go_and_end_as_the_export_draws_them() {
        let (to_test, renders) = std::sync::mpsc::channel();
        let mut canvas = super::Canvas::new(Document::new([120, 90]), move |rendered| {
            let _ = to_test.send(rendered);
        });
        canvas.set_area([0, 0, 120, 90]);
        settle(&mut canvas, &renders);
        let blank = canvas.display().clone();
        let symmetry = canvas.symmetry();
        canvas.set_symmetry(ugu_session::Symmetry {
            on: true,
            lines: 2,
            ..symmetry
        });
        canvas.brushes_mut().pen.width = 6.0;
        canvas.brushes_mut().pen.opacity = 0.5;
        let at = |canvas: &super::Canvas, x: f64, y: f64| {
            let [x, y] = canvas.to_client([x, y]);
            press(x, y)
        };
        let begin = at(&canvas, 20.0, 20.0);
        canvas.apply(CanvasInput::Begin(Gesture::Draw, PointerKind::Mouse, begin));
        for x in [30.0, 40.0, 50.0] {
            let sample = at(&canvas, x, 25.0);
            canvas.apply(CanvasInput::Extend(sample));
        }
        let pixel = |shown: &Pixmap, x: usize, y: usize| {
            shown.data_as_u8_slice()[(y * 120 + x) * 4..][..4].to_vec()
        };
        // Two lines mirror and turn the stroke into all four corners.
        for [x, y] in [[35, 24], [84, 24], [35, 65], [84, 65]] {
            assert_ne!(
                pixel(canvas.display(), x, y),
                pixel(&blank, x, y),
                "{x}, {y}"
            );
        }
        let end = at(&canvas, 50.0, 25.0);
        canvas.apply(CanvasInput::End(end));
        let layer = canvas.session().current_layer();
        let LayerKind::Paint(paint) = &canvas.session().document().layer(layer).unwrap().kind
        else {
            panic!("a paint layer");
        };
        assert_eq!(paint.ops.len(), 4);
        let mut full = Pixmap::new(120, 90);
        ugu_render::document::DocumentRenderer::new(0).render(
            canvas.session().document(),
            canvas.session().frame(),
            ugu_render::document::Purpose::Display,
            &mut full,
        );
        assert!(canvas.display().data_as_u8_slice() == full.data_as_u8_slice());
        canvas.edit(|session| session.undo()).unwrap();
        settle(&mut canvas, &renders);
        assert!(canvas.display().data_as_u8_slice() == blank.data_as_u8_slice());
    }

    /// What a stroke costs a frame with the symmetry ruler off and with
    /// 2, 12 and 24 strokes, at one sample a frame and at sixteen (a
    /// 1000 Hz mouse at 60 Hz), and the pen-up after it.
    #[test]
    #[ignore = "measurement; run with --release --ignored --nocapture"]
    fn symmetry_stroke_frame_cost() {
        let (to_test, renders) = std::sync::mpsc::channel();
        let mut canvas = super::Canvas::new(Document::new([2048, 2048]), move |rendered| {
            let _ = to_test.send(rendered);
        });
        canvas.set_area([0, 0, 2048, 2048]);
        settle(&mut canvas, &renders);
        canvas.brushes_mut().pen.width = 12.0;
        let at = |canvas: &super::Canvas, t: f64| {
            // A loop around the middle, as a mandala is drawn.
            let angle = t * std::f64::consts::TAU;
            let radius = 300.0 + 500.0 * t;
            let [x, y] =
                canvas.to_client([1024.0 + radius * angle.cos(), 1024.0 + radius * angle.sin()]);
            press(x, y)
        };
        for (lines, mirror) in [(1, false), (1, true), (6, true), (12, true)] {
            let symmetry = canvas.symmetry();
            canvas.set_symmetry(ugu_session::Symmetry {
                on: true,
                lines,
                mirror,
                ..symmetry
            });
            let strokes = canvas.symmetry().copies().len() + 1;
            for per_frame in [1u32, 16] {
                settle(&mut canvas, &renders);
                let begin = at(&canvas, 0.0);
                canvas.apply(CanvasInput::Begin(Gesture::Draw, PointerKind::Mouse, begin));
                let frames = 120;
                let mut times = Vec::new();
                for frame in 0..frames {
                    let started = std::time::Instant::now();
                    for sample in 1..=per_frame {
                        let t =
                            f64::from(frame * per_frame + sample) / f64::from(frames * per_frame);
                        let sample = at(&canvas, t);
                        canvas.apply(CanvasInput::Extend(sample));
                    }
                    canvas.take_upload();
                    times.push(started.elapsed().as_secs_f64() * 1000.0);
                }
                let started = std::time::Instant::now();
                canvas.apply(CanvasInput::End(at(&canvas, 1.0)));
                canvas.take_upload();
                let up = started.elapsed().as_secs_f64() * 1000.0;
                let total: f64 = times.iter().sum();
                times.sort_by(f64::total_cmp);
                println!(
                    "{strokes} strokes, {per_frame}/frame: total {total:.0} ms, \
                     median {:.2} ms, max {:.2} ms, pen-up {up:.2} ms",
                    times[times.len() / 2],
                    times[times.len() - 1]
                );
                canvas.edit(|session| session.undo()).unwrap();
            }
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

    #[test]
    fn a_selection_being_dragged_is_dropped_when_its_tab_is_left() {
        let (mut canvas, _renders) = animated_canvas();
        canvas.set_tool(Tool::Select).unwrap();
        canvas.apply(CanvasInput::Begin(
            Gesture::Draw,
            PointerKind::Mouse,
            press(10.0, 10.0),
        ));
        canvas.apply(CanvasInput::Extend(press(60.0, 40.0)));
        assert!(canvas.session().lasso().is_some());
        assert!(canvas.open_tab(Document::new([32, 16]), true).unwrap());
        assert!(canvas.show_tab(0));
        assert!(canvas.session().lasso().is_none());
        assert!(matches!(canvas.interaction, Interaction::Idle));
    }

    fn animated_canvas() -> (super::Canvas, std::sync::mpsc::Receiver<Rendered>) {
        let (to_test, renders) = std::sync::mpsc::channel();
        let mut canvas = super::Canvas::new(Document::new([120, 90]), move |rendered| {
            let _ = to_test.send(rendered);
        });
        canvas.set_area([0, 0, 120, 90]);
        settle(&mut canvas, &renders);
        (canvas, renders)
    }

    #[test]
    fn copies_of_a_layer_moved_to_other_places_merge_as_they_look() {
        let (mut canvas, renders) = animated_canvas();
        stroke_through(&mut canvas, &[[10.0, 20.0], [40.0, 40.0], [20.0, 60.0]]);
        for x in [30.0, 60.0] {
            canvas
                .edit(|session| session.duplicate_layer(|name| format!("{name} copy")))
                .unwrap();
            canvas.set_pose(ugu_core::keys::Pose {
                offset: [x, 5.0],
                ..ugu_core::keys::Pose::REST
            });
        }
        settle(&mut canvas, &renders);
        let before = full_render(&canvas);
        for _ in 0..2 {
            canvas.edit(Session::merge_down).unwrap();
        }
        assert_eq!(canvas.session().document().layers.len(), 1);
        settle(&mut canvas, &renders);
        let after = full_render(&canvas);
        assert!(before.data_as_u8_slice() == after.data_as_u8_slice());
        assert!(canvas.display().data_as_u8_slice() == after.data_as_u8_slice());
    }

    #[test]
    fn layers_faded_in_one_place_by_keys_merge_as_they_look() {
        let (mut canvas, renders) = animated_canvas();
        stroke_through(&mut canvas, &[[10.0, 20.0], [40.0, 40.0], [20.0, 60.0]]);
        canvas.set_pose(ugu_core::keys::Pose {
            opacity: 0.5,
            ..ugu_core::keys::Pose::REST
        });
        canvas
            .edit(|session| session.duplicate_layer(|name| format!("{name} copy")))
            .unwrap();
        canvas.set_pose(ugu_core::keys::Pose {
            offset: [30.0, 0.0],
            opacity: 0.7,
            ..ugu_core::keys::Pose::REST
        });
        settle(&mut canvas, &renders);
        let before = full_render(&canvas);
        canvas.edit(Session::merge_down).unwrap();
        settle(&mut canvas, &renders);
        assert_eq!(canvas.session().document().layers.len(), 1);
        assert!(before.data_as_u8_slice() == full_render(&canvas).data_as_u8_slice());
    }

    fn full_render(canvas: &super::Canvas) -> Pixmap {
        let document = canvas.session().document();
        let [width, height] = document.canvas.map(|edge| edge as u16);
        let mut full = Pixmap::new(width, height);
        ugu_render::document::DocumentRenderer::new(0).render(
            document,
            canvas.session().frame(),
            ugu_render::document::Purpose::Display,
            &mut full,
        );
        full
    }

    fn stroke_through(canvas: &mut super::Canvas, points: &[[f64; 2]]) {
        let at = |canvas: &super::Canvas, [x, y]: [f64; 2]| {
            let [x, y] = canvas.to_client([x, y]);
            press(x, y)
        };
        let first = at(canvas, points[0]);
        canvas.apply(CanvasInput::Begin(Gesture::Draw, PointerKind::Mouse, first));
        for point in &points[1..] {
            let sample = at(canvas, *point);
            canvas.apply(CanvasInput::Extend(sample));
        }
        let last = at(canvas, *points.last().unwrap());
        canvas.apply(CanvasInput::End(last));
    }

    #[test]
    fn a_stroke_on_a_moved_layer_shows_where_the_full_render_draws_it() {
        let (mut canvas, renders) = animated_canvas();
        canvas.set_pose(ugu_core::keys::Pose {
            offset: [13.5, 7.25],
            rotation: 20.0,
            scale: 0.8,
            flip: false,
            opacity: 1.0,
        });
        settle(&mut canvas, &renders);
        assert!(canvas.tab.split.as_ref().unwrap().1.moved.is_some());
        stroke_through(&mut canvas, &[[20.0, 20.0], [60.0, 45.0], [100.0, 30.0]]);
        let full = full_render(&canvas);
        let most = max_difference(canvas.display(), &full);
        assert!(most <= 2, "differs by {most}");
        // The stroke went into the layer's own pixels: on screen it starts
        // where the pen did.
        let stroke = canvas
            .session()
            .document()
            .store
            .strokes
            .values()
            .next()
            .unwrap();
        let first = stroke.points[0];
        let shown = canvas
            .session()
            .layer_transform()
            .unwrap()
            .apply([f64::from(first.x), f64::from(first.y)]);
        assert!((shown[0] - 20.0).abs() < 0.01 && (shown[1] - 20.0).abs() < 0.01);
    }

    #[test]
    fn the_move_tool_shows_a_pose_as_it_is_dragged_and_keys_it_on_release() {
        let (mut canvas, renders) = animated_canvas();
        stroke_through(&mut canvas, &[[30.0, 40.0], [60.0, 40.0]]);
        settle(&mut canvas, &renders);
        canvas.set_tool(Tool::Move).unwrap();
        let revision = canvas.session().revision();
        let at = |canvas: &super::Canvas, x: f64, y: f64| {
            let [x, y] = canvas.to_client([x, y]);
            press(x, y)
        };
        let begin = at(&canvas, 60.0, 45.0);
        canvas.apply(CanvasInput::Begin(Gesture::Draw, PointerKind::Mouse, begin));
        let moved = at(&canvas, 70.0, 50.0);
        canvas.apply(CanvasInput::Extend(moved));
        assert_eq!(canvas.session().revision(), revision);
        settle(&mut canvas, &renders);
        let mut shifted = Document::new([120, 90]);
        shifted.layers = canvas.session().shown_document().layers.clone();
        shifted.store = canvas.session().document().store.clone();
        let mut expected = Pixmap::new(120, 90);
        ugu_render::document::DocumentRenderer::new(0).render(
            &shifted,
            0,
            ugu_render::document::Purpose::Display,
            &mut expected,
        );
        assert!(canvas.display().data_as_u8_slice() == expected.data_as_u8_slice());
        canvas.apply(CanvasInput::End(moved));
        let layer = canvas.session().current_layer();
        let key = canvas.session().key_here(layer).unwrap();
        let near =
            |offset: [f64; 2]| (offset[0] - 10.0).abs() < 1e-6 && (offset[1] - 5.0).abs() < 1e-6;
        assert!(near(key.pose.offset), "{:?}", key.pose.offset);
        assert_eq!(
            canvas.session().undo_label(),
            Some("Move layer on this frame")
        );
        // Esc during a drag puts the layer back.
        let begin = at(&canvas, 70.0, 50.0);
        canvas.apply(CanvasInput::Begin(Gesture::Draw, PointerKind::Mouse, begin));
        let away = at(&canvas, 90.0, 50.0);
        canvas.apply(CanvasInput::Extend(away));
        canvas.escape();
        assert!(canvas.session().posing().is_none());
        assert!(near(canvas.session().key_here(layer).unwrap().pose.offset));
    }

    #[test]
    fn the_first_key_turns_about_the_middle_of_the_drawing_and_the_pivot_moves() {
        let (mut canvas, renders) = animated_canvas();
        stroke_through(&mut canvas, &[[20.0, 20.0], [40.0, 30.0]]);
        settle(&mut canvas, &renders);
        let centre = canvas.drawing_centre().expect("drawn");
        assert!(
            (centre[0] - 30.0).abs() < 8.0 && (centre[1] - 25.0).abs() < 8.0,
            "{centre:?}"
        );
        assert_eq!(canvas.pivot(), None);
        canvas.toggle_key();
        assert_eq!(canvas.pivot(), Some(centre));
        canvas.set_pivot([100.0, 10.0]);
        assert_eq!(canvas.pivot(), Some([100.0, 10.0]));
        assert_eq!(canvas.session().undo_label(), Some("Move pivot"));
    }

    #[test]
    fn an_onion_skin_shows_the_cels_around_on_screen_only() {
        let (mut canvas, renders) = animated_canvas();
        stroke_through(&mut canvas, &[[20.0, 20.0], [100.0, 70.0]]);
        canvas
            .edit(ugu_session::Session::make_animation_folder)
            .unwrap();
        canvas.step_frame(1);
        canvas.edit(ugu_session::Session::new_cel).unwrap();
        settle(&mut canvas, &renders);
        let full = full_render(&canvas);
        assert!(canvas.display().data_as_u8_slice() == full.data_as_u8_slice());
        canvas.set_onion_skin(true);
        let started = std::time::Instant::now();
        while canvas.display().data_as_u8_slice() == full.data_as_u8_slice() {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the onion skin shows"
            );
            if let Ok(rendered) = renders.recv_timeout(Duration::from_millis(100)) {
                canvas.adopt(rendered);
            }
            canvas.sync();
        }
        // Frame 0's stroke shows faintly in red, before this frame.
        let pixel = |pixmap: &Pixmap| {
            let index = (60 * 120 + 85) * 4;
            pixmap.data_as_u8_slice()[index..index + 4].to_vec()
        };
        let tinted = pixel(canvas.display());
        assert!(tinted[0] > tinted[2], "{tinted:?}");
        // What is exported and read is the frame alone.
        assert!(full_render(&canvas).data_as_u8_slice() == full.data_as_u8_slice());
        canvas.set_onion_skin(false);
        assert!(canvas.display().data_as_u8_slice() == full.data_as_u8_slice());
    }
}
