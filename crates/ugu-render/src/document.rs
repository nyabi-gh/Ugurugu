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
use ugu_core::ops::{MaskId, Op, PaintLayer, Wobble};
use ugu_core::store::{BrushEngine, Mask, Store, Stroke};
use vello_cpu::color::AlphaColor;
use vello_cpu::kurbo::{Affine, BezPath};
use vello_cpu::peniko::{BlendMode, Compose, Mix};
use vello_cpu::{Pixmap, RasterizerSettings, RenderContext, RenderSettings, Resources};

use crate::compose::premultiplied;
use crate::composite::{self, Source};
use crate::mask::Runs;
use crate::plan::RenderPlan;
use crate::raster::document_level;
use crate::stroke::{self, Pen, Resampler};
use crate::tile::TiledSurface;

/// Content this build cannot draw yet, and the milestone that adds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unsupported {
    /// Images and moved selections (M4).
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
            Self::Image => "images",
            Self::Selection => "moved selections",
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
            Op::Paint { stroke, .. } | Op::Erase { stroke, .. } => {
                if store
                    .strokes
                    .get(stroke)
                    .is_some_and(|stroke| stroke.brush.engine != BrushEngine::Line)
                {
                    return Err(Unsupported::Brush);
                }
            }
            Op::Fill { .. } | Op::ClearSelection { .. } => {}
            Op::PlaceImage { .. } => return Err(Unsupported::Image),
            Op::TransformSelection { .. } => return Err(Unsupported::Selection),
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
    /// What `main` was made with; 0 draws on the calling thread.
    main_threads: u16,
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
    /// Stroke sample spacing in sixteenths of the full detail; see
    /// `set_detail`.
    detail: u32,
    timings: Timings,
    masks: MaskCache,
}

/// Full detail for `DocumentRenderer::set_detail`.
pub const FULL_DETAIL: u32 = 16;

/// A layer's own pixels and what they were drawn from.
struct Cached {
    /// `None` when drawn without revisions, so never reused.
    revision: Option<u64>,
    /// The frame within the cycle, or `None` for a layer that does not move.
    frame: Option<u32>,
    detail: u32,
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

/// What every layer of one frame is drawn with.
#[derive(Clone, Copy)]
struct Frame<'a> {
    document: &'a Document,
    frame: u32,
    shrink: u32,
    detail: u32,
    masks: &'a MaskCache,
}

