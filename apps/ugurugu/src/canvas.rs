// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The document on screen: drawing, panning and zooming.
//!
//! The image shown is the cached split put together. A stroke being drawn is
//! composited over it within the pixels it changes, and at pen-up the
//! committed stroke is added to the cached layer instead of rendering the
//! frame again. Anything else that changes the document asks the cache
//! worker for a new split and keeps showing the old one until it arrives.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ugu_core::document::{Document, LayerId, LayerKind};
use ugu_core::edit::Outcome;
use ugu_core::motion::frame_in_cycle;
use ugu_core::ops::Op;
use ugu_core::selection::Combine;
use ugu_render::compose::{Split, Stamp, composite};
use ugu_render::document::{FULL_DETAIL, Purpose, TILE_EDGE, scaled_size, surface_estimate};
use ugu_render::live::LiveStroke;
use ugu_render::plan::{Reference, RenderPlan};
use ugu_render::raster::PixelRect;
use ugu_render::stroke::Pen;
use ugu_render::view::Placement;
use ugu_session::{FillError, InputPoint, Reads, Session, StrokeRefused, Tool};
use ugu_win::clock::Ticks;
use ugu_win::pointer::{PointerKind, PointerSample};
use vello_cpu::Pixmap;

use crate::budget::Budget;
use crate::cache::{CacheWorker, Key, Preview, Rendered, Renders, Snapshot, Version};
use crate::i18n::tr;
use crate::input::{CanvasInput, Gesture};

mod clipboard;
mod text;
mod transform;

pub use transform::{Grip, HANDLES};

/// Shown around the document, opaque straight RGBA.
pub const WORKSPACE: [u8; 4] = [0x2A, 0x2C, 0x30, 255];
const ZOOM_STEP: f64 = 1.25;
pub const ZOOM_RANGE: std::ops::RangeInclusive<f64> = 0.05..=32.0;

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
}

