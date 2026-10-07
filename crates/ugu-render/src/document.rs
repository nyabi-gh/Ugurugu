// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Draws one frame of a document with Vello CPU.
//!
//! Each paint layer and isolated section is a Vello layer: its operations
//! draw on a transparent surface in order, so an eraser removes only what is
//! below it in the same surface, and the surface is then drawn over what is
//! beneath with its opacity. This is the meaning `ugu_core::semantics` pins.
//! Only what M2 can draw is accepted; `check` names the rest.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ugu_core::document::{Document, LayerId, LayerKind};
use ugu_core::history::LayerRevisions;
use ugu_core::motion::frame_in_cycle;
use ugu_core::ops::{Op, PaintLayer, Wobble};
use ugu_core::store::{BrushEngine, Store, Stroke};
use vello_cpu::color::AlphaColor;
use vello_cpu::kurbo::{Affine, BezPath};
use vello_cpu::peniko::{BlendMode, Compose, Mix};
use vello_cpu::{Pixmap, RasterizerSettings, RenderContext, RenderSettings, Resources};

use crate::compose::premultiplied;
use crate::composite::{self, Source};
use crate::plan::RenderPlan;
use crate::raster::document_level;
use crate::stroke::{self, Pen, Resampler};
use crate::tile::TiledSurface;

/// Content this build cannot draw yet, and the milestone that adds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unsupported {
    /// Fills, images, selections and their clips (M4).
    Fill,
    Image,
    Selection,
    /// Crops and resizes (M4).
    CanvasChange,
    /// Airbrush and spray (M4).
    Brush,
}

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Fill => "fills",
            Self::Image => "images",
            Self::Selection => "selections",
            Self::CanvasChange => "canvas crops or resizes",
            Self::Brush => "airbrush or spray strokes",
        })
    }
}

/// Whether this build can draw `document`.
pub fn check(document: &Document) -> Result<(), Unsupported> {
    check_layers(&document.layers, &document.store)
}

fn check_layers(layers: &[ugu_core::document::Layer], store: &Store) -> Result<(), Unsupported> {
    for layer in layers {
        match &layer.kind {
            LayerKind::Paint(paint) => check_ops(&paint.ops, store)?,
            LayerKind::Group(group) => check_layers(&group.children, store)?,
        }
    }
    Ok(())
}

fn check_ops(ops: &[Op], store: &Store) -> Result<(), Unsupported> {
    for op in ops {
        match op {
            Op::Paint { stroke, clip } | Op::Erase { stroke, clip } => {
                if clip.is_some() {
                    return Err(Unsupported::Selection);
                }
                if store
                    .strokes
                    .get(stroke)
                    .is_some_and(|stroke| stroke.brush.engine != BrushEngine::Line)
                {
                    return Err(Unsupported::Brush);
                }
            }
            Op::Fill { .. } => return Err(Unsupported::Fill),
            Op::PlaceImage { .. } => return Err(Unsupported::Image),
            Op::TransformSelection { .. } | Op::ClearSelection { .. } => {
                return Err(Unsupported::Selection);
            }
            Op::Crop { .. } | Op::Resample { .. } => return Err(Unsupported::CanvasChange),
            Op::Isolated(section) => check_ops(&section.ops, store)?,
        }
    }
    Ok(())
}

/// Which layers a frame shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// Everything visible, reference layers included.
    Display,
    /// Visible layers except reference layers.
    Export,
}

pub struct DocumentRenderer {
    /// Draws one layer at a time with every thread.
    main: Raster,
    /// One single-threaded rasterizer per thread, to draw several layers at
    /// once.
    singles: Vec<Raster>,
    level: vello_cpu::Level,
    threads: usize,
    /// The paint layers' own surfaces of the last render, kept for reuse.
    cache: HashMap<LayerId, Cached>,
    /// Layers the last render drew.
    drawn: usize,
    stop: Option<Arc<AtomicBool>>,
    /// Bytes the layers' own surfaces may take; beyond it, frames are put
    /// together one layer at a time.
    surface_budget: usize,
    tile_edge: u32,
    timings: Timings,
}

/// A layer's own pixels and what they were drawn from.
struct Cached {
    /// `None` when drawn without revisions, so never reused.
    revision: Option<u64>,
    /// The frame within the cycle, or `None` for a layer that does not move.
    frame: Option<u32>,
    surface: Arc<TiledSurface>,
}

/// A layer to draw, and the surface to draw it on.
struct Work<'a> {
    id: LayerId,
    paint: &'a PaintLayer,
    revision: Option<u64>,
    /// `None` when the layer does not move.
    frame: Option<u32>,
    surface: TiledSurface,
    /// Whether it was drawn before a stop.
    drawn: bool,
}

/// Where the last render spent its time. With layers drawn at once, the
/// layer stages add up the time of every thread.
#[derive(Clone, Copy, Debug, Default)]
pub struct Timings {
    pub outlines: std::time::Duration,
    /// Handing paths to Vello, up to and including its flush.
    pub encode: std::time::Duration,
    pub rasterize: std::time::Duration,
    /// Putting the layer surfaces together.
    pub composite: std::time::Duration,
}

impl std::ops::AddAssign for Timings {
    fn add_assign(&mut self, other: Self) {
        self.outlines += other.outlines;
        self.encode += other.encode;
        self.rasterize += other.rasterize;
        self.composite += other.composite;
    }
}

/// A Vello context and the threads that make stroke outlines for it.
struct Raster {
    context: RenderContext,
    resources: Resources,
    threads: usize,
}

/// Bytes the layers' own surfaces may take by default: half of the working
/// set budget (scope.md), the rest left for playback frames, the display
/// and the program.
pub const SURFACE_BUDGET: usize = 512 * 1024 * 1024;

/// The size a canvas of `size` is drawn at with `shrink`.
pub fn scaled_size(size: [u32; 2], shrink: u32) -> [u32; 2] {
    size.map(|edge| edge.div_ceil(shrink))
}