/// A Vello context and the threads that make stroke outlines for it.
struct Raster {
    context: RenderContext,
    resources: Resources,
    threads: usize,
    /// The outlines of the last layer drawn, one per stroke, refilled for
    /// the next so that their memory is not given back and taken again.
    paths: Vec<BezPath>,
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
        /// The selection the stroke was drawn in.
        clip: Option<Arc<Ready>>,
    },
    /// Puts `color` in place of what is in the area, and when the fill is
    /// antialiased, behind what is in the fringe (2.2.13 `applyFillStroke`).
    Fill {
        color: [u8; 4],
        ready: Arc<Ready>,
    },
    /// Removes what is in the area.
    Clear {
        ready: Arc<Ready>,
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
            main_threads: threads,
            threads: usize::from(threads.max(1)),
            cache: HashMap::new(),
            drawn: 0,
            stop: None,
            surface_budget: SURFACE_BUDGET,
            tile_edge: TILE_EDGE,
            detail: FULL_DETAIL,
            timings: Timings::default(),
            masks: MaskCache::default(),
        }
    }

    /// Spaces stroke samples `sixteenths`/16 times as far apart as at full
    /// detail ([`FULL_DETAIL`]). The strokes keep their motion, which follows
    /// the distance along them, and are outlined from fewer samples: for
    /// playback frames shown smaller than the canvas, where the samples would
    /// otherwise be closer together on screen than when editing at 100%.
    /// Editing and export keep full detail.
    pub fn set_detail(&mut self, sixteenths: u32) {
        self.detail = sixteenths.max(FULL_DETAIL);
    }

    /// Lets go of Vello's working memory, which keeps the size of the
    /// largest layer drawn; the next render allocates it again.
    pub fn release_scratch(&mut self) {
        self.main = Raster::new(self.level, self.main_threads);
        self.singles.clear();
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

    pub fn is_stopped(&self) -> bool {
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
        self.timings += self.main.draw_layer(
            &Frame {
                document,
                frame,
                shrink,
                detail: self.detail,
                masks: &self.masks,
            },
            paint,
            &mut surface,
        );
        surface
    }

    /// Paint layer `id`'s own pixels on frame 0, drawn smaller by a whole
    /// factor so that they fit in `fit`; `None` for a group or a missing
    /// layer.
    pub fn thumbnail(&mut self, document: &Document, id: LayerId, fit: [u32; 2]) -> Option<Pixmap> {
        let Some(LayerKind::Paint(paint)) = document.layer(id).map(|layer| &layer.kind) else {
            return None;
        };
        let shrink = (0..2)
            .map(|axis| document.canvas[axis].div_ceil(fit[axis].max(1)))
            .max()
            .unwrap_or(1)
            .max(1);
        Some(self.draw_alone(document, 0, shrink, paint).to_pixmap())
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
        self.masks.next_render();
        let edge = self.tile_edge;
        if self.over_budget(document, plan, shrink) {
            self.cache.clear();
            self.drawn = plan.layers.len();
            self.timings = Timings::default();
            return self.render_streamed(document, plan, frame, shrink, pixmap);
        }
        let detail = self.detail;
        self.cache.retain(|id, cached| {
            plan.moves(*id).is_some()
                && cached.surface.size() == size
                && cached.surface.edge() == edge
                && cached.detail == detail
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
                self.timings += self.main.draw_layer(
                    &Frame {
                        document,
                        frame,
                        shrink,
                        detail,
                        masks: &self.masks,
                    },
                    each.paint,
                    &mut each.surface,
                );
                each.drawn = true;
            }
        }
        let complete = work.iter().all(|each| each.drawn);
        for each in work.into_iter().filter(|each| each.drawn) {
            let cached = Cached {
                revision: each.revision,
                frame: each.frame,
                detail,
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
        let detail = self.detail;
        let masks = &self.masks;
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
                                &Frame {
                                    document,
                                    frame,
                                    shrink,
                                    detail,
                                    masks,
                                },
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
            paths: Vec::new(),
        }
    }

    /// Draws a paint layer's own pixels at 1/`shrink` of their size into the
    /// tiles its strokes reach.
    fn draw_layer(
        &mut self,
        at: &Frame<'_>,
        paint: &PaintLayer,
        surface: &mut TiledSurface,
    ) -> Timings {
        let Frame {
            document,
            frame,
            shrink,
            detail,
            masks,
        } = *at;
        let steps = layer_steps(document, paint, masks);
        let (span, reached) = reach(step_bounds(&steps), shrink, surface);
        let reused = surface.clear();
        if span[0] >= span[2] {
            return Timings::default();
        }
        let (origin, extent) = surface.area(span);
        let mut pixmap = match reused {
            Some(pixmap) if [pixmap.width(), pixmap.height()] == extent => pixmap,
            _ => Pixmap::new(extent[0], extent[1]),
        };
        let timings = self.draw_steps(&steps, frame, shrink, detail, origin, &mut pixmap);
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
        detail: u32,
        origin: [u32; 2],
        pixmap: &mut Pixmap,
    ) -> Timings {
        let [width, height] = [pixmap.width(), pixmap.height()];
        let started = std::time::Instant::now();
        let mut paths = std::mem::take(&mut self.paths);
        self.outlines(steps, frame, detail, &mut paths);
        let outlined = started.elapsed();

        self.context.reset_and_resize(width, height);
        self.context.set_transform(
            Affine::translate((-f64::from(origin[0]), -f64::from(origin[1])))
                * Affine::scale(1.0 / f64::from(shrink)),
        );
        let mut outlines = paths.iter();
        for step in steps {
            match step {
                Step::Push(opacity) => {
                    self.context
                        .push_layer(None, None, Some(*opacity), None, None);
                }
                Step::Pop => self.context.pop_layer(),
                Step::Draw {
                    stroke,
                    erase,
                    clip,
                    ..
                } => {
                    let path = outlines.next().expect("one outline per stroke");
                    if !path.is_empty() {
                        self.draw(stroke, *erase, path, clip.as_ref().map(|clip| &clip.area));
                    }
                }
                Step::Fill {
                    color: [r, g, b, a],
                    ready,
                } => {
                    let color = AlphaColor::from_rgba8(*r, *g, *b, *a);
                    self.take_away(&ready.area);
                    self.context.set_paint(color);
                    self.context.fill_path(&ready.area);
                    if let Some(fringe) = &ready.fringe {
                        let behind = BlendMode::new(Mix::Normal, Compose::DestOver);
                        self.context
                            .push_layer(None, Some(behind), None, None, None);
                        self.context.set_paint(color);
                        self.context.fill_path(fringe);
                        self.context.pop_layer();
                    }
                }
                Step::Clear { ready } => self.take_away(&ready.area),
            }
        }
        self.paths = paths;
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

    /// The outline of each `Step::Draw` into `paths`, in order (empty
    /// without samples), made on the worker threads in contiguous runs.
    fn outlines(&self, steps: &[Step<'_>], frame: u32, detail: u32, paths: &mut Vec<BezPath>) {
        let draws: Vec<(&Stroke, &Pen)> = steps
            .iter()
            .filter_map(|step| match step {
                Step::Draw { stroke, pen, .. } => Some((*stroke, pen)),
                _ => None,
            })
            .collect();
        paths.resize_with(draws.len(), BezPath::new);
        let make = |(stroke, pen): &(&Stroke, &Pen), path: &mut BezPath| {
            let spacing =
                stroke::spacing(stroke.width) * f64::from(detail) / f64::from(FULL_DETAIL);
            let samples = Resampler::whole(&stroke.points, spacing);
            stroke::outline_into(&samples, pen, frame, path);
        };
        let run = draws.len().div_ceil(self.threads).max(1);
        if self.threads == 1 || draws.len() < 2 {
            for (draw, path) in draws.iter().zip(paths.iter_mut()) {
                make(draw, path);
            }
            return;
        }
        std::thread::scope(|scope| {
            for (chunk, paths) in draws.chunks(run).zip(paths.chunks_mut(run)) {
                scope.spawn(move || {
                    for (draw, path) in chunk.iter().zip(paths) {
                        make(draw, path);
                    }
                });
            }
        });
    }

    /// Draws `stroke` along `path`, inside `clip` when given. A layer takes
    /// the aliasing setting its clip is pushed with, so the clip is pushed
    /// first and stays exact for aliased pens too.
    fn draw(&mut self, stroke: &Stroke, erase: bool, path: &BezPath, clip: Option<&BezPath>) {
        let [r, g, b, a] = stroke.color.0;
        let alpha = (f32::from(a) * stroke.brush.opacity.clamp(0.0, 1.0)).round() as u8;
        let layer = erase || clip.is_some();
        if layer {
            let mode = erase.then(|| BlendMode::new(Mix::Normal, Compose::DestOut));
            self.context.push_layer(clip, mode, None, None, None);
        }
        self.context
            .set_aliasing_threshold((!stroke.brush.antialias).then_some(128));
        let paint = if erase {
            [0, 0, 0, alpha]
        } else {
            [r, g, b, alpha]
        };
        self.context.set_paint(AlphaColor::from_rgba8(
            paint[0], paint[1], paint[2], paint[3],
        ));
        self.context.fill_path(path);
        self.context.set_aliasing_threshold(None);
        if layer {
            self.context.pop_layer();
        }
    }

    /// Removes what is under `area`.
    fn take_away(&mut self, area: &BezPath) {
        let mode = BlendMode::new(Mix::Normal, Compose::DestOut);
        self.context.push_layer(None, Some(mode), None, None, None);
        self.context.set_paint(AlphaColor::from_rgba8(0, 0, 0, 255));
        self.context.fill_path(area);
        self.context.pop_layer();
    }
}

/// What drawing `paint` takes, in order.
fn layer_steps<'a>(document: &'a Document, paint: &PaintLayer, masks: &MaskCache) -> Vec<Step<'a>> {
    let mut steps = Vec::new();
    collect(
        &paint.ops,
        &document.store,
        masks,
        document.wobble,
        paint.wobble.unwrap_or(document.wobble),
        &mut steps,
    );
    steps
}

fn pen(stroke: &Stroke, wobble: Wobble) -> Pen {
    Pen {
        width: stroke.width,
        brush: stroke.brush,
        seed: stroke.seed,
        wobble: f64::from(wobble.amount) * f64::from(stroke.brush.wobble_scale),
    }
}

/// Where the steps that add pixels reach, in document pixels. Erasing and
/// clearing only take away.
fn step_bounds<'s>(steps: &'s [Step<'_>]) -> impl Iterator<Item = [f64; 4]> + 's {
    steps.iter().filter_map(|step| match step {
        Step::Draw {
            stroke,
            pen,
            erase: false,
            ..
        } => Some(stroke::bounds(&stroke.points, pen)),
        Step::Fill { ready, .. } => ready.bounds.map(|bounds| bounds.map(f64::from)),
        _ => None,
    })
}

/// Where `ops` may add pixels, from stroke bounds and mask bounds alone,
/// without reading mask bits; a fill may cover less.
fn op_bounds(
    ops: &[Op],
    store: &Store,
    document_wobble: Wobble,
    wobble: Wobble,
    out: &mut Vec<[f64; 4]>,
) {
    for op in ops {
        match op {
            Op::Paint { stroke, .. } => {
                if let Some(stroke) = store.strokes.get(stroke) {
                    out.push(stroke::bounds(&stroke.points, &pen(stroke, wobble)));
                }
            }
            Op::Fill {
                coverage,
                antialias,
                clip,
                ..
            } => {
                let edges = |id: &MaskId| {
                    store.masks.get(id).map(|mask| {
                        let [left, top, width, height] = mask.bounds;
                        [left, top, left + width, top + height]
                    })
                };
                let Some(mut bounds) = edges(coverage) else {
                    continue;
                };
                if *antialias {
                    bounds = [bounds[0] - 1, bounds[1] - 1, bounds[2] + 1, bounds[3] + 1];
                }
                if let Some(cut) = clip.as_ref().and_then(edges) {
                    bounds = [
                        bounds[0].max(cut[0]),
                        bounds[1].max(cut[1]),
                        bounds[2].min(cut[2]),
                        bounds[3].min(cut[3]),
                    ];
                }
                if bounds[0] < bounds[2] && bounds[1] < bounds[3] {
                    out.push(bounds.map(f64::from));
                }
            }
            Op::Isolated(section) => op_bounds(
                &section.ops,
                store,
                document_wobble,
                section.wobble.unwrap_or(document_wobble),
                out,
            ),
            _ => {}
        }
    }
}

/// The tiles of `surface` that what lies in `bounds` (document pixels)
/// reaches when drawn at 1/`shrink`, and the rectangle of tiles around them
/// (left, top, right, bottom; empty when none).
fn reach(
    bounds: impl Iterator<Item = [f64; 4]>,
    shrink: u32,
    surface: &TiledSurface,
) -> ([u32; 4], Vec<bool>) {
    let [columns, rows] = surface.grid();
    let edge = f64::from(surface.edge());
    let size = surface.size().map(f64::from);
    let mut reached = vec![false; (columns * rows) as usize];
    for bounds in bounds {
        // One more pixel each way for antialiasing.
        let [left, top, right, bottom] = bounds.map(|value| value / f64::from(shrink));
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

/// Bytes the surfaces of `plan`'s layers take at most when drawn at
/// 1/`shrink` on tiles of `edge` pixels.
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
            let mut bounds = Vec::new();
            op_bounds(
                &paint.ops,
                &document.store,
                document.wobble,
                paint.wobble.unwrap_or(document.wobble),
                &mut bounds,
            );
            let (span, _) = reach(bounds.into_iter(), shrink, &empty);
            if span[0] >= span[2] {
                return 0;
            }
            let (_, extent) = empty.area(span);
            usize::from(extent[0]) * usize::from(extent[1]) * 4
        })
        .sum()
}

/// A mask, or a fill's coverage cut to its clip, ready to draw: its pixels
/// as a path and, for an antialiased fill, the pixels just outside it.
pub(crate) struct Ready {
    area: BezPath,
    fringe: Option<BezPath>,
    /// Left, top, right, bottom of both; `None` when they hold no pixel.
    bounds: Option<[i32; 4]>,
}

/// What a `Ready` was made from: the address and bounds of each mask's bits.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum MaskKey {
    Area(usize, [i32; 4]),
    Fill {
        coverage: (usize, [i32; 4]),
        clip: Option<(usize, [i32; 4])>,
        antialias: bool,
    },
}

