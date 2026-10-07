// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The document on screen: drawing, panning and zooming.
//!
//! The image shown is the cached split put together. A stroke being drawn is
//! composited over it within the pixels it changes, and at pen-up the
//! committed stroke is added to the cached layer instead of rendering the
//! frame again. Anything else that changes the document asks the cache
//! worker for a new split and keeps showing the old one until it arrives.

use std::time::Instant;

use ugu_core::document::{Document, LayerKind};
use ugu_core::edit::Outcome;
use ugu_core::motion::frame_in_cycle;
use ugu_core::ops::Op;
use ugu_render::compose::{Split, Stamp, composite, premultiplied, stroke_color};
use ugu_render::live::LiveStroke;
use ugu_render::raster::PixelRect;
use ugu_render::stroke::Pen;
use ugu_render::view::Placement;
use ugu_session::{InputPoint, Session, StrokeRefused, Tool};
use ugu_win::clock::Ticks;
use ugu_win::pointer::{PointerKind, PointerSample};
use vello_cpu::Pixmap;

use crate::cache::{CacheWorker, Key, Rendered};
use crate::input::{CanvasInput, Gesture};

/// Shown around the document, opaque straight RGBA.
pub const WORKSPACE: [u8; 4] = [64, 66, 70, 255];
const ZOOM_STEP: f64 = 1.25;
const ZOOM_RANGE: std::ops::RangeInclusive<f64> = 0.05..=32.0;

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
    display: Pixmap,
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
    notice: Option<&'static str>,
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
            upload: Some([0, 0, u32::from(width), u32::from(height)]),
            stamp: Stamp::default(),
            interaction: Interaction::Idle,
            area: None,
            scale: 1.0,
            offset: [0.0, 0.0],
            placed: false,
            sample_count: 0,
            notice: None,
        }
    }

    pub fn session(&self) -> &Session {
        &self.session
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

    pub fn notice(&self) -> Option<&'static str> {
        self.notice
    }

    pub fn scale(&self) -> f64 {
        self.scale
    }

    fn key(&self) -> Key {
        let document = self.session.document();
        Key {
            revision: self.session.revision(),
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

    /// Asks for a new split when the shown one no longer matches.
    pub fn sync(&mut self) {
        let key = self.key();
        if self.split.as_ref().map(|(shown, _)| *shown) != Some(key) && self.requested != Some(key)
        {
            self.cache.request(key, self.session.document().clone());
            self.requested = Some(key);
        }
    }

    /// Takes a finished split if it is still the one wanted.
    pub fn adopt(&mut self, rendered: Rendered) {
        if rendered.key != self.key() {
            return;
        }
        self.requested = None;
        self.display = rendered.display;
        self.split = Some((rendered.key, rendered.split));
        let [width, height] = [self.display.width(), self.display.height()];
        self.upload = Some([0, 0, u32::from(width), u32::from(height)]);
        if let Interaction::Drawing { rect, .. } = &self.interaction {
            let rect = *rect;
            self.recomposite(rect);
        }
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
                self.notice = Some("The current layer is hidden");
                return;
            }
            Err(StrokeRefused::NoLayer) => {
                self.notice = Some("There is no layer to draw on");
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
                return self.recomposite(rect);
            }
        }
        let after = self.key();
        let document = self.session.document();
        let added = match self.split.as_mut() {
            Some((key, split)) if *key == before && after.layer == before.layer => {
                last_stroke(document, after.layer).and_then(|(stroke, erase)| {
                    let wobble = wobble_of(document, after.layer);
                    let changed = self.stamp.apply_stroke(
                        &mut split.surface,
                        stroke,
                        erase,
                        wobble,
                        after.frame,
                    );
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
        let [width, height] = [self.display.width(), self.display.height()];
        self.upload = Some([0, 0, u32::from(width), u32::from(height)]);
    }

    /// The display pixels changed since the last call.
    pub fn take_upload(&mut self) -> Option<PixelRect> {
        self.upload.take()
    }

    pub fn display(&self) -> &Pixmap {
        &self.display
    }

    /// Where the document is drawn, in client physical pixels.
    pub fn placement(&self) -> Placement {
        let origin = self.area.map_or([0, 0], |area| [area[0], area[1]]);
        Placement {
            offset: [
                (f64::from(origin[0]) + self.offset[0]) as f32,
                (f64::from(origin[1]) + self.offset[1]) as f32,
            ],
            scale: self.scale as f32,
        }
    }
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