/// Pixels along a side of a layer surface tile.
pub const TILE_EDGE: u32 = 128;

/// One thing to draw, in document order.
enum Step<'a> {
    /// Starts a surface: a layer or an isolated section.
    Push(f32),
    Pop,
    Draw {
        stroke: &'a Stroke,
        pen: Pen,
        erase: bool,
    },
}

impl DocumentRenderer {
    /// `threads` 0 draws on the calling thread; the result is the same.
    pub fn new(threads: u16) -> Self {
        Self::with_level(threads, document_level())
    }

    /// Uses the SIMD instructions of `level` instead of the document level.
    pub fn with_level(threads: u16, level: vello_cpu::Level) -> Self {
        Self {
            main: Raster::new(level, threads),
            singles: Vec::new(),
            level,
            threads: usize::from(threads.max(1)),
            cache: HashMap::new(),
            drawn: 0,
            stop: None,
            surface_budget: SURFACE_BUDGET,
            tile_edge: TILE_EDGE,
            timings: Timings::default(),
        }
    }

    pub fn timings(&self) -> Timings {
        self.timings
    }

    /// Lets the layers' own surfaces take `bytes` instead of
    /// `SURFACE_BUDGET`.
    pub fn set_surface_budget(&mut self, bytes: usize) {
        self.surface_budget = bytes;
    }

    /// Whether the layer surfaces of `plan` drawn at 1/`shrink` would take
    /// more than the budget.
    pub fn over_budget(&self, document: &Document, plan: &RenderPlan, shrink: u32) -> bool {
        surface_estimate(document, plan, shrink, self.tile_edge) > self.surface_budget
    }

    pub(crate) fn is_stopped(&self) -> bool {
        self.stopped()
    }

    pub(crate) fn thread_count(&self) -> usize {
        self.threads
    }

    /// `paint`'s own pixels on a surface of its own, with every thread.
    pub(crate) fn draw_alone(
        &mut self,
        document: &Document,
        frame: u32,
        shrink: u32,
        paint: &PaintLayer,
    ) -> TiledSurface {
        let size = scaled_size(document.canvas, shrink);
        let mut surface = TiledSurface::new(size, self.tile_edge);
        self.timings += self
            .main
            .draw_layer(document, frame, shrink, paint, &mut surface);
        surface
    }

    /// Uses tiles of `edge` pixels, a multiple of 4, instead of `TILE_EDGE`.
    pub fn set_tile_edge(&mut self, edge: u32) {
        self.tile_edge = edge;
    }

    /// Bytes held by the layer surfaces kept for reuse.
    pub fn surface_bytes(&self) -> usize {
        self.cache
            .values()
            .map(|cached| cached.surface.bytes())
            .sum()
    }

    /// Layers the last render drew rather than reused.
    pub fn layers_drawn(&self) -> usize {
        self.drawn
    }

    /// The surface of `layer` from the last render.
    pub fn surface(&self, layer: LayerId) -> Option<Arc<TiledSurface>> {
        self.cache.get(&layer).map(|cached| cached.surface.clone())
    }

    /// Lets go of the surface of `layer`, so it is drawn again next time.
    pub fn forget(&mut self, layer: LayerId) {
        self.cache.remove(&layer);
    }

    /// Draws `frame` of a document that passed `check` into `pixmap`, which
    /// must have the canvas size, as premultiplied RGBA8.
    pub fn render(
        &mut self,
        document: &Document,
        frame: i64,
        purpose: Purpose,
        pixmap: &mut Pixmap,
    ) {
        let plan = RenderPlan::new(document, purpose);
        self.render_plan(document, &plan, frame, None, pixmap);
    }

    /// Draws `frame` as `plan`, made from `document`, says. With `revisions`
    /// of the document, a layer whose pixels are as when it was last drawn
    /// here is reused. Returns `false` when the stop flag ended it early;
    /// `pixmap` is then not finished, and the layers drawn are kept.
    pub fn render_plan(
        &mut self,
        document: &Document,
        plan: &RenderPlan,
        frame: i64,
        revisions: Option<&LayerRevisions>,
        pixmap: &mut Pixmap,
    ) -> bool {
        self.render_scaled(document, plan, frame, revisions, 1, pixmap)
    }