struct Prepared {
    /// Keeps the bits alive, so their address names no other mask.
    _bits: Vec<Arc<[u8]>>,
    ready: Arc<Ready>,
    /// The render it was last used in.
    used: u64,
}

/// Masks made ready to draw, kept from render to render: reading a mask's
/// bits into a path costs milliseconds for a canvas-sized mask, and stored
/// masks never change.
#[derive(Default)]
pub(crate) struct MaskCache {
    prepared: std::sync::Mutex<HashMap<MaskKey, Prepared>>,
    render: u64,
}

fn mask_key(mask: &Mask) -> (usize, [i32; 4]) {
    (mask.bits.as_ptr() as usize, mask.bounds)
}

impl MaskCache {
    fn get(&self, key: MaskKey, bits: &[&Mask], make: impl FnOnce() -> Ready) -> Arc<Ready> {
        let lock = || self.prepared.lock().expect("no panic while holding it");
        if let Some(prepared) = lock().get_mut(&key) {
            prepared.used = self.render;
            return prepared.ready.clone();
        }
        // Made without the lock, so other layers are not held up.
        let ready = Arc::new(make());
        let prepared = Prepared {
            _bits: bits.iter().map(|mask| mask.bits.clone()).collect(),
            ready: ready.clone(),
            used: self.render,
        };
        lock().insert(key, prepared);
        ready
    }

