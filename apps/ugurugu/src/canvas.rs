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

use ugu_core::document::{Document, LayerKind};
use ugu_core::edit::Outcome;
use ugu_core::motion::frame_in_cycle;
use ugu_core::ops::Op;
use ugu_render::compose::{Split, Stamp, composite, premultiplied, stroke_color};
use ugu_render::document::{Purpose, SURFACE_BUDGET, TILE_EDGE, scaled_size, surface_estimate};
use ugu_render::live::LiveStroke;
use ugu_render::plan::RenderPlan;
use ugu_render::raster::PixelRect;
use ugu_render::stroke::Pen;
use ugu_render::view::Placement;
use ugu_session::{InputPoint, Session, StrokeRefused, Tool};
use ugu_win::clock::Ticks;
use ugu_win::pointer::{PointerKind, PointerSample};
use vello_cpu::Pixmap;

use crate::cache::{CacheWorker, Key, Rendered, Renders, Snapshot, Version};
use crate::input::{CanvasInput, Gesture};

/// Shown around the document, opaque straight RGBA.
pub const WORKSPACE: [u8; 4] = [64, 66, 70, 255];
const ZOOM_STEP: f64 = 1.25;
const ZOOM_RANGE: std::ops::RangeInclusive<f64> = 0.05..=32.0;
/// Memory for the layers' own surfaces and the playback frames rendered
/// ahead together. Frames beyond it are rendered when they come up.
const RENDER_BUDGET: usize = 768 * 1024 * 1024;

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
    /// How much smaller than the canvas the frames are drawn.
    shrink: u32,
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
}