    /// `render_plan` at 1/`shrink` of the canvas size in each direction,
    /// `shrink` a power of two: the same plan and compositing with the
    /// strokes drawn smaller. `pixmap` has the canvas size divided by
    /// `shrink`, rounded up.
    pub fn render_scaled(
        &mut self,
        document: &Document,
        plan: &RenderPlan,
        frame: i64,
        revisions: Option<&LayerRevisions>,
        shrink: u32,
        pixmap: &mut Pixmap,
    ) -> bool {
        let size = scaled_size(document.canvas, shrink);
        let [width, height] = size.map(|edge| edge as u16);
        assert_eq!([pixmap.width(), pixmap.height()], [width, height]);
        let frame = frame_in_cycle(frame, document.frames);
        let edge = self.tile_edge;
        if self.over_budget(document, plan, shrink) {
            self.cache.clear();
            self.drawn = plan.layers.len();
            self.timings = Timings::default();
            return self.render_streamed(document, plan, frame, shrink, pixmap);
        }
        self.cache.retain(|id, cached| {
            plan.moves(*id).is_some()
                && cached.surface.size() == size
                && cached.surface.edge() == edge
        });
        let mut work = Vec::new();
        for (id, moves) in &plan.layers {
            let revision = revisions.map(|revisions| revisions.of(*id));
            let at = moves.then_some(frame);
            if self.cache.get(id).is_some_and(|cached| {
                cached.revision.is_some() && cached.revision == revision && cached.frame == at
            }) {
                continue;
            }
            let Some(LayerKind::Paint(paint)) = document.layer(*id).map(|layer| &layer.kind) else {
                panic!("the plan was made from another document");
            };
            // A surface still shared elsewhere is left to its other owner.
            let surface = self
                .cache
                .remove(id)
                .and_then(|cached| Arc::try_unwrap(cached.surface).ok())
                .unwrap_or_else(|| TiledSurface::new(size, edge));
            work.push(Work {
                id: *id,
                paint,
                revision,
                frame: at,
                surface,
                drawn: false,
            });
        }
        self.drawn = work.len();
        self.timings = Timings::default();
        if work.len() >= self.threads && self.threads > 1 {
            self.draw_at_once(document, &mut work, frame, shrink);
        } else {
            for each in &mut work {
                if self.stopped() {
                    break;
                }
                self.timings +=
                    self.main
                        .draw_layer(document, frame, shrink, each.paint, &mut each.surface);
                each.drawn = true;
            }
        }
        let complete = work.iter().all(|each| each.drawn);
        for each in work.into_iter().filter(|each| each.drawn) {
            let cached = Cached {
                revision: each.revision,
                frame: each.frame,
                surface: Arc::new(each.surface),
            };
            self.cache.insert(each.id, cached);
        }
        if !complete {
            return false;
        }
        let started = std::time::Instant::now();
        let sources: Vec<Source<'_>> = plan
            .layers
            .iter()
            .map(|(id, _)| {
                self.cache
                    .get(id)
                    .map_or(Source::Empty, |cached| Source::Tiles(&cached.surface))
            })
            .collect();
        let background = premultiplied(document.background.0);
        let rect = [0, 0, u32::from(width), u32::from(height)];
        let finished = composite::evaluate(
            &composite::program(plan),
            Some(background),
            &sources,
            rect,
            pixmap,
            self.threads,
            self.stop.as_deref(),
        );
        self.timings.composite = started.elapsed();
        finished
    }

    /// Stops renders early, between layers and between rows put together,
    /// once `stop` is set.
    pub fn set_stop(&mut self, stop: Arc<AtomicBool>) {
        self.stop = Some(stop);
    }

    fn stopped(&self) -> bool {
        self.stop
            .as_ref()
            .is_some_and(|stop| stop.load(Ordering::Relaxed))
    }

    /// Draws each layer on one thread, as many layers at once as there are
    /// threads.
    fn draw_at_once(
        &mut self,
        document: &Document,
        work: &mut [Work<'_>],
        frame: u32,
        shrink: u32,
    ) {
        while self.singles.len() < self.threads {
            self.singles.push(Raster::new(self.level, 0));
        }
        let next = std::sync::atomic::AtomicUsize::new(0);
        let slots: Vec<std::sync::Mutex<&mut Work<'_>>> =
            work.iter_mut().map(std::sync::Mutex::new).collect();
        let stop = self.stop.as_deref();
        let timings = std::thread::scope(|scope| {
            let workers: Vec<_> = self
                .singles
                .iter_mut()
                .map(|raster| {
                    let (next, slots) = (&next, &slots);
                    scope.spawn(move || {
                        let mut timings = Timings::default();
                        loop {
                            if stop.is_some_and(|stop| stop.load(Ordering::Relaxed)) {
                                return timings;
                            }
                            let index = next.fetch_add(1, Ordering::Relaxed);
                            let Some(slot) = slots.get(index) else {
                                return timings;
                            };
                            let mut slot = slot.lock().expect("one worker per layer");
                            let each = &mut **slot;
                            timings += raster.draw_layer(
                                document,
                                frame,
                                shrink,
                                each.paint,
                                &mut each.surface,
                            );
                            each.drawn = true;
                        }
                    })
                })
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().expect("layer worker panicked"))
                .fold(Timings::default(), |mut sum, each| {
                    sum += each;
                    sum
                })
        });
        self.timings += timings;
    }
}

impl Raster {
    fn new(level: vello_cpu::Level, threads: u16) -> Self {
        Self {
            context: RenderContext::new_with(
                1,
                1,
                RenderSettings {
                    level,
                    num_threads: threads,
                },
            ),
            resources: Resources::new(),
            threads: usize::from(threads.max(1)),
        }
    }

    /// Draws a paint layer's own pixels at 1/`shrink` of their size into the
    /// tiles its strokes reach.
    fn draw_layer(
        &mut self,
        document: &Document,
        frame: u32,
        shrink: u32,
        paint: &PaintLayer,
        surface: &mut TiledSurface,
    ) -> Timings {
        let steps = layer_steps(document, paint);
        let (span, reached) = reach(&steps, shrink, surface);
        let reused = surface.clear();
        if span[0] >= span[2] {
            return Timings::default();
        }
        let (origin, extent) = surface.area(span);
        let mut pixmap = match reused {
            Some(pixmap) if [pixmap.width(), pixmap.height()] == extent => pixmap,
            _ => Pixmap::new(extent[0], extent[1]),
        };
        let timings = self.draw_steps(&steps, frame, shrink, origin, &mut pixmap);
        surface.set(span, pixmap, reached);
        timings
    }