    fn area(&self, mask: &Mask) -> Arc<Ready> {
        self.get(
            MaskKey::Area(mask_key(mask).0, mask.bounds),
            &[mask],
            || {
                let runs = Runs::from_mask(mask);
                Ready {
                    area: runs.path(),
                    fringe: None,
                    bounds: runs.bounds(),
                }
            },
        )
    }

    fn fill(&self, coverage: &Mask, clip: Option<&Mask>, antialias: bool) -> Arc<Ready> {
        let key = MaskKey::Fill {
            coverage: mask_key(coverage),
            clip: clip.map(mask_key),
            antialias,
        };
        let bits: Vec<&Mask> = std::iter::once(coverage).chain(clip).collect();
        self.get(key, &bits, || {
            let covered = Runs::from_mask(coverage);
            let clip = clip.map(Runs::from_mask);
            let cut = |runs: Runs| match &clip {
                Some(clip) => runs.intersect(clip),
                None => runs,
            };
            let fringe = antialias
                .then(|| cut(covered.fringe()))
                .filter(|fringe| !fringe.is_empty());
            let area = cut(covered);
            let bounds = [area.bounds(), fringe.as_ref().and_then(Runs::bounds)]
                .into_iter()
                .flatten()
                .reduce(|[l, t, r, b], [l2, t2, r2, b2]| {
                    [l.min(l2), t.min(t2), r.max(r2), b.max(b2)]
                });
            Ready {
                area: area.path(),
                fringe: fringe.map(|fringe| fringe.path()),
                bounds,
            }
        })
    }