pub struct Canvas {
    session: Session,
    cache: CacheWorker,
    split: Option<(Key, Split)>,
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
    /// Canvas area in client physical pixels: left, top, right, bottom.
    area: Option<[i32; 4]>,
    /// Physical pixels per document pixel.
    scale: f64,
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
            requested: None,
            display: Pixmap::new(width, height),
            held: None,
            upload: Some([0, 0, u32::from(width), u32::from(height)]),
            stamp: Stamp::default(),
            interaction: Interaction::Idle,
            area: None,
            scale: 1.0,
            offset: [0.0, 0.0],
            placed: false,
            sample_count: 0,
            notice: None,
            playback: None,
            snapshot: None,
            generation: 0,
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
        self.requested = None;
        self.display = Pixmap::new(width, height);
        self.held = None;
        self.interaction = Interaction::Idle;
        self.playback = None;
        self.upload_all();
        self.snapshot = None;
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
        change(&mut self.session)
    }

    /// Zooms around the middle of the canvas area.
    pub fn zoom_in_place(&mut self, notches: f32) {
        if let Some([left, top, right, bottom]) = self.area {
            let middle = [f64::from(left + right) / 2.0, f64::from(top + bottom) / 2.0];
            self.zoom(middle, notches);
        }
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

    /// Asks for a new split when the shown one no longer matches. Playback
    /// shows whole frames instead.
    pub fn sync(&mut self) {
        if self.playback.is_some() {
            return;
        }
        let key = self.key();
        if self.split.as_ref().map(|(shown, _)| *shown) != Some(key) && self.requested != Some(key)
        {
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
                self.upload_all();
                if let Interaction::Drawing { rect, .. } = &self.interaction {
                    let rect = *rect;
                    self.recomposite(rect);
                }
            }
            Rendered::Frame {
                version,
                frame,
                shrink,
                pixels,
            } => {
                if let Some(playback) = self.playback.as_mut()
                    && playback.version == version
                    && playback.shrink == shrink
                {
                    playback.frames.insert(frame, pixels);
                }
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
        self.edit(|_| ());
        // Stopping asks for a split of the frame it stops on. Until then the
        // old one's layer surfaces would only keep the renderer from reusing
        // them for playback.
        self.split = None;
        self.requested = None;
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
            shrink: preview_shrink(self.scale),
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
        let shrink = preview_shrink(self.scale);
        let playback = self.playback.as_ref()?;
        if playback.version != version || playback.shrink != shrink || playback.ahead == 0 {
            let ahead = frames_ahead(document, shrink);
            let playback = self.playback.as_mut()?;
            playback.frames.clear();
            playback.version = version;
            playback.shrink = shrink;
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
                .request_frames(version, &missing, &snapshot, shrink);
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
                    Interaction::Idle => {}
                }
            }
            CanvasInput::End(sample) => {
                self.sample_count += 1;
                match std::mem::replace(&mut self.interaction, Interaction::Idle) {
                    Interaction::Drawing { live, rect } => self.end_stroke(&sample, live, rect),
                    Interaction::Panning { .. } | Interaction::Idle => {}
                }
            }
            CanvasInput::Cancel => {
                if let Interaction::Drawing { rect, .. } =
                    std::mem::replace(&mut self.interaction, Interaction::Idle)
                {
                    self.session.cancel_stroke();
                    self.recomposite(rect);
                }
            }
            CanvasInput::Zoom { position, notches } => self.zoom(position, notches),
        }
    }

    fn begin_stroke(&mut self, kind: PointerKind, sample: &PointerSample) {
        self.notice = None;
        if self.playback.take().is_some() {
            self.upload_all();
        }
        // The eraser end of a pen erases whatever tool is chosen.
        let tool = self.session.tool;
        if kind == PointerKind::Pen && sample.inverted {
            self.session.tool = Tool::Eraser;
        }
        let begun = self.session.begin_stroke(self.input_point(sample));
        self.session.tool = tool;
        match begun {
            Ok(()) => {}
            Err(StrokeRefused::HiddenLayer) => {
                self.notice = Some("The current layer is hidden".to_owned());
                return;
            }
            Err(StrokeRefused::NoLayer) => {
                self.notice = Some("There is no layer to draw on".to_owned());
                return;
            }
        }
        let live = self.session.live().expect("a stroke has begun");
        let document = self.session.document();
        let pen = Pen {
            width: live.template.width,
            brush: live.template.brush,
            seed: live.template.seed,
            wobble: f64::from(wobble_of(document, live.layer))
                * f64::from(live.template.brush.wobble_scale),
        };
        let size = document.canvas.map(|edge| edge as u16);
        let color = premultiplied(stroke_color(&live.template, live.erase));
        let mut stroke = LiveStroke::new(size, pen, self.key().frame, color, live.erase);
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
        drop(live);
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
                last_stroke(document, after.layer).and_then(|(stroke, erase)| {
                    let wobble = wobble_of(document, after.layer);
                    let changed = split.stamp(&mut self.stamp, stroke, erase, wobble);
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
/// render budget beside the layers' own surfaces; at least 2.
fn frames_ahead(document: &Document, shrink: u32) -> u32 {
    let plan = RenderPlan::new(document, Purpose::Display);
    let surfaces = surface_estimate(document, &plan, shrink, TILE_EDGE).min(SURFACE_BUDGET);
    let [width, height] = scaled_size(document.canvas, shrink);
    let frame = width as usize * height as usize * 4;
    let fit = (RENDER_BUDGET - surfaces) / frame.max(1);
    fit.clamp(2, document.frames as usize) as u32
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

/// The wobble amount of strokes on `layer`.
fn wobble_of(document: &Document, layer: ugu_core::document::LayerId) -> f32 {
    match document.layer(layer).map(|layer| &layer.kind) {
        Some(LayerKind::Paint(paint)) => paint.wobble.unwrap_or(document.wobble).amount,
        _ => document.wobble.amount,
    }
}

/// The stroke the last operation of `layer` draws, and whether it erases.
fn last_stroke(
    document: &Document,
    layer: ugu_core::document::LayerId,
) -> Option<(&ugu_core::store::Stroke, bool)> {
    let Some(LayerKind::Paint(paint)) = document.layer(layer).map(|layer| &layer.kind) else {
        return None;
    };
    let (id, erase) = match paint.ops.last()? {
        Op::Paint { stroke, .. } => (stroke, false),
        Op::Erase { stroke, .. } => (stroke, true),
        _ => return None,
    };
    document.store.strokes.get(id).map(|stroke| (stroke, erase))
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
}