    /// Draws `steps` at 1/`shrink` of their size into `pixmap`, which covers
    /// the scaled document from `origin`.
    fn draw_steps(
        &mut self,
        steps: &[Step<'_>],
        frame: u32,
        shrink: u32,
        origin: [u32; 2],
        pixmap: &mut Pixmap,
    ) -> Timings {
        let [width, height] = [pixmap.width(), pixmap.height()];
        let started = std::time::Instant::now();
        let outlines = self.outlines(steps, frame);
        let outlined = started.elapsed();

        self.context.reset_and_resize(width, height);
        self.context.set_transform(
            Affine::translate((-f64::from(origin[0]), -f64::from(origin[1])))
                * Affine::scale(1.0 / f64::from(shrink)),
        );
        let mut outlines = outlines.into_iter();
        for step in steps {
            match step {
                Step::Push(opacity) => {
                    self.context
                        .push_layer(None, None, Some(*opacity), None, None);
                }
                Step::Pop => self.context.pop_layer(),
                Step::Draw { stroke, erase, .. } => {
                    if let Some(path) = outlines.next().flatten() {
                        self.draw(stroke, *erase, &path);
                    }
                }
            }
        }
        self.context.flush();
        let encoded = started.elapsed();
        self.context
            .render_with(pixmap, &mut self.resources, RasterizerSettings::default());
        Timings {
            outlines: outlined,
            encode: encoded - outlined,
            rasterize: started.elapsed() - encoded,
            composite: std::time::Duration::ZERO,
        }
    }

    /// The outline of each `Step::Draw`, in order, made on the worker
    /// threads in contiguous runs.
    fn outlines(&self, steps: &[Step<'_>], frame: u32) -> Vec<Option<BezPath>> {
        let draws: Vec<(&Stroke, &Pen)> = steps
            .iter()
            .filter_map(|step| match step {
                Step::Draw { stroke, pen, .. } => Some((*stroke, pen)),
                _ => None,
            })
            .collect();
        let make = |(stroke, pen): &(&Stroke, &Pen)| {
            let samples = Resampler::whole(&stroke.points, stroke::spacing(stroke.width));
            stroke::outline(&samples, pen, frame)
        };
        let run = draws.len().div_ceil(self.threads).max(1);
        if self.threads == 1 || draws.len() < 2 {
            return draws.iter().map(make).collect();
        }
        std::thread::scope(|scope| {
            let workers: Vec<_> = draws
                .chunks(run)
                .map(|chunk| scope.spawn(move || chunk.iter().map(make).collect::<Vec<_>>()))
                .collect();
            workers
                .into_iter()
                .flat_map(|worker| worker.join().expect("outline worker panicked"))
                .collect()
        })
    }

    fn draw(&mut self, stroke: &Stroke, erase: bool, path: &BezPath) {
        let [r, g, b, a] = stroke.color.0;
        let alpha = (f32::from(a) * stroke.brush.opacity.clamp(0.0, 1.0)).round() as u8;
        self.context
            .set_aliasing_threshold((!stroke.brush.antialias).then_some(128));
        if erase {
            let mode = BlendMode::new(Mix::Normal, Compose::DestOut);
            self.context.push_layer(None, Some(mode), None, None, None);
            self.context
                .set_paint(AlphaColor::from_rgba8(0, 0, 0, alpha));
            self.context.fill_path(path);
            self.context.pop_layer();
        } else {
            self.context
                .set_paint(AlphaColor::from_rgba8(r, g, b, alpha));
            self.context.fill_path(path);
        }
        self.context.set_aliasing_threshold(None);
    }
}

/// What drawing `paint` takes, in order.
fn layer_steps<'a>(document: &'a Document, paint: &PaintLayer) -> Vec<Step<'a>> {
    let mut steps = Vec::new();
    collect(
        &paint.ops,
        &document.store,
        document.wobble,
        paint.wobble.unwrap_or(document.wobble),
        &mut steps,
    );
    steps
}