    /// Starts a render, letting go of what the one before did not use.
    fn next_render(&mut self) {
        self.render += 1;
        let render = self.render;
        self.prepared
            .get_mut()
            .expect("no panic while holding it")
            .retain(|_, prepared| prepared.used + 1 >= render);
    }
}

fn collect<'a>(
    ops: &[Op],
    store: &'a Store,
    masks: &MaskCache,
    document_wobble: Wobble,
    wobble: Wobble,
    steps: &mut Vec<Step<'a>>,
) {
    for op in ops {
        match op {
            Op::Paint { stroke, clip } | Op::Erase { stroke, clip } => {
                let Some(stroke) = store.strokes.get(stroke) else {
                    continue;
                };
                // A missing mask cuts nothing away; validation keeps it from
                // happening.
                let clip = clip
                    .and_then(|id| store.masks.get(&id))
                    .map(|mask| masks.area(mask));
                steps.push(Step::Draw {
                    stroke,
                    pen: pen(stroke, wobble),
                    erase: matches!(op, Op::Erase { .. }),
                    clip,
                });
            }
            Op::Fill {
                coverage,
                color,
                antialias,
                clip,
            } => {
                let Some(coverage) = store.masks.get(coverage) else {
                    continue;
                };
                let clip = clip.and_then(|id| store.masks.get(&id));
                let ready = masks.fill(coverage, clip, *antialias);
                if ready.bounds.is_some() {
                    steps.push(Step::Fill {
                        color: color.0,
                        ready,
                    });
                }
            }
            Op::ClearSelection { mask } => {
                let Some(mask) = store.masks.get(mask) else {
                    continue;
                };
                let ready = masks.area(mask);
                if ready.bounds.is_some() {
                    steps.push(Step::Clear { ready });
                }
            }
            Op::Isolated(section) => {
                steps.push(Step::Push(section.opacity));
                collect(
                    &section.ops,
                    store,
                    masks,
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
        let mask = ugu_core::ops::MaskId(0);
        let fill = Op::Fill {
            coverage: mask,
            color: Rgba8([0, 0, 0, 255]),
            antialias: false,
            clip: Some(mask),
        };
        paint_layer(&mut document).ops = vec![
            fill.clone(),
            Op::ClearSelection { mask },
            Op::Isolated(Box::new(Section {
                ops: vec![Op::TransformSelection {
                    mask,
                    transform: ugu_core::ops::Affine::IDENTITY,
                    sampling: ugu_core::ops::Sampling::Nearest,
                    keep_source: false,
                }],
                opacity: 1.0,
                wobble: None,
            })),
        ];
        assert_eq!(check(&document), Err(Unsupported::Selection));
        let image = Op::PlaceImage {
            asset: ugu_core::ops::AssetId([0; 32]),
            transform: ugu_core::ops::Affine::IDENTITY,
            sampling: ugu_core::ops::Sampling::Smooth,
        };
        document.layers = vec![tree_group(
            10,
            Blend::Overlay,
            vec![tree_layer(1, vec![fill, image], |paint| {
                paint.clip_to_below = true
            })],
        )];
        assert_eq!(check(&document), Err(Unsupported::Image));
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

    /// Adds a mask of the pixels `inside` picks within `bounds`.
    fn add_mask(
        document: &mut Document,
        bounds: [i32; 4],
        inside: impl Fn(i32, i32) -> bool,
    ) -> ugu_core::ops::MaskId {
        let [left, top, width, height] = bounds;
        let row_bytes = ugu_core::store::Mask::row_bytes(width);
        let mut bits = vec![0u8; row_bytes * height as usize];
        for row in 0..height {
            for column in 0..width {
                if inside(left + column, top + row) {
                    bits[row as usize * row_bytes + column as usize / 8] |= 0x80 >> (column % 8);
                }
            }
        }
        let id = ugu_core::ops::MaskId(document.store.masks.len() as u32);
        let mask = ugu_core::store::Mask {
            bounds,
            bits: Arc::from(bits),
        };
        document.store.masks.insert(id, mask);
        id
    }

    /// A blob with a hole and a stray column, so that runs start and end
    /// inside strokes.
    fn blob(x: i32, y: i32) -> bool {
        let d = (x - 40) * (x - 40) + (y - 24) * (y - 24);
        (30..300).contains(&d) || x == 70
    }

    fn with_ops(document: &Document, ops: Vec<Op>) -> Document {
        let mut document = document.clone();
        paint_layer(&mut document).ops = ops;
        document
    }

    /// Each pixel of `cut` is `inside`'s where `mask` covers it and
    /// `outside`'s elsewhere, byte for byte.
    fn assert_cut(cut: &Pixmap, inside: &Pixmap, outside: &Pixmap, mask: &ugu_core::store::Mask) {
        for y in 0..cut.height() {
            for x in 0..cut.width() {
                let expected = if mask.contains(i32::from(x), i32::from(y)) {
                    at(inside, x, y)
                } else {
                    at(outside, x, y)
                };
                assert_eq!(at(cut, x, y), expected, "at {x}, {y}");
            }
        }
    }

    #[test]
    fn clipped_strokes_erasers_and_clears_change_only_the_masked_pixels() {
        for antialias in [true, false] {
            let mut document = document();
            let red = line(&mut document, 8.0, 88.0, 24.0, RED, antialias);
            let blue = line(&mut document, 20.0, 76.0, 22.0, BLUE, antialias);
            let mask = add_mask(&mut document, [10, 4, 70, 40], blob);
            let bits = document.store.masks[&mask].clone();
            let paint = |stroke, clip| Op::Paint { stroke, clip };
            let erase = |stroke, clip| Op::Erase { stroke, clip };
            let empty = render(&with_ops(&document, vec![]), 3, 0);
            let painted = render(&with_ops(&document, vec![paint(red, None)]), 3, 0);
            let clipped = render(&with_ops(&document, vec![paint(red, Some(mask))]), 3, 0);
            assert_cut(&clipped, &painted, &empty, &bits);
            let erased = render(
                &with_ops(&document, vec![paint(red, None), erase(blue, None)]),
                3,
                0,
            );
            let erased_inside = render(
                &with_ops(&document, vec![paint(red, None), erase(blue, Some(mask))]),
                3,
                0,
            );
            assert_cut(&erased_inside, &erased, &painted, &bits);
            let cleared = render(
                &with_ops(
                    &document,
                    vec![paint(red, None), Op::ClearSelection { mask }],
                ),
                3,
                0,
            );
            assert_cut(&cleared, &empty, &painted, &bits);
        }
    }

    #[test]
    fn a_fill_replaces_what_it_covers_and_goes_behind_its_edge() {
        let mut document = document();
        let red = line(&mut document, 8.0, 88.0, 24.0, [220, 30, 30, 120], true);
        let coverage = add_mask(&mut document, [10, 4, 70, 40], blob);
        let clip = add_mask(&mut document, [0, 0, 96, 30], |_, _| true);
        let half_green = Rgba8([0, 255, 0, 128]);
        let green = premultiplied(half_green.0);
        let under = render(
            &with_ops(
                &document,
                vec![Op::Paint {
                    stroke: red,
                    clip: None,
                }],
            ),
            0,
            0,
        );
        for clip in [None, Some(clip)] {
            let filled = render(
                &with_ops(
                    &document,
                    vec![
                        Op::Paint {
                            stroke: red,
                            clip: None,
                        },
                        Op::Fill {
                            coverage,
                            color: half_green,
                            antialias: true,
                            clip,
                        },
                    ],
                ),
                0,
                0,
            );
            let mask = &document.store.masks[&coverage];
            let cut = |y: i32| clip.is_none_or(|_| y < 30);
            for y in 0..filled.height() {
                for x in 0..filled.width() {
                    let (cx, cy) = (i32::from(x), i32::from(y));
                    let below = at(&under, x, y);
                    let got = at(&filled, x, y);
                    let next = [(-1, 0), (1, 0), (0, -1), (0, 1)]
                        .iter()
                        .any(|(dx, dy)| mask.contains(cx + dx, cy + dy));
                    if mask.contains(cx, cy) && cut(cy) {
                        assert_eq!(got, green, "covered at {x}, {y}");
                    } else if next && cut(cy) {
                        let keep = 255 - u16::from(below[3]);
                        let expected: [u8; 4] = std::array::from_fn(|c| {
                            below[c] + ((u16::from(green[c]) * keep + 127) / 255) as u8
                        });
                        let off = (0..4).map(|c| got[c].abs_diff(expected[c])).max().unwrap();
                        assert!(off <= 1, "edge at {x}, {y}: {got:?} against {expected:?}");
                    } else {
                        assert_eq!(got, below, "outside at {x}, {y}");
                    }
                }
            }
        }
    }

    /// A clipped stroke, a clipped fill, a clear and a stroke over them.
    fn masked_work() -> (Document, Op) {
        let mut document = document();
        let red = line(&mut document, 8.0, 88.0, 24.0, RED, true);
        let coverage = add_mask(&mut document, [10, 4, 70, 40], blob);
        let clip = add_mask(&mut document, [0, 0, 60, 48], |x, y| (x + y) % 9 != 0);
        let fill = Op::Fill {
            coverage,
            color: Rgba8([0, 160, 90, 200]),
            antialias: true,
            clip: Some(clip),
        };
        let mixed = with_ops(
            &document,
            vec![
                Op::Paint {
                    stroke: red,
                    clip: Some(clip),
                },
                fill.clone(),
                Op::ClearSelection { mask: clip },
                Op::Paint {
                    stroke: red,
                    clip: None,
                },
            ],
        );
        (mixed, fill)
    }

    #[test]
    fn a_fill_stays_while_strokes_move_and_draws_alike_everywhere() {
        let (mixed, fill) = masked_work();
        let alone = with_ops(&mixed, vec![fill]);
        assert!(render(&alone, 0, 0).data_as_u8_slice() == render(&alone, 1, 0).data_as_u8_slice());
        for frame in [0, 5] {
            let single = render(&mixed, frame, 0);
            assert!(render(&mixed, frame, 8).data_as_u8_slice() == single.data_as_u8_slice());
            for edge in [16, 64, 4096] {
                assert!(max_difference(&render_tiled(&mixed, frame, edge, 8), &single) <= 1);
            }
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
            (masked_work().0, 3.0),
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
    fn fewer_samples_keep_a_smaller_frame_within_the_same_limits() {
        let (base, below, above) = two_layers();
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
                // Playback asks for less than this: under 2 × `shrink`.
                renderer.set_detail(FULL_DETAIL * 2 * shrink);
                assert!(renderer.render_scaled(&document, &plan, 2, None, shrink, &mut small));
                let expected = box_down(&full, shrink);
                let mean = small
                    .data_as_u8_slice()
                    .iter()
                    .zip(&expected)
                    .map(|(a, b)| (f32::from(*a) - b).abs())
                    .sum::<f32>()
                    / expected.len() as f32;
                assert!(
                    mean < limit,
                    "1/{shrink} with fewer samples differs by {mean} on average"
                );
            }
        }
    }

    #[test]
    fn layers_drawn_with_fewer_samples_are_not_reused_at_full_detail() {
        let (document, below, above) = two_layers();
        let document = with_layers(&document, &[below, above]);
        let revisions = LayerRevisions::default();
        let mut renderer = DocumentRenderer::new(0);
        let plan = RenderPlan::new(&document, Purpose::Display);
        let [width, height] = document.canvas.map(|edge| edge as u16);
        let mut pixmap = Pixmap::new(width, height);
        renderer.set_detail(FULL_DETAIL * 2);
        renderer.render_plan(&document, &plan, 1, Some(&revisions), &mut pixmap);
        renderer.set_detail(FULL_DETAIL);
        assert_eq!(cached(&mut renderer, &document, 1, &revisions), 2);
    }

    #[test]
    fn a_thumbnail_fits_and_shows_the_layer_alone() {
        let document = many_layers();
        let mut renderer = DocumentRenderer::new(0);
        let mut seen = 0;
        for layer in &document.layers {
            let Some(thumbnail) = renderer.thumbnail(&document, layer.id, [48, 32]) else {
                assert!(matches!(layer.kind, LayerKind::Group(_)));
                continue;
            };
            assert!(thumbnail.width() <= 48 && thumbnail.height() <= 32);
            let shrink = document.canvas[0]
                .div_ceil(48)
                .max(document.canvas[1].div_ceil(32));
            assert_eq!(
                [thumbnail.width(), thumbnail.height()].map(u32::from),
                scaled_size(document.canvas, shrink)
            );
            seen += usize::from(
                thumbnail
                    .data_as_u8_slice()
                    .chunks(4)
                    .any(|pixel| pixel[3] > 0),
            );
        }
        assert!(seen > 0);
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