pub struct Canvas {
    session: Session,
    cache: CacheWorker,
    split: Option<(Key, Split)>,
    /// A pending transform shown on the split's layer.
    preview: Option<transform::Preview>,
    /// Placed text shown on the split's layer.
    text_preview: Option<text::TextPreview>,
    typesetter: text::Typesetter,
    requested: Option<Key>,
    /// The edited image; empty from playback until the next split.
    display: Pixmap,
    /// The frame playback stopped on and how much smaller it is, shown
    /// until the split of that frame arrives.
    held: Option<(Arc<Pixmap>, u32)>,
    /// Display pixels not yet uploaded.
    upload: Option<PixelRect>,
    stamp: Stamp,
    interaction: Interaction,
    /// Shift and Alt.
    modifiers: (bool, bool),
    /// The last stroke's coverage, cleared, for the next stroke.
    spare_coverage: Option<Pixmap>,
    /// What the wand and the bucket last read, kept for the next click.
    reference: Option<Pixmap>,
    /// Canvas area in client physical pixels: left, top, right, bottom.
    area: Option<[i32; 4]>,
    /// Physical pixels per document pixel.
    scale: f64,
    pixels_per_point: f32,
    /// The document's top-left corner from the area's, in physical pixels.
    offset: [f64; 2],
    placed: bool,
    sample_count: usize,
    notice: Option<String>,
    playback: Option<Playback>,
    /// Counts documents opened, so renders of an earlier one are told apart.
    generation: u64,
    /// The document as last handed to the worker, shared by its jobs.
    snapshot: Option<(u64, Snapshot)>,
    /// Small images of paint layers' own pixels, with the revision each
    /// shows.
    thumbnails: HashMap<LayerId, (u64, Arc<Pixmap>)>,
    /// Thumbnails asked for and not yet back.
    thumbnails_asked: bool,
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
    pub fn new(document: Document, cache_done: impl Fn(Rendered) + Send + 'static) -> Self {
        let [width, height] = document.canvas.map(|edge| edge as u16);
        Self {
            session: Session::new(document, false),
            cache: CacheWorker::start(cache_done),
            split: None,
            preview: None,
            text_preview: None,
            typesetter: text::Typesetter::default(),
            requested: None,
            display: Pixmap::new(width, height),
            held: None,
            upload: Some([0, 0, u32::from(width), u32::from(height)]),
            stamp: Stamp::default(),
            interaction: Interaction::Idle,
            modifiers: (false, false),
            spare_coverage: None,
            reference: None,
            area: None,
            scale: 1.0,
            pixels_per_point: 1.0,
            offset: [0.0, 0.0],
            placed: false,
            sample_count: 0,
            notice: None,
            playback: None,
            snapshot: None,
            generation: 0,
            thumbnails: HashMap::new(),
            thumbnails_asked: false,
        }
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    /// Puts `document` in place of the open one, with an empty history;
    /// `saved` when it is on disk as it is.
    pub fn replace(&mut self, document: Document, saved: bool) {
        let [width, height] = document.canvas.map(|edge| edge as u16);
        self.session = Session::new(document, saved);
        self.generation += 1;
        self.split = None;
        self.preview = None;
        self.text_preview = None;
        self.requested = None;
        self.display = Pixmap::new(width, height);
        self.held = None;
        self.interaction = Interaction::Idle;
        self.spare_coverage = None;
        self.playback = None;
        self.upload_all();
        self.snapshot = None;
        self.thumbnails.clear();
        self.thumbnails_asked = false;
        self.placed = false;
        if let Some(area) = self.area {
            self.place(area);
        }
    }

    /// The document as it is now, shared with work off this thread.
    pub fn snapshot_now(&mut self) -> Arc<Document> {
        self.snapshot().document
    }

    pub fn mark_saved(&mut self, state: ugu_core::history::StateId) {
        self.session.mark_saved(state);
    }

    /// Changes the session outside drawing. A stroke being drawn is dropped
    /// first, as the change may move or remove what it is drawn on.
    pub fn edit<R>(&mut self, change: impl FnOnce(&mut Session) -> R) -> R {
        if let Interaction::Drawing { rect, .. } =
            std::mem::replace(&mut self.interaction, Interaction::Idle)
        {
            self.session.cancel_stroke();
            self.recomposite(rect);
        }
        let before = self.session.document().canvas;
        let key = self.key();
        let result = change(&mut self.session);
        if self.session.document().canvas != before {
            // The split is of the canvas before; what it shows stays until
            // the split of the new one arrives.
            self.split = None;
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
        let notches = (scale / self.scale).ln() / ZOOM_STEP.ln();
        self.zoom_in_place(notches as f32);
    }

    /// The document point under `position`, in client physical pixels.
    pub fn document_point(&self, position: [f64; 2]) -> [f64; 2] {
        self.to_document(position)
    }

    /// Shows the whole document, at most at 100%.
    pub fn fit(&mut self) {
        if let Some(area) = self.area {
            self.place(area);
        }
    }

    pub fn sample_count(&self) -> usize {
        self.sample_count
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    fn version(&self) -> Version {
        Version {
            document: self.generation,
            revision: self.session.revision(),
        }
    }

    pub fn scale(&self) -> f64 {
        self.scale
    }

    fn key(&self) -> Key {
        let document = self.session.document();
        Key {
            version: self.version(),
            layer: self.session.current_layer(),
            frame: frame_in_cycle(self.session.frame(), document.frames),
        }
    }

    pub fn contains(&self, position: [f64; 2]) -> bool {
        self.area.is_some_and(|[left, top, right, bottom]| {
            (f64::from(left)..f64::from(right)).contains(&position[0])
                && (f64::from(top)..f64::from(bottom)).contains(&position[1])
        })
    }

    fn to_document(&self, position: [f64; 2]) -> [f64; 2] {
        let origin = self.area.map_or([0, 0], |area| [area[0], area[1]]);
        std::array::from_fn(|axis| {
            (position[axis] - f64::from(origin[axis]) - self.offset[axis]) / self.scale
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
        let revision = self.session.revision();
        match &self.snapshot {
            Some((at, snapshot)) if *at == revision => snapshot.clone(),
            _ => {
                let snapshot = Snapshot {
                    document: Arc::new(self.session.document().clone()),
                    layers: Arc::new(self.session.layer_revisions().clone()),
                };
                self.snapshot = Some((revision, snapshot.clone()));
                snapshot
            }
        }
    }

    /// Where renders for export go, after the canvas's own.
    pub fn renders(&self) -> Renders {
        self.cache.renders()
    }

    /// The thumbnail of paint layer `id` and the revision it shows.
    pub fn thumbnail(&self, id: LayerId) -> Option<(u64, &Arc<Pixmap>)> {
        self.thumbnails
            .get(&id)
            .map(|(revision, pixels)| (*revision, pixels))
    }

    /// Asks for the thumbnails of paint layers whose pixels changed since
    /// theirs were drawn, unless some are already on their way.
    fn sync_thumbnails(&mut self) {
        if self.thumbnails_asked || matches!(self.interaction, Interaction::Drawing { .. }) {
            return;
        }
        let revisions = self.session.layer_revisions();
        let mut stale = Vec::new();
        paint_layers(&self.session.document().layers, &mut |id| {
            let revision = revisions.of(id);
            if self
                .thumbnails
                .get(&id)
                .is_none_or(|(shown, _)| *shown != revision)
            {
                stale.push((id, revision));
            }
        });
        if stale.is_empty() {
            return;
        }
        let snapshot = self.snapshot();
        self.cache
            .request_thumbnails(self.generation, stale, snapshot.document);
        self.thumbnails_asked = true;
    }

    /// Shows a pending transform as it is now, asks for a new split when the
    /// shown one no longer matches, and for thumbnails that are out of date.
    /// Playback shows whole frames instead of a split.
    pub fn sync(&mut self) {
        self.refresh_preview();
        self.refresh_text_preview();
        self.sync_thumbnails();
        if self.playback.is_some() {
            return;
        }
        let key = self.key();
        if self.split.as_ref().map(|(shown, _)| *shown) != Some(key) && self.requested != Some(key)
        {
            let budget = Budget::now(self.cache.surface_bytes() as u64);
            self.cache.set_surface_budget(budget.surfaces);
            let snapshot = self.snapshot();
            self.cache.request(key, snapshot);
            self.requested = Some(key);
        }
    }

    /// Takes a finished render if it is still wanted.
    pub fn adopt(&mut self, rendered: Rendered) {
        match rendered {
            Rendered::Split {
                key,
                split,
                display,
            } => {
                if key != self.key() || self.playback.is_some() {
                    return;
                }
                self.requested = None;
                self.held = None;
                self.display = display;
                self.split = Some((key, split));
                self.preview = None;
                self.text_preview = None;
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
                if let Some(playback) = self.playback.as_mut()
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
                if document != self.generation {
                    return;
                }
                self.thumbnails_asked = false;
                for (id, revision, pixels) in thumbnails {
                    self.thumbnails.insert(id, (revision, Arc::new(pixels)));
                }
                let document = self.session.document();
                self.thumbnails
                    .retain(|id, _| document.layer(*id).is_some());
            }
        }
    }

    pub fn is_playing(&self) -> bool {
        self.playback.is_some()
    }

    /// Plays from the current frame, or stops on the frame shown.
    pub fn toggle_playback(&mut self) {
        if let Some(playback) = self.playback.take() {
            self.held = Some(playback.shown);
            self.upload_all();
            return;
        }
        // Playing moves through frames, which applies a pending transform or
        // placed text.
        self.apply_pending();
        // Stopping asks for a split of the frame it stops on. Until then the
        // old one's layer surfaces would only keep the renderer from reusing
        // them for playback.
        self.split = None;
        self.requested = None;
        self.spare_coverage = None;
        let shown = self.held.take().unwrap_or_else(|| {
            let display = std::mem::replace(&mut self.display, Pixmap::new(1, 1));
            (Arc::new(display), 1)
        });
        tracing::debug!("playback started");
        self.playback = Some(Playback {
            next: self.session.frame() + 1,
            due: Instant::now(),
            frames: HashMap::new(),
            version: self.version(),
            preview: preview(self.scale),
            ahead: 0,
            shown,
            window: None,
        });
    }

    /// Shows the next frame if it is due and rendered, and returns when to
    /// look again; `None` when not playing.
    pub fn tick(&mut self, now: Instant) -> Option<Instant> {
        let document = self.session.document();
        let (frames, fps) = (document.frames, f64::from(document.frames_per_second));
        let period = Duration::from_secs_f64(1.0 / fps);
        let version = self.version();
        let preview = preview(self.scale);
        let shrink = preview.shrink;
        let playback = self.playback.as_ref()?;
        if playback.version != version || playback.preview != preview || playback.ahead == 0 {
            // The frames held are given back first, so they count as free.
            self.playback.as_mut()?.frames.clear();
            let (ahead, surfaces) = frames_ahead(document, shrink);
            self.cache.set_surface_budget(surfaces);
            let playback = self.playback.as_mut()?;
            playback.version = version;
            playback.preview = preview;
            playback.ahead = ahead;
            playback.window = None;
        }
        let playback = self.playback.as_mut()?;
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
            self.cache
                .request_frames(version, &missing, &snapshot, preview);
        }
        if shown {
            tracing::debug!(frame = cycle, "playback frame shown");
            let next = self.playback.as_ref().expect("playing").next;
            self.session.set_frame(next - 1);
            self.upload_all();
        }
        Some(wake)
    }

    /// Puts `rect` of the display back together, with the stroke being drawn.
    fn recomposite(&mut self, rect: Option<PixelRect>) {
        let (Some(rect), Some((_, split))) = (rect, &self.split) else {
            return;
        };
        let live = match &self.interaction {
            Interaction::Drawing { live, .. } => Some(live.as_ref()),
            _ => None,
        };
        composite(split, live, rect, &mut self.display);
        self.upload = union(self.upload, Some(rect));
    }

    pub fn apply(&mut self, input: CanvasInput) {
        match input {
            CanvasInput::Begin(Gesture::Pan, _, sample) => {
                self.interaction = Interaction::Panning {
                    last: sample.position,
                };
            }
            CanvasInput::Begin(Gesture::Draw, _, sample) if self.session.pending().is_some() => {
                self.sample_count += 1;
                self.begin_transform_drag(sample.position);
            }
            CanvasInput::Begin(Gesture::Draw, _, sample) if self.session.tool() == Tool::Select => {
                self.sample_count += 1;
                self.notice = None;
                let point = self.to_document(sample.position);
                self.session.begin_selection(point, self.combine());
                self.interaction = Interaction::Selecting;
            }
            CanvasInput::Begin(Gesture::Draw, _, sample) if self.session.tool() == Tool::Text => {
                self.sample_count += 1;
                self.press_text(sample.position);
            }
            CanvasInput::Begin(Gesture::Draw, _, sample)
                if matches!(self.session.tool(), Tool::Wand | Tool::Fill) =>
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
                        self.offset[0] += sample.position[0] - last[0];
                        self.offset[1] += sample.position[1] - last[1];
                        *last = sample.position;
                    }
                    Interaction::Drawing { .. } => self.extend_stroke(&sample),
                    Interaction::Selecting => {
                        let point = self.to_document(sample.position);
                        self.session.extend_selection(point);
                    }
                    Interaction::Transforming { .. } => self.drag_transform(sample.position),
                    Interaction::MovingText { .. } => self.drag_text(sample.position),
                    Interaction::Idle => {}
                }
            }
            CanvasInput::End(sample) => {
                self.sample_count += 1;
                match std::mem::replace(&mut self.interaction, Interaction::Idle) {
                    Interaction::Drawing { live, rect } => self.end_stroke(&sample, live, rect),
                    Interaction::Selecting => {
                        let point = self.to_document(sample.position);
                        let before = self.key();
                        let ended = self.session.end_selection(point);
                        if self.session.lasso_paints {
                            self.after_fill(before, ended);
                        }
                    }
                    Interaction::Transforming { .. } => self.drag_transform(sample.position),
                    moving @ Interaction::MovingText { .. } => {
                        self.interaction = moving;
                        self.drag_text(sample.position);
                        self.interaction = Interaction::Idle;
                    }
                    Interaction::Panning { .. } | Interaction::Idle => {}
                }
            }
            CanvasInput::Cancel => {
                self.cancel_gesture();
            }
            CanvasInput::Zoom { position, notches } => self.zoom(position, notches),
        }
    }

    /// Ends a drawing, panning or selecting gesture without its result;
    /// returns whether there was one.
    fn cancel_gesture(&mut self) -> bool {
        match std::mem::replace(&mut self.interaction, Interaction::Idle) {
            Interaction::Drawing { rect, .. } => {
                self.session.cancel_stroke();
                self.recomposite(rect);
            }
            Interaction::Selecting => {
                self.session.cancel_selection();
            }
            Interaction::Transforming { base, .. } => {
                self.session.set_transform(base);
                self.refresh_preview();
            }
            Interaction::MovingText { base, .. } => {
                if let Some(outline) = self
                    .session
                    .placed_text()
                    .map(|placed| placed.outline.clone())
                {
                    let _ = self.session.place_text(base, outline);
                }
                self.refresh_text_preview();
            }
            Interaction::Panning { .. } => {}
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
        if self.playback.take().is_some() {
            self.upload_all();
        }
        let read = self.read_reference();
        let reference = self
            .reference
            .as_ref()
            .filter(|_| read)
            .map(|pixels| pixels.data_as_u8_slice().as_chunks::<4>().0);
        let before = self.key();
        if self.session.tool() == Tool::Wand {
            let combine = self.combine();
            if let Err(error) = self.session.wand(point, combine, reference) {
                self.fill_notice(&error);
            }
        } else {
            let filled = self.session.bucket(point, reference);
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
        let filled = self.session.fill_selection();
        self.after_fill(before, filled.map(committed));
    }

    /// Draws `reference`'s pixels as the wand and the bucket read them into
    /// `self.reference`: put together from the split when it shows this
    /// frame and holds the layers, else drawn by the worker. `false` when
    /// there is nothing to read.
    fn read_reference(&mut self) -> bool {
        let document = self.session.document();
        let reference = match self.session.fill.reads {
            Reads::Current => Reference::Layer(self.session.current_layer()),
            Reads::Marked => Reference::Marked,
            Reads::Visible => Reference::Visible,
        };
        let plan = RenderPlan::reference(document, reference);
        if plan.layers.is_empty() {
            return false;
        }
        let [width, height] = document.canvas.map(|edge| edge as u16);
        let pixels = match self.reference.take() {
            Some(pixels) if pixels.width() == width && pixels.height() == height => pixels,
            _ => Pixmap::new(width, height),
        };
        let mut pixels = pixels;
        let key = self.key();
        let threads = std::thread::available_parallelism().map_or(1, usize::from);
        let from_split = self.split.as_ref().is_some_and(|(shown, split)| {
            *shown == key && split.reference(&plan, &mut pixels, threads)
        });
        if from_split {
            self.reference = Some(pixels);
            return true;
        }
        let document = self.snapshot().document;
        self.reference = self
            .cache
            .renders()
            .render(document, plan, self.session.frame());
        self.reference.is_some()
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
        let document = self.session.document();
        let added = match self.split.as_mut() {
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

    fn fill_notice(&mut self, error: &FillError) {
        let reads = self.session.fill.reads;
        self.notice = Some(match error {
            FillError::NoLayer => "Select a paint layer to draw on".to_owned(),
            FillError::HiddenLayer => "The current layer is hidden".to_owned(),
            FillError::NoReference if reads == Reads::Marked => tr("fill-no-reference").to_owned(),
            FillError::NoReference => "Select a paint layer to draw on".to_owned(),
            FillError::NothingThere if self.session.tool() == Tool::Wand => {
                tr("wand-nothing").to_owned()
            }
            FillError::NothingThere => tr("fill-nothing").to_owned(),
            FillError::OutsideSelection => tr("fill-outside").to_owned(),
            FillError::NoSelection => tr("fill-no-selection").to_owned(),
            FillError::Edit(error) => format!("The fill was not added: {error}"),
        });
    }

    fn begin_stroke(&mut self, kind: PointerKind, sample: &PointerSample) {
        self.notice = None;
        if self.playback.take().is_some() {
            self.upload_all();
        }
        // The eraser end of a pen erases whatever tool is chosen.
        let tool = self.session.tool();
        if kind == PointerKind::Pen && sample.inverted {
            self.session.set_tool(Tool::Eraser);
        }
        let begun = self.session.begin_stroke(self.input_point(sample));
        self.session.set_tool(tool);
        match begun {
            Ok(()) => {}
            Err(StrokeRefused::HiddenLayer) => {
                self.notice = Some("The current layer is hidden".to_owned());
                return;
            }
            Err(StrokeRefused::NoLayer) => {
                self.notice = Some("Select a paint layer to draw on".to_owned());
                return;
            }
        }
        let live = self.session.live().expect("a stroke has begun");
        let document = self.session.document();
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
        if !self.session.extend_stroke(point) {
            return;
        }
        let points = &self.session.live().expect("drawing").points;
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
        let outcome = self.session.end_stroke(point);
        self.spare_coverage = Some(live.into_spare());
        match outcome {
            Ok(Outcome::Committed(_)) => {}
            Ok(Outcome::NoChange) => return self.recomposite(rect),
            Err(error) => {
                tracing::error!(%error, "the stroke could not be committed");
                self.notice = Some(format!("The stroke was not added: {error}"));
                return self.recomposite(rect);
            }
        }
        let after = self.key();
        let document = self.session.document();
        let added = match self.split.as_mut() {
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
        let scale = (self.scale * ZOOM_STEP.powf(f64::from(notches)))
            .clamp(*ZOOM_RANGE.start(), *ZOOM_RANGE.end());
        let origin = self.area.map_or([0, 0], |area| [area[0], area[1]]);
        self.scale = scale;
        // Keep the document point under the pointer where it is.
        self.offset = std::array::from_fn(|axis| {
            (position[axis] - f64::from(origin[axis]) - anchor[axis] * scale).round()
        });
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
        self.area = Some(area);
        if !self.placed {
            self.place(area);
        }
        area
    }

    /// 100% if the document fits, otherwise small enough to fit, centred.
    fn place(&mut self, area: [i32; 4]) {
        let size = self.session.document().canvas.map(f64::from);
        let room = [f64::from(area[2] - area[0]), f64::from(area[3] - area[1])];
        if room[0] <= 0.0 || room[1] <= 0.0 {
            return;
        }
        self.scale = (room[0] / size[0]).min(room[1] / size[1]).min(1.0);
        self.offset =
            std::array::from_fn(|axis| ((room[axis] - size[axis] * self.scale) / 2.0).round());
        self.placed = true;
    }

    /// Marks the whole display for upload, as a new GPU device needs.
    pub fn upload_all(&mut self) {
        let shown = self.display();
        let [width, height] = [shown.width(), shown.height()];
        self.upload = Some([0, 0, u32::from(width), u32::from(height)]);
    }

    /// The display pixels changed since the last call.
    pub fn take_upload(&mut self) -> Option<PixelRect> {
        self.upload.take()
    }

    /// What the canvas shows: the playback frame, the frame playback
    /// stopped on, or the edited image.
    pub fn display(&self) -> &Pixmap {
        self.shown().0
    }

    /// The pixels shown and how much smaller than the canvas they are.
    fn shown(&self) -> (&Pixmap, u32) {
        let frame = match &self.playback {
            Some(playback) => Some(&playback.shown),
            None => self.held.as_ref(),
        };
        frame.map_or((&self.display, 1), |(pixels, shrink)| (pixels, *shrink))
    }

    /// Where the document is drawn, in client physical pixels.
    pub fn placement(&self) -> Placement {
        let origin = self.area.map_or([0, 0], |area| [area[0], area[1]]);
        // A playback frame drawn smaller has fewer pixels to spread out.
        let shrink = self.shown().1;
        Placement {
            offset: [
                (f64::from(origin[0]) + self.offset[0]) as f32,
                (f64::from(origin[1]) + self.offset[1]) as f32,
            ],
            scale: (self.scale * f64::from(shrink)) as f32,
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
fn paint_layers(layers: &[ugu_core::document::Layer], each: &mut impl FnMut(LayerId)) {
    for layer in layers {
        match &layer.kind {
            LayerKind::Paint(_) => each(layer.id),
            LayerKind::Group(group) => paint_layers(&group.children, each),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_playback_frame_drawn_smaller_is_uploaded_whole_and_no_more() {
        let (to_test, renders) = std::sync::mpsc::channel();
        let mut canvas = super::Canvas::new(Document::new([320, 200]), move |rendered| {
            let _ = to_test.send(rendered);
        });
        canvas.scale = 0.5;
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
        assert_eq!(canvas.placement().scale, 1.0);
        canvas.sync();
        while canvas.split.is_none() {
            let rendered = renders
                .recv_timeout(Duration::from_secs(10))
                .expect("the split renders");
            canvas.adopt(rendered);
        }
        assert_eq!(canvas.display().width(), 320);
        assert_eq!(canvas.placement().scale, 0.5);
    }

    #[test]
    fn playback_follows_a_canvas_change() {
        let (to_test, renders) = std::sync::mpsc::channel();
        let mut canvas = super::Canvas::new(Document::new([320, 200]), move |rendered| {
            let _ = to_test.send(rendered);
        });
        canvas.scale = 0.5;
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
        while canvas.split.as_ref().map(|(key, _)| *key) != Some(canvas.key()) {
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
        canvas.edit(|session| {
            let point = |position, time| ugu_session::InputPoint {
                position,
                pressure: None,
                time,
            };
            session.begin_stroke(point(corners[0], 0.0)).unwrap();
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

        canvas.session.set_tool(Tool::Wand);
        canvas.click_area([160.0, 100.0]);
        let from_split = canvas
            .session()
            .selection()
            .cloned()
            .expect("the box's inside");
        canvas.edit(Session::deselect);
        canvas.split = None;
        canvas.click_area([160.0, 100.0]);
        assert_eq!(canvas.session().selection(), Some(&from_split));
        canvas.edit(Session::deselect);
        settle(&mut canvas, &renders);

        canvas.session.set_tool(Tool::Fill);
        canvas.session.pen.color = ugu_core::ops::Rgba8([30, 120, 200, 255]);
        canvas.click_area([160.0, 100.0]);
        assert_eq!(canvas.notice(), None);
        // Added to the split, not drawn again.
        assert_eq!(
            canvas.split.as_ref().map(|(key, _)| *key),
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
        canvas.edit(|session| {
            let point = |position, time| ugu_session::InputPoint {
                position,
                pressure: None,
                time,
            };
            session.begin_stroke(point([40.0, 40.0], 0.0)).unwrap();
            for step in 1..=60 {
                let t = f64::from(step) / 60.0;
                let at = [40.0 + 200.0 * t, 40.0 + 100.0 * t + (t * 12.0).sin() * 20.0];
                session.extend_stroke(point(at, f64::from(step) * 4.0));
            }
            session.end_stroke(point([240.0, 140.0], 250.0)).unwrap();
            session.selection_shape = ugu_session::ShapeKind::Rectangle;
            session.begin_selection([60.0, 50.0], Combine::Replace);
            session.extend_selection([180.0, 130.0]);
            session.end_selection([180.0, 130.0]).unwrap();
        });
        settle(&mut canvas, &renders);
        let before = canvas.display().clone();

        canvas.begin_transform();
        assert!(canvas.preview.is_some());
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
            canvas.split.as_ref().map(|(key, _)| *key),
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
        canvas.edit(|session| {
            let point = |position, time| ugu_session::InputPoint {
                position,
                pressure: None,
                time,
            };
            session.begin_stroke(point([40.0, 40.0], 0.0)).unwrap();
            for step in 1..=60 {
                let t = f64::from(step) / 60.0;
                session.extend_stroke(point([40.0 + 200.0 * t, 40.0 + 100.0 * t], t * 250.0));
            }
            session.end_stroke(point([240.0, 140.0], 250.0)).unwrap();
            session.selection_shape = ugu_session::ShapeKind::Rectangle;
            session.begin_selection([60.0, 50.0], Combine::Replace);
            session.end_selection([180.0, 130.0]).unwrap();
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
            assert!(canvas.preview.is_some());
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
            assert!(canvas.preview.is_none() && canvas.session().pending().is_none());
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
        canvas.edit(|session| {
            let point = |position, time| ugu_session::InputPoint {
                position,
                pressure: None,
                time,
            };
            session.begin_stroke(point([20.0, 150.0], 0.0)).unwrap();
            session.end_stroke(point([300.0, 60.0], 100.0)).unwrap();
            session.set_tool(Tool::Text);
            session.pen.width = 3.0;
        });
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
        assert!(canvas.text_preview.is_some());
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
            canvas.split.as_ref().map(|(key, _)| *key),
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
        canvas.edit(|session| {
            let point = |position, time| ugu_session::InputPoint {
                position,
                pressure: None,
                time,
            };
            session.begin_stroke(point([10.0, 60.0], 0.0)).unwrap();
            for step in 1..=60 {
                let x = 10.0 + 3.0 * f64::from(step);
                session.extend_stroke(point(
                    [x, 60.0 + (x / 9.0).sin() * 20.0],
                    f64::from(step) * 4.0,
                ));
            }
            session.end_stroke(point([190.0, 60.0], 250.0)).unwrap();
            session.selection_shape = ugu_session::ShapeKind::Rectangle;
            session.begin_selection([50.0, 20.0], Combine::Replace);
            session.extend_selection([120.0, 100.0]);
            session.end_selection([120.0, 100.0]).unwrap();
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
}