/// The tiles of `surface` that `steps` drawn at 1/`shrink` reach, and the
/// rectangle of tiles around them (left, top, right, bottom; empty when
/// none).
fn reach(steps: &[Step<'_>], shrink: u32, surface: &TiledSurface) -> ([u32; 4], Vec<bool>) {
    let [columns, rows] = surface.grid();
    let edge = f64::from(surface.edge());
    let size = surface.size().map(f64::from);
    let mut reached = vec![false; (columns * rows) as usize];
    for step in steps {
        // Erasing only takes away, so it reaches no new tile.
        let Step::Draw {
            stroke,
            pen,
            erase: false,
        } = step
        else {
            continue;
        };
        // One more pixel each way for antialiasing.
        let [left, top, right, bottom] =
            stroke::bounds(&stroke.points, pen).map(|value| value / f64::from(shrink));
        let from = [(left - 1.0).max(0.0), (top - 1.0).max(0.0)];
        let to = [(right + 1.0).min(size[0]), (bottom + 1.0).min(size[1])];
        if from[0] >= to[0] || from[1] >= to[1] {
            continue;
        }
        let first = from.map(|value| (value / edge) as u32);
        let last = [
            ((to[0] / edge).ceil() as u32).min(columns),
            ((to[1] / edge).ceil() as u32).min(rows),
        ];
        for row in first[1]..last[1] {
            for column in first[0]..last[0] {
                reached[(row * columns + column) as usize] = true;
            }
        }
    }
    let mut span = [u32::MAX, u32::MAX, 0, 0];
    for (index, _) in reached.iter().enumerate().filter(|(_, reached)| **reached) {
        let [column, row] = [index as u32 % columns, index as u32 / columns];
        span = [
            span[0].min(column),
            span[1].min(row),
            span[2].max(column + 1),
            span[3].max(row + 1),
        ];
    }
    (span, reached)
}

/// Bytes the surfaces of `plan`'s layers take when drawn at 1/`shrink` on
/// tiles of `edge` pixels.
pub fn surface_estimate(document: &Document, plan: &RenderPlan, shrink: u32, edge: u32) -> usize {
    let empty = TiledSurface::new(scaled_size(document.canvas, shrink), edge);
    plan.layers
        .iter()
        .filter_map(
            |(id, _)| match document.layer(*id).map(|layer| &layer.kind) {
                Some(LayerKind::Paint(paint)) => Some(paint),
                _ => None,
            },
        )
        .map(|paint| {
            let (span, _) = reach(&layer_steps(document, paint), shrink, &empty);
            if span[0] >= span[2] {
                return 0;
            }
            let (_, extent) = empty.area(span);
            usize::from(extent[0]) * usize::from(extent[1]) * 4
        })
        .sum()
}

fn collect<'a>(
    ops: &[Op],
    store: &'a Store,
    document_wobble: Wobble,
    wobble: Wobble,
    steps: &mut Vec<Step<'a>>,
) {
    for op in ops {
        match op {
            Op::Paint { stroke, .. } | Op::Erase { stroke, .. } => {
                let Some(stroke) = store.strokes.get(stroke) else {
                    continue;
                };
                steps.push(Step::Draw {
                    stroke,
                    pen: Pen {
                        width: stroke.width,
                        brush: stroke.brush,
                        seed: stroke.seed,
                        wobble: f64::from(wobble.amount) * f64::from(stroke.brush.wobble_scale),
                    },
                    erase: matches!(op, Op::Erase { .. }),
                });
            }
            Op::Isolated(section) => {
                steps.push(Step::Push(section.opacity));
                collect(
                    &section.ops,
                    store,
                    document_wobble,
                    section.wobble.unwrap_or(document_wobble),
                    steps,
                );
                steps.push(Step::Pop);
            }
            // `check` refuses documents with anything else.
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use ugu_core::document::{Layer, LayerId};
    use ugu_core::ops::{Blend, Rgba8, Section, StrokeId, merge_down};
    use ugu_core::store::{Brush, Point};

    const RED: [u8; 4] = [220, 30, 30, 255];
    const BLUE: [u8; 4] = [30, 30, 220, 255];

    fn document() -> Document {
        let mut document = Document::new([96, 48]);
        document.background = Rgba8([0, 0, 0, 0]);
        document
    }

    /// A horizontal line from `from` to `to` at height `y`.
    fn line(
        document: &mut Document,
        from: f32,
        to: f32,
        y: f32,
        color: [u8; 4],
        antialias: bool,
    ) -> StrokeId {
        let id = StrokeId(document.store.strokes.len() as u32);
        let points: Vec<Point> = (0..=20)
            .map(|step| Point {
                x: from + (to - from) * step as f32 / 20.0,
                y,
                pressure: 1.0,
            })
            .collect();
        document.store.strokes.insert(
            id,
            Stroke {
                points: Arc::from(points),
                color: Rgba8(color),
                width: 8.0,
                brush: Brush {
                    engine: BrushEngine::Line,
                    opacity: 1.0,
                    hardness: 1.0,
                    antialias,
                    size_dynamics: 0.8,
                    wobble_scale: 1.0,
                },
                seed: 0x1234 + u64::from(id.0),
            },
        );
        id
    }

    fn paint_layer(document: &mut Document) -> &mut PaintLayer {
        match &mut document.layers[0].kind {
            LayerKind::Paint(paint) => paint,
            LayerKind::Group(_) => unreachable!(),
        }
    }

    fn render(document: &Document, frame: i64, threads: u16) -> Pixmap {
        let [width, height] = document.canvas.map(|edge| edge as u16);
        let mut pixmap = Pixmap::new(width, height);
        DocumentRenderer::new(threads).render(document, frame, Purpose::Display, &mut pixmap);
        pixmap
    }

    fn at(pixmap: &Pixmap, x: u16, y: u16) -> [u8; 4] {
        let index = (usize::from(y) * usize::from(pixmap.width()) + usize::from(x)) * 4;
        pixmap.data_as_u8_slice()[index..index + 4]
            .try_into()
            .unwrap()
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
    fn an_eraser_reaches_only_what_came_before_it() {
        let mut document = document();
        document.wobble = Wobble::classic(0.0);
        let before = line(&mut document, 8.0, 88.0, 24.0, RED, true);
        let eraser = line(&mut document, 8.0, 88.0, 24.0, [0, 0, 0, 255], true);
        let after = line(&mut document, 56.0, 88.0, 24.0, BLUE, true);
        paint_layer(&mut document).ops = vec![
            Op::Paint {
                stroke: before,
                clip: None,
            },
            Op::Erase {
                stroke: eraser,
                clip: None,
            },
            Op::Paint {
                stroke: after,
                clip: None,
            },
        ];
        let result = render(&document, 0, 0);
        assert_eq!(at(&result, 24, 24), [0, 0, 0, 0]);
        assert_eq!(at(&result, 72, 24), BLUE);
    }

    /// Two layers with their own opacity and motion, each with an eraser that
    /// overlaps the other layer's line.
    fn two_layers() -> (Document, PaintLayer, PaintLayer) {
        let mut document = document();
        let lower_line = line(&mut document, 8.0, 88.0, 20.0, RED, true);
        let lower_eraser = line(&mut document, 8.0, 30.0, 26.0, [0, 0, 0, 255], true);
        let upper_line = line(&mut document, 20.0, 80.0, 28.0, BLUE, true);
        let upper_eraser = line(&mut document, 50.0, 70.0, 22.0, [0, 0, 0, 255], true);
        let mut below = paint_layer(&mut document).clone();
        below.ops = vec![
            Op::Paint {
                stroke: lower_line,
                clip: None,
            },
            Op::Erase {
                stroke: lower_eraser,
                clip: None,
            },
        ];
        below.opacity = 0.6;
        let mut above = below.clone();
        above.ops = vec![
            Op::Paint {
                stroke: upper_line,
                clip: None,
            },
            Op::Erase {
                stroke: upper_eraser,
                clip: None,
            },
        ];
        above.opacity = 0.75;
        above.wobble = Some(Wobble::classic(4.0));
        (document, below, above)
    }

    fn with_layers(document: &Document, layers: &[PaintLayer]) -> Document {
        let mut document = document.clone();
        let template = document.layers[0].clone();
        document.layers = layers
            .iter()
            .enumerate()
            .map(|(index, paint)| {
                let mut layer = template.clone();
                layer.id = LayerId(index as u32 + 1);
                layer.kind = LayerKind::Paint(paint.clone());
                layer
            })
            .collect();
        document
    }

    #[test]
    fn merging_keeps_every_frame_and_each_eraser_in_its_own_layer() {
        let (document, below, above) = two_layers();
        let merged = merge_down(&below, &above).unwrap();
        let separate = with_layers(&document, &[below, above]);
        let joined = with_layers(&document, &[merged]);
        for frame in 0..4 {
            let difference =
                max_difference(&render(&separate, frame, 0), &render(&joined, frame, 0));
            // Opacity is applied once per surface either way; only the u8
            // rounding of nested surfaces may differ.
            assert!(difference <= 2, "frame {frame} differs by {difference}");
        }
        // The upper eraser crosses the lower red line without removing it.
        let red = at(&render(&joined, 0, 0), 60, 20);
        assert!(red[0] > 100 && red[3] > 100, "{red:?}");
    }

    #[test]
    fn an_eraser_added_after_a_merge_reaches_both_layers() {
        let (mut document, below, mut above) = two_layers();
        document.wobble = Wobble::classic(0.0);
        above.wobble = None;
        // Covers y 20..28: the lower line's lower half and the upper line's
        // upper half.
        let eraser = line(&mut document, 36.0, 44.0, 24.0, [0, 0, 0, 255], true);
        let mut merged = merge_down(&below, &above).unwrap();
        merged.ops.push(Op::Erase {
            stroke: eraser,
            clip: None,
        });
        let before = render(&with_layers(&document, &[below, above]), 0, 0);
        assert_ne!(at(&before, 40, 21), [0, 0, 0, 0]);
        assert_ne!(at(&before, 40, 26), [0, 0, 0, 0]);
        let result = render(&with_layers(&document, &[merged]), 0, 0);
        assert_eq!(at(&result, 40, 21), [0, 0, 0, 0]);
        assert_eq!(at(&result, 40, 26), [0, 0, 0, 0]);
        assert_ne!(at(&result, 40, 30), [0, 0, 0, 0]);
    }

    #[test]
    fn every_thread_count_draws_the_same_pixels() {
        let (document, below, above) = two_layers();
        let document = with_layers(&document, &[below, above]);
        let single = render(&document, 5, 0);
        for threads in [1, 4, 8] {
            assert!(
                render(&document, 5, threads).data_as_u8_slice() == single.data_as_u8_slice(),
                "{threads} threads differ"
            );
        }
    }

    #[test]
    fn strokes_move_from_frame_to_frame_and_frames_wrap() {
        let (document, below, above) = two_layers();
        let document = with_layers(&document, &[below, above]);
        let first = render(&document, 0, 0);
        assert!(max_difference(&first, &render(&document, 1, 0)) > 0);
        let wrapped = render(&document, i64::from(document.frames), 0);
        assert!(first.data_as_u8_slice() == wrapped.data_as_u8_slice());
    }

    #[test]
    fn a_pen_without_antialiasing_draws_whole_pixels() {
        let mut document = document();
        let stroke = line(&mut document, 8.0, 88.0, 24.3, RED, false);
        paint_layer(&mut document).ops = vec![Op::Paint { stroke, clip: None }];
        let result = render(&document, 2, 0);
        assert!(
            result
                .data_as_u8_slice()
                .chunks(4)
                .all(|pixel| pixel[3] == 0 || pixel[3] == 255)
        );
    }

    #[test]
    fn hidden_and_reference_layers() {
        let (document, below, above) = two_layers();
        let mut document = with_layers(&document, &[below, above]);
        document.layers[1].visible = false;
        let without_upper = render(&document, 0, 0);
        document.layers[1].visible = true;
        document.layers[1].reference = true;
        let [width, height] = document.canvas.map(|edge| edge as u16);
        let mut exported = Pixmap::new(width, height);
        DocumentRenderer::new(0).render(&document, 0, Purpose::Export, &mut exported);
        assert!(exported.data_as_u8_slice() == without_upper.data_as_u8_slice());
        assert!(max_difference(&exported, &render(&document, 0, 0)) > 0);
    }

    #[test]
    fn what_this_build_cannot_draw_is_named() {
        let mut document = document();
        assert_eq!(check(&document), Ok(()));
        paint_layer(&mut document).ops = vec![Op::Crop {
            offset: [0, 0],
            size: [96, 48],
        }];
        assert_eq!(check(&document), Err(Unsupported::CanvasChange));
        paint_layer(&mut document).ops = vec![Op::Isolated(Box::new(Section {
            ops: vec![Op::ClearSelection {
                mask: ugu_core::ops::MaskId(0),
            }],
            opacity: 1.0,
            wobble: None,
        }))];
        assert_eq!(check(&document), Err(Unsupported::Selection));
        let fill = Op::Fill {
            coverage: ugu_core::ops::MaskId(0),
            color: Rgba8([0, 0, 0, 255]),
            antialias: false,
            clip: None,
        };
        document.layers = vec![tree_group(
            10,
            Blend::Overlay,
            vec![tree_layer(1, vec![fill], |paint| {
                paint.clip_to_below = true
            })],
        )];
        assert_eq!(check(&document), Err(Unsupported::Fill));
        document.layers = vec![tree_group(
            10,
            Blend::Overlay,
            vec![tree_layer(1, vec![], |_| {})],
        )];
        assert_eq!(check(&document), Ok(()));
    }

    fn tree_layer(id: u32, ops: Vec<Op>, update: impl FnOnce(&mut PaintLayer)) -> Layer {
        let mut paint = PaintLayer {
            ops,
            opacity: 1.0,
            blend: Blend::Normal,
            clip_to_below: false,
            wobble: Some(Wobble::classic(0.0)),
            initial_size: [96, 48],
        };
        update(&mut paint);
        Layer {
            id: LayerId(id),
            name: String::new(),
            visible: true,
            reference: false,
            kind: LayerKind::Paint(paint),
        }
    }

    fn tree_group(id: u32, blend: Blend, children: Vec<Layer>) -> Layer {
        Layer {
            id: LayerId(id),
            name: String::new(),
            visible: true,
            reference: false,
            kind: LayerKind::Group(ugu_core::document::Group {
                opacity: 1.0,
                blend,
                clip_to_below: false,
                children,
            }),
        }
    }

    fn painting(stroke: StrokeId) -> Vec<Op> {
        vec![Op::Paint { stroke, clip: None }]
    }

    #[test]
    fn a_clipped_layer_shows_only_where_its_base_is() {
        let mut document = document();
        let base = line(&mut document, 8.0, 48.0, 24.0, RED, true);
        let over = line(&mut document, 8.0, 88.0, 24.0, BLUE, true);
        document.layers = vec![
            tree_layer(1, painting(base), |_| {}),
            tree_layer(2, painting(over), |paint| paint.clip_to_below = true),
        ];
        let result = render(&document, 0, 0);
        assert_eq!(at(&result, 24, 24), BLUE);
        assert_eq!(at(&result, 72, 24), [0, 0, 0, 0]);
    }

    #[test]
    fn a_group_keeps_its_blend_modes_inside() {
        let mut document = document();
        let red = line(&mut document, 8.0, 88.0, 24.0, RED, true);
        let green = line(&mut document, 8.0, 88.0, 24.0, [30, 220, 30, 255], true);
        let group = |blend| {
            tree_group(
                10,
                blend,
                vec![tree_layer(2, painting(green), |paint| {
                    paint.blend = Blend::Multiply
                })],
            )
        };
        document.layers = vec![tree_layer(1, painting(red), |_| {}), group(Blend::Normal)];
        // Multiplied with the empty group surface, the green stays green.
        assert_eq!(at(&render(&document, 0, 0), 48, 24), [30, 220, 30, 255]);
        document.layers[1] = group(Blend::Multiply);
        let multiplied = at(&render(&document, 0, 0), 48, 24);
        assert!(multiplied[0] < 40 && multiplied[1] < 40, "{multiplied:?}");
    }

    /// Ten layers with erasers, every blend mode, clipping and a group.
    fn many_layers() -> Document {
        let mut document = document();
        let mut layers = Vec::new();
        for index in 0..10u32 {
            let y = 6.0 + index as f32 * 4.0;
            let stroke = line(
                &mut document,
                4.0,
                92.0,
                y,
                [20 * index as u8, 90, 200, 220],
                true,
            );
            let eraser = line(&mut document, 40.0, 50.0, y, [0, 0, 0, 255], true);
            let blend = [
                Blend::Normal,
                Blend::Multiply,
                Blend::Screen,
                Blend::Overlay,
            ][index as usize % 4];
            let ops = vec![
                Op::Paint { stroke, clip: None },
                Op::Erase {
                    stroke: eraser,
                    clip: None,
                },
            ];
            layers.push(tree_layer(index + 1, ops, |paint| {
                paint.blend = blend;
                paint.opacity = 0.7;
                paint.clip_to_below = index % 3 == 2;
                paint.wobble = None;
            }));
        }
        let grouped = layers.split_off(6);
        layers.push(tree_group(20, Blend::Screen, grouped));
        document.layers = layers;
        document
    }

    #[test]
    fn many_layers_draw_the_same_on_any_thread_count() {
        let document = many_layers();
        let single = render(&document, 3, 0);
        for threads in [1, 4, 8, 16] {
            assert!(
                render(&document, 3, threads).data_as_u8_slice() == single.data_as_u8_slice(),
                "{threads} threads differ"
            );
        }
    }

    fn render_tiled(document: &Document, frame: i64, edge: u32, threads: u16) -> Pixmap {
        let [width, height] = document.canvas.map(|edge| edge as u16);
        let mut pixmap = Pixmap::new(width, height);
        let mut renderer = DocumentRenderer::new(threads);
        renderer.set_tile_edge(edge);
        renderer.render(document, frame, Purpose::Display, &mut pixmap);
        pixmap
    }

    #[test]
    fn tiles_keep_the_pixels_within_a_level() {
        let (base, below, above) = two_layers();
        let merged = merge_down(&below, &above).unwrap();
        let merged = with_layers(&base, &[below, above, merged]);
        for document in [merged, many_layers()] {
            for frame in [0, 3] {
                // One tile covers the whole canvas, as before tiles.
                let whole = render_tiled(&document, frame, 4096, 0);
                for edge in [8, 12, 32] {
                    let tiled = render_tiled(&document, frame, edge, 0);
                    // A layer is drawn from where its tiles start, and Vello
                    // rounds moved coordinates to f32 afresh, which can move
                    // an edge pixel's coverage by a level.
                    let most = max_difference(&tiled, &whole);
                    assert!(
                        most <= 1,
                        "{edge}-pixel tiles differ by {most} on frame {frame}"
                    );
                    let threaded = render_tiled(&document, frame, edge, 8);
                    assert!(
                        threaded.data_as_u8_slice() == tiled.data_as_u8_slice(),
                        "{edge}-pixel tiles differ on 8 threads"
                    );
                }
            }
        }
    }

    /// Renders with reuse and checks the result against a fresh render.
    fn cached(
        renderer: &mut DocumentRenderer,
        document: &Document,
        frame: i64,
        revisions: &LayerRevisions,
    ) -> usize {
        let plan = RenderPlan::new(document, Purpose::Display);
        let [width, height] = document.canvas.map(|edge| edge as u16);
        let mut pixmap = Pixmap::new(width, height);
        renderer.render_plan(document, &plan, frame, Some(revisions), &mut pixmap);
        assert!(
            pixmap.data_as_u8_slice() == render(document, frame, 0).data_as_u8_slice(),
            "a reused layer is out of date"
        );
        renderer.layers_drawn()
    }

    #[test]
    fn still_layers_are_drawn_once_for_every_frame() {
        let (document, below, mut above) = two_layers();
        above.wobble = Some(Wobble::classic(0.0));
        let document = with_layers(&document, &[below, above]);
        let revisions = LayerRevisions::default();
        let mut renderer = DocumentRenderer::new(0);
        let drawn: Vec<usize> = (0..4)
            .map(|frame| cached(&mut renderer, &document, frame, &revisions))
            .collect();
        assert_eq!(drawn, [2, 1, 1, 1]);
    }

    #[test]
    fn only_changed_layers_are_drawn_again() {
        use ugu_core::command;
        use ugu_core::history::History;

        let (document, below, above) = two_layers();
        let mut history = History::new(with_layers(&document, &[below, above]), false);
        let mut renderer = DocumentRenderer::new(0);
        let render_now = |renderer: &mut DocumentRenderer, history: &History| {
            cached(renderer, history.document(), 2, history.layer_revisions())
        };
        assert_eq!(render_now(&mut renderer, &history), 2);
        assert_eq!(render_now(&mut renderer, &history), 0);

        let stroke = document.store.strokes[&StrokeId(0)].clone();
        history
            .edit("Draw", |document| {
                command::draw(document, LayerId(2), stroke, false, None)
            })
            .unwrap();
        assert_eq!(render_now(&mut renderer, &history), 1);

        history
            .edit("Opacity", |document| {
                command::update_layer(document, LayerId(1), |layer| {
                    if let LayerKind::Paint(paint) = &mut layer.kind {
                        paint.opacity = 0.3;
                        paint.blend = Blend::Screen;
                    }
                })
                .unwrap()
            })
            .unwrap();
        assert_eq!(render_now(&mut renderer, &history), 0);

        history.undo().unwrap();
        history.undo().unwrap();
        assert_eq!(render_now(&mut renderer, &history), 1);
    }

    #[test]
    fn a_stopped_render_keeps_nothing_half_drawn() {
        let document = many_layers();
        let revisions = LayerRevisions::default();
        let plan = RenderPlan::new(&document, Purpose::Display);
        for threads in [0, 8] {
            let stop = Arc::new(AtomicBool::new(true));
            let mut renderer = DocumentRenderer::new(threads);
            renderer.set_stop(stop.clone());
            let mut pixmap = Pixmap::new(96, 48);
            assert!(!renderer.render_plan(&document, &plan, 1, Some(&revisions), &mut pixmap));
            assert_eq!(renderer.surface_bytes(), 0);
            stop.store(false, Ordering::Relaxed);
            assert_eq!(
                cached(&mut renderer, &document, 1, &revisions),
                plan.layers.len()
            );
        }
    }

    /// Averages `shrink` × `shrink` blocks of `pixmap`.
    fn box_down(pixmap: &Pixmap, shrink: u32) -> Vec<f32> {
        let [width, height] = [pixmap.width(), pixmap.height()].map(u32::from);
        let size = scaled_size([width, height], shrink);
        let data = pixmap.data_as_u8_slice();
        let mut out = vec![0.0; (size[0] * size[1] * 4) as usize];
        for y in 0..height {
            for x in 0..width {
                let to = (((y / shrink) * size[0] + x / shrink) * 4) as usize;
                let from = ((y * width + x) * 4) as usize;
                for channel in 0..4 {
                    out[to + channel] += f32::from(data[from + channel]) / (shrink * shrink) as f32;
                }
            }
        }
        out
    }

    #[test]
    fn a_smaller_render_is_the_full_render_made_smaller() {
        let (base, below, above) = two_layers();
        // Lines 8 pixels wide over each other every 4 pixels: at a quarter
        // size, where edges of several layers share a pixel, their coverage
        // is averaged before it is combined, which a smaller render cannot
        // avoid. A misplaced or missing layer would differ by far more.
        let cases = [
            (with_layers(&base, &[below, above]), 3.0),
            (many_layers(), 20.0),
        ];
        for (document, limit) in cases {
            let plan = RenderPlan::new(&document, Purpose::Display);
            let full = render(&document, 2, 0);
            for shrink in [2, 4] {
                let size = scaled_size(document.canvas, shrink).map(|edge| edge as u16);
                let mut small = Pixmap::new(size[0], size[1]);
                let mut renderer = DocumentRenderer::new(4);
                assert!(renderer.render_scaled(&document, &plan, 2, None, shrink, &mut small));
                let expected = box_down(&full, shrink);
                let mean = small
                    .data_as_u8_slice()
                    .iter()
                    .zip(&expected)
                    .map(|(a, b)| (f32::from(*a) - b).abs())
                    .sum::<f32>()
                    / expected.len() as f32;
                assert!(mean < limit, "1/{shrink} differs by {mean} on average");
            }
        }
    }

    #[test]
    fn a_frame_over_the_budget_is_the_same_frame() {
        let document = many_layers();
        let plan = RenderPlan::new(&document, Purpose::Display);
        for shrink in [1, 2] {
            let size = scaled_size(document.canvas, shrink).map(|edge| edge as u16);
            for frame in [0, 3] {
                let mut kept = Pixmap::new(size[0], size[1]);
                DocumentRenderer::new(0)
                    .render_scaled(&document, &plan, frame, None, shrink, &mut kept);
                for threads in [0, 8] {
                    let mut renderer = DocumentRenderer::new(threads);
                    renderer.set_surface_budget(0);
                    let mut streamed = Pixmap::new(size[0], size[1]);
                    assert!(renderer.render_scaled(
                        &document,
                        &plan,
                        frame,
                        None,
                        shrink,
                        &mut streamed
                    ));
                    assert!(
                        streamed.data_as_u8_slice() == kept.data_as_u8_slice(),
                        "1/{shrink} frame {frame} on {threads} threads"
                    );
                    assert_eq!(renderer.surface_bytes(), 0);
                }
            }
        }
    }
}
