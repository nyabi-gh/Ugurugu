// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Draws one frame of a document with Vello CPU.
//!
//! Each paint layer and isolated section is a Vello layer: its operations
//! draw on a transparent surface in order, so an eraser removes only what is
//! below it in the same surface, and the surface is then drawn over what is
//! beneath with its opacity. This is the meaning `ugu_core::semantics` pins.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ugu_core::document::{Document, LayerId, LayerKind};
use ugu_core::history::LayerRevisions;
use ugu_core::motion::frame_in_cycle;
use ugu_core::ops::{self, AssetId, MaskId, Op, PaintLayer, Sampling, Wobble};
use ugu_core::store::{BrushEngine, Mask, Stroke};
use vello_cpu::color::AlphaColor;
use vello_cpu::kurbo::{Affine, BezPath, Point, Rect, Shape, Vec2};
use vello_cpu::peniko::{BlendMode, Compose, ImageQuality, ImageSampler, Mix};
use vello_cpu::{Image, ImageSource, Pixmap, RasterizerSettings, RenderContext, Resources};

use crate::compose::premultiplied;
use crate::composite::{self, Source};
use crate::dab::{self, Dab};
use crate::mask::Runs;
use crate::plan::RenderPlan;
use crate::raster::document_level;
use crate::stroke::{self, Pen, Resampler};
use crate::tile::TiledSurface;

/// Which layers a frame shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// Everything visible, reference layers included.
    Display,
    /// Visible layers except reference layers.
    Export,
    /// What a tool that reads the frame sees (`RenderPlan::reference`),
    /// over transparency instead of the background.
    Reference,
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
    masks: DrawCache,
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
    masks: &'a DrawCache,
}

/// A Vello context and the threads that make stroke outlines for it.
struct Raster {
    context: RenderContext,
    resources: Resources,
    threads: usize,
    /// The outlines of the last layer drawn, one per stroke, refilled for
    /// the next so that their memory is not given back and taken again.
    paths: Vec<BezPath>,
    /// Likewise the dabs of airbrush and spray strokes, one list per stroke.
    dabs: Vec<Vec<Dab>>,
    /// One for each thread.
    painters: Vec<dab::Painter>,
    /// Dab strokes' dabs in target pixels.
    placed: Vec<Vec<Dab>>,
    /// The buffers dab strokes are painted into, the first `used` of them
    /// handed to Vello for the run being drawn; the rest are kept for later
    /// runs.
    buffers: Vec<Arc<Pixmap>>,
    used: usize,
    /// Dab strokes painted ahead of drawing, by their place among the
    /// layer's strokes; `None` when one paints nothing on the target.
    ready: std::collections::VecDeque<(usize, Option<Painted>)>,
    /// Bytes of `buffers` the run holds, and how many it may hold before
    /// the run is drawn and the next goes on over it.
    held: usize,
    dab_budget: usize,
}

/// Bytes of dab stroke buffers a run may hold; Vello keeps them all until
/// it draws the run.
const DAB_BUDGET: usize = 256 * 1024 * 1024;

/// A dab stroke painted into a buffer whose top left is at `origin` on the
/// target.
struct Painted {
    origin: [u16; 2],
    buffer: Arc<Pixmap>,
}

/// Bytes the layers' own surfaces may take until `set_surface_budget`; the
/// app sets it from the memory of the PC.
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
    /// Draws `image` placed by `transform`, in document pixels.
    Image {
        image: Arc<Pixmap>,
        transform: Affine,
        quality: ImageQuality,
    },
    /// Cuts what is drawn so far within the mask, clears it unless
    /// `keep_source`, and draws it moved by `transform` on top. It needs the
    /// pixels drawn so far, so drawing stops and starts again here.
    Move {
        ready: Arc<Ready>,
        transform: Affine,
        quality: ImageQuality,
        keep_source: bool,
    },
    /// An isolated section that moves a selection, drawn on its own and
    /// then put over what is there at `opacity`.
    Section {
        steps: Vec<Step<'a>>,
        opacity: f32,
    },
    /// Carries what is drawn so far to the next canvas, where the steps after
    /// it draw. Drawing stops and starts again at a resample.
    Canvas(Reframe),
}

/// A crop or resample.
#[derive(Clone, Copy, Debug)]
struct Reframe {
    /// The canvas before and after.
    from: [u32; 2],
    to: [u32; 2],
    /// From the canvas before to the canvas after, in document pixels.
    map: Affine,
    quality: ImageQuality,
    resample: bool,
}

impl Reframe {
    fn of(op: &Op, from: [u32; 2]) -> Option<Self> {
        let (map, quality, resample, to) = match *op {
            Op::Crop { offset, size } => (
                Affine::translate((f64::from(offset[0]), f64::from(offset[1]))),
                ImageQuality::Low,
                false,
                size,
            ),
            Op::Resample { size, sampling } => (
                Affine::scale_non_uniform(
                    f64::from(size[0]) / f64::from(from[0]),
                    f64::from(size[1]) / f64::from(from[1]),
                ),
                match sampling {
                    Sampling::Nearest => ImageQuality::Low,
                    Sampling::Smooth => ImageQuality::Medium,
                },
                true,
                size,
            ),
            _ => return None,
        };
        Some(Self {
            from,
            to,
            map,
            quality,
            resample,
        })
    }

    /// Where what lies in `bounds` on the canvas before is on the canvas
    /// after; `None` when it is outside the canvas before.
    fn carry(&self, [left, top, right, bottom]: [f64; 4]) -> Option<[f64; 4]> {
        let [width, height] = self.from.map(f64::from);
        let mut kept = [
            left.max(0.0),
            top.max(0.0),
            right.min(width),
            bottom.min(height),
        ];
        if kept[0] >= kept[2] || kept[1] >= kept[3] {
            return None;
        }
        if self.resample {
            // Smooth sampling spreads a pixel into its neighbours.
            kept = [kept[0] - 1.0, kept[1] - 1.0, kept[2] + 1.0, kept[3] + 1.0];
        }
        Some(moved_bounds(kept, self.map))
    }
}

/// How the crops before a resample, or before the end, are drawn: without
/// stopping, each part moved by the crops after it onto the last canvas.
struct Crops {
    /// From the canvas of each part, before each crop and after the last,
    /// to the last canvas.
    shifts: Vec<Affine>,
    /// The canvas before each crop on the last canvas, where it cuts away
    /// some of the last canvas; outside it nothing drawn before is kept.
    clips: Vec<Option<BezPath>>,
}

/// The crops of each canvas `steps` resample to, the first canvas first.
fn crops(steps: &[Step<'_>]) -> Vec<Crops> {
    let mut canvases = vec![Vec::new()];
    for step in steps {
        match step {
            Step::Canvas(reframe) if reframe.resample => canvases.push(Vec::new()),
            Step::Canvas(reframe) => canvases.last_mut().expect("one at least").push(*reframe),
            _ => {}
        }
    }
    canvases
        .into_iter()
        .map(|reframes| {
            let mut shifts = vec![Affine::IDENTITY; reframes.len() + 1];
            let mut clips = vec![None; reframes.len()];
            let Some(last) = reframes.last() else {
                return Crops { shifts, clips };
            };
            let size = last.to.map(f64::from);
            let mut moved = Vec2::ZERO;
            for (index, reframe) in reframes.iter().enumerate().rev() {
                moved += reframe.map.translation();
                shifts[index] = Affine::translate(moved);
                let from = reframe.from.map(f64::from);
                let kept = Rect::new(moved.x, moved.y, moved.x + from[0], moved.y + from[1]);
                let covers =
                    kept.x0 <= 0.0 && kept.y0 <= 0.0 && kept.x1 >= size[0] && kept.y1 >= size[1];
                clips[index] = (!covers).then(|| kept.to_path(0.1));
            }
            Crops { shifts, clips }
        })
        .collect()
}

/// Bounds in `out` carried over `reframe`.
fn carry_all(out: &mut Vec<[f64; 4]>, reframe: &Reframe) {
    *out = std::mem::take(out)
        .into_iter()
        .filter_map(|bounds| reframe.carry(bounds))
        .collect();
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
            masks: DrawCache {
                spare_count: usize::from(threads.max(1)),
                ..DrawCache::default()
            },
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
        self.masks
            .spare
            .get_mut()
            .expect("no panic while holding it")
            .clear();
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
        let background = if plan.purpose == Purpose::Reference {
            [0; 4]
        } else {
            premultiplied(document.background.0)
        };
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
            context: crate::raster::context(level, threads),
            resources: Resources::new(),
            threads: usize::from(threads.max(1)),
            paths: Vec::new(),
            dabs: Vec::new(),
            painters: Vec::new(),
            placed: Vec::new(),
            buffers: Vec::new(),
            used: 0,
            ready: std::collections::VecDeque::new(),
            held: 0,
            dab_budget: DAB_BUDGET,
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
            shrink,
            masks,
            ..
        } = *at;
        let steps = layer_steps(document, paint, masks);
        let mut bounds = Vec::new();
        step_bounds(&steps, &mut bounds);
        let (span, reached) = reach(bounds.into_iter(), shrink, surface);
        let reused = surface.clear();
        if span[0] >= span[2] {
            return Timings::default();
        }
        let (origin, extent) = surface.area(span);
        let mut pixmap = match reused {
            Some(pixmap) if [pixmap.width(), pixmap.height()] == extent => pixmap,
            _ => Pixmap::new(extent[0], extent[1]),
        };
        let timings = self.draw_steps(&steps, at, origin, Affine::IDENTITY, &mut pixmap);
        surface.set(span, pixmap, reached);
        timings
    }

    /// Draws `steps` at 1/`shrink` of their size into `pixmap`, which covers
    /// the scaled last canvas from `origin`; `shift` takes them to it. Steps
    /// before a resample draw on the whole of their own canvas.
    fn draw_steps(
        &mut self,
        steps: &[Step<'_>],
        at: &Frame<'_>,
        origin: [u32; 2],
        shift: Affine,
        pixmap: &mut Pixmap,
    ) -> Timings {
        let Frame {
            frame,
            shrink,
            detail,
            masks: cache,
            ..
        } = *at;
        let last = Area {
            origin,
            size: [pixmap.width(), pixmap.height()],
        };
        let areas: Vec<Area> = steps
            .iter()
            .filter_map(|step| match step {
                Step::Canvas(reframe) if reframe.resample => Some(Area {
                    origin: [0, 0],
                    size: scaled_size(reframe.from, shrink).map(|edge| edge as u16),
                }),
                _ => None,
            })
            .chain([last])
            .collect();
        let crops = crops(steps);
        let mut timings = Timings::default();
        // Sections that move selections are drawn first, each on its own.
        let mut sections = Vec::new();
        let (mut canvas, mut part) = (0, 0);
        for step in steps {
            match step {
                Step::Section { steps, .. } => {
                    let area = areas[canvas];
                    let mut section = Pixmap::new(area.size[0], area.size[1]);
                    let moved = shift * crops[canvas].shifts[part];
                    timings += self.draw_steps(steps, at, area.origin, moved, &mut section);
                    sections.push(Arc::new(section));
                }
                Step::Canvas(reframe) if reframe.resample => (canvas, part) = (canvas + 1, 0),
                Step::Canvas(_) => part += 1,
                _ => {}
            }
        }
        let mut sections = sections.into_iter();

        let started = std::time::Instant::now();
        let mut paths = std::mem::take(&mut self.paths);
        let mut dabs = std::mem::take(&mut self.dabs);
        self.outlines(steps, frame, detail, &mut paths, &mut dabs);
        timings.outlines += started.elapsed();
        let mut next_draw = 0;

        // A run after a moved selection or a resample draws over what the
        // run before drew, so buffers take turns instead of copying: one
        // drawn into, the other drawn from. Those left over are kept for the
        // next layers.
        let mut free = vec![std::mem::replace(pixmap, Pixmap::new(1, 1))];
        let (mut canvas, mut part) = (0, 0);
        let mut target = take_buffer(&mut free, cache, areas[0].size);
        // What is drawn before the step that starts each run, and what the
        // run before that drew from.
        let mut so_far: Option<Arc<Pixmap>> = None;
        let mut drawn_from: Option<Arc<Pixmap>> = None;
        // The resample that carries `so_far` to this run's canvas.
        let mut carried: Option<Reframe> = None;
        let mut rest = steps;
        loop {
            let area = areas[canvas];
            let [width, height] = area.size;
            let base = area.base(shrink);
            let started = std::time::Instant::now();
            // Vello lets go of the previous scene's images here.
            self.context.reset_and_resize(width, height);
            (self.used, self.held) = (0, 0);
            debug_assert!(self.ready.is_empty());
            if let Some(used) = drawn_from.take() {
                free.extend(Arc::try_unwrap(used).ok());
            }
            // What is drawn on a canvas a later crop makes smaller and a
            // crop after that larger again stays cut away.
            let cuts = &crops[canvas];
            self.context.set_transform(base);
            for clip in cuts.clips[part..].iter().rev().flatten() {
                self.context.push_layer(Some(clip), None, None, None, None);
            }
            // A selection moved right after a resample moves what was
            // carried, which needs a run of its own first.
            let fresh = carried.is_some();
            if let Some(so_far) = &so_far {
                match carried.take() {
                    Some(reframe) => {
                        let before = areas[canvas - 1].base(shrink);
                        self.context
                            .set_transform(base * reframe.map * before.inverse());
                        self.put_image(so_far, reframe.quality);
                    }
                    None => {
                        self.context.set_transform(Affine::IDENTITY);
                        self.put_image(so_far, ImageQuality::Low);
                    }
                }
            }
            carried = None;
            let mut here = base * shift * cuts.shifts[part];
            self.context.set_transform(here);
            let mut taken = 0;
            let mut depth = 0;
            for step in rest {
                if (taken > 0 || fresh) && matches!(step, Step::Move { .. }) {
                    break;
                }
                if taken > 0
                    && depth == 0
                    && matches!(step, Step::Draw { .. })
                    && !self.is_ready(next_draw)
                    && self.held + dab_bytes(&dabs[next_draw], here, area.size) > self.dab_budget
                {
                    break;
                }
                taken += 1;
                match step {
                    Step::Push(opacity) => {
                        depth += 1;
                        self.context
                            .push_layer(None, None, Some(*opacity), None, None);
                    }
                    Step::Pop => {
                        depth -= 1;
                        self.context.pop_layer();
                    }
                    Step::Draw {
                        stroke,
                        erase,
                        clip,
                        ..
                    } => {
                        let index = next_draw;
                        next_draw += 1;
                        let clip = clip.as_ref().map(|clip| &clip.area);
                        if !paths[index].is_empty() {
                            self.draw(stroke, *erase, &paths[index], clip);
                        } else if !dabs[index].is_empty() {
                            if !self.is_ready(index) {
                                let ahead = &rest[taken - 1..];
                                self.paint_ahead(ahead, index, &dabs, here, area.size);
                            }
                            let (_, painted) = self.ready.pop_front().expect("painted above");
                            if let Some(painted) = painted {
                                self.lay_dabs(&painted, *erase, clip, here);
                            }
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
                    Step::Image {
                        image,
                        transform,
                        quality,
                    } => {
                        self.context.set_transform(here * *transform);
                        self.put_image(image, *quality);
                        self.context.set_transform(here);
                    }
                    Step::Move {
                        ready,
                        transform,
                        quality,
                        keep_source,
                    } => {
                        // Nothing is drawn before it, so nothing moves.
                        let Some(source) = &so_far else {
                            continue;
                        };
                        if !keep_source {
                            self.take_away(&ready.area);
                        }
                        let moved = here * *transform;
                        self.context.set_transform(moved);
                        self.context
                            .push_layer(Some(&ready.area), None, None, None, None);
                        self.context.set_transform(moved * here.inverse());
                        self.put_image(source, *quality);
                        self.context.pop_layer();
                        self.context.set_transform(here);
                    }
                    Step::Section { opacity, .. } => {
                        let section = sections.next().expect("drawn above");
                        self.context.set_transform(Affine::IDENTITY);
                        self.context
                            .push_layer(None, None, Some(*opacity), None, None);
                        self.put_image(&section, ImageQuality::Low);
                        self.context.pop_layer();
                        self.context.set_transform(here);
                    }
                    Step::Canvas(reframe) if reframe.resample => {
                        carried = Some(*reframe);
                        break;
                    }
                    Step::Canvas(_) => {
                        if cuts.clips[part].is_some() {
                            self.context.pop_layer();
                        }
                        part += 1;
                        here = base * shift * cuts.shifts[part];
                        self.context.set_transform(here);
                    }
                }
            }
            for _ in cuts.clips[part..].iter().flatten() {
                self.context.pop_layer();
            }
            let only_changes = rest[..taken]
                .iter()
                .all(|step| matches!(step, Step::Canvas(_)));
            rest = &rest[taken..];
            if carried.is_some() && so_far.is_none() && only_changes {
                // Nothing is drawn on the canvas before, so nothing is carried.
                carried = None;
                (canvas, part) = (canvas + 1, 0);
                let next = take_buffer(&mut free, cache, areas[canvas].size);
                free.push(std::mem::replace(&mut target, next));
                continue;
            }
            self.context.flush();
            let encoded = started.elapsed();
            self.context.render_with(
                &mut target,
                &mut self.resources,
                RasterizerSettings::default(),
            );
            timings.encode += encoded;
            timings.rasterize += started.elapsed() - encoded;
            if rest.is_empty() && carried.is_none() {
                break;
            }
            if carried.is_some() {
                (canvas, part) = (canvas + 1, 0);
            }
            let next = take_buffer(&mut free, cache, areas[canvas].size);
            drawn_from = so_far.replace(Arc::new(std::mem::replace(&mut target, next)));
        }
        *pixmap = target;
        if so_far.is_some() {
            self.context.reset_and_resize(last.size[0], last.size[1]);
            let left = [so_far, drawn_from].into_iter().flatten();
            free.extend(left.filter_map(|used| Arc::try_unwrap(used).ok()));
        }
        for left in free {
            cache.keep_spare(left);
        }
        self.paths = paths;
        self.dabs = dabs;
        timings
    }

    /// Fills `image`'s rectangle in the current transform with its pixels.
    fn put_image(&mut self, image: &Arc<Pixmap>, quality: ImageQuality) {
        let size = Rect::new(
            0.0,
            0.0,
            f64::from(image.width()),
            f64::from(image.height()),
        );
        self.context.set_paint(Image {
            image: ImageSource::Pixmap(image.clone()),
            sampler: ImageSampler {
                quality,
                ..ImageSampler::default()
            },
        });
        self.context.fill_rect(&size);
    }

    /// The outline of each `Step::Draw` into `paths`, or for an airbrush or
    /// spray its dabs into `dabs`, in order (the other one and both without
    /// samples left empty), made on the worker threads in contiguous runs.
    fn outlines(
        &self,
        steps: &[Step<'_>],
        frame: u32,
        detail: u32,
        paths: &mut Vec<BezPath>,
        dabs: &mut Vec<Vec<Dab>>,
    ) {
        let draws: Vec<(&Stroke, &Pen)> = steps
            .iter()
            .filter_map(|step| match step {
                Step::Draw { stroke, pen, .. } => Some((*stroke, pen)),
                _ => None,
            })
            .collect();
        paths.resize_with(draws.len(), BezPath::new);
        dabs.resize_with(draws.len(), Vec::new);
        let make = |(stroke, pen): &(&Stroke, &Pen), path: &mut BezPath, dabs: &mut Vec<Dab>| {
            if stroke.brush.engine == BrushEngine::Line {
                dabs.clear();
                let spacing =
                    stroke::spacing(stroke.width) * f64::from(detail) / f64::from(FULL_DETAIL);
                let samples = Resampler::whole(&stroke.points, spacing);
                stroke::outline_into(&samples, pen, frame, path);
            } else {
                // Spacing is part of how dabs add up, so it keeps full detail.
                path.truncate(0);
                dab::stroke_dabs(&stroke.points, pen, frame, stroke.color.0[3], dabs);
            }
        };
        let run = draws.len().div_ceil(self.threads).max(1);
        if self.threads == 1 || draws.len() < 2 {
            for ((draw, path), dabs) in draws.iter().zip(paths.iter_mut()).zip(dabs.iter_mut()) {
                make(draw, path, dabs);
            }
            return;
        }
        std::thread::scope(|scope| {
            let chunks = draws
                .chunks(run)
                .zip(paths.chunks_mut(run))
                .zip(dabs.chunks_mut(run));
            for ((chunk, paths), dabs) in chunks {
                scope.spawn(move || {
                    for ((draw, path), dabs) in chunk.iter().zip(paths).zip(dabs) {
                        make(draw, path, dabs);
                    }
                });
            }
        });
    }

    /// Draws `stroke` along `path`, inside `clip` when given. A layer takes
    /// the aliasing setting its clip is pushed with, so the clip is pushed
    /// first and stays exact for aliased pens too.
    fn draw(&mut self, stroke: &Stroke, erase: bool, path: &BezPath, clip: Option<&BezPath>) {
        let [r, g, b, _] = stroke.color.0;
        let alpha = stroke::line_alpha(stroke);
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

    fn is_ready(&self, index: usize) -> bool {
        self.ready.front().is_some_and(|(at, _)| *at == index)
    }

    /// Paints the dab strokes from the one at `first`, the first of
    /// `ahead`, to the next change of canvas or moved selection, while their
    /// buffers fit the budget (the first always does), on as many threads as
    /// this raster has. `here` takes them to the target, which is `size`
    /// pixels.
    fn paint_ahead(
        &mut self,
        ahead: &[Step<'_>],
        first: usize,
        dabs: &[Vec<Dab>],
        here: Affine,
        size: [u16; 2],
    ) {
        let mut jobs = Vec::new();
        let mut index = first;
        for step in ahead {
            match step {
                Step::Draw { stroke, erase, .. } => {
                    let list = &dabs[index];
                    if !list.is_empty() {
                        if !jobs.is_empty()
                            && self.held + dab_bytes(list, here, size) > self.dab_budget
                        {
                            break;
                        }
                        let slot = jobs.len();
                        if slot == self.placed.len() {
                            self.placed.push(Vec::new());
                        }
                        let placement = placed(list, here, size, &mut self.placed[slot]);
                        if let Some((_, [width, height])) = placement {
                            self.held += usize::from(width) * usize::from(height) * 4;
                        }
                        let [r, g, b, _] = if *erase { [0; 4] } else { stroke.color.0 };
                        let look = dab::look(&stroke.brush);
                        jobs.push((index, placement, look, [r, g, b]));
                    }
                    index += 1;
                }
                Step::Move { .. } | Step::Canvas(_) => break,
                _ => {}
            }
        }
        let painting = jobs.iter().filter(|job| job.1.is_some()).count();
        while self.buffers.len() < self.used + painting {
            self.buffers.push(Arc::new(Pixmap::new(1, 1)));
        }
        let mut buffers = self.buffers[self.used..self.used + painting].iter_mut();
        let mut tasks = Vec::new();
        for (slot, (_, placement, look, rgb)) in jobs.iter().enumerate() {
            let Some((_, [width, height])) = placement else {
                continue;
            };
            let buffer = buffers.next().expect("one for each");
            if Arc::get_mut(buffer).is_none() {
                *buffer = Arc::new(Pixmap::new(1, 1));
            }
            let pixels = Arc::get_mut(buffer).expect("just made");
            pixels.resize(*width, *height);
            let task = (pixels, &self.placed[slot], *look, *rgb);
            tasks.push(std::sync::Mutex::new(task));
        }
        let threads = self.threads.min(tasks.len()).max(1);
        self.painters
            .resize_with(threads.max(self.painters.len()), Default::default);
        let next = std::sync::atomic::AtomicUsize::new(0);
        let work = |painter: &mut dab::Painter| {
            while let Some(task) = tasks.get(next.fetch_add(1, Ordering::Relaxed)) {
                let mut task = task.lock().expect("no panic while painting");
                let (pixels, placed, look, rgb) = &mut *task;
                let width = usize::from(pixels.width());
                let bytes = pixels.data_as_u8_slice_mut();
                bytes.fill(0);
                painter.paint(bytes, width, placed, *look, *rgb);
            }
        };
        if threads == 1 {
            work(&mut self.painters[0]);
        } else {
            std::thread::scope(|scope| {
                for painter in &mut self.painters[..threads] {
                    scope.spawn(|| work(painter));
                }
            });
        }
        drop(tasks);
        for (index, placement, ..) in jobs {
            let painted = placement.map(|(origin, _)| {
                self.used += 1;
                Painted {
                    origin,
                    buffer: self.buffers[self.used - 1].clone(),
                }
            });
            self.ready.push_back((index, painted));
        }
    }

    /// Lays a painted dab stroke over what is there, or erases it from it,
    /// within `clip`.
    fn lay_dabs(&mut self, painted: &Painted, erase: bool, clip: Option<&BezPath>, here: Affine) {
        let layer = erase || clip.is_some();
        if layer {
            let mode = erase.then(|| BlendMode::new(Mix::Normal, Compose::DestOut));
            self.context.push_layer(clip, mode, None, None, None);
        }
        let [left, top] = painted.origin.map(f64::from);
        self.context.set_transform(Affine::translate((left, top)));
        self.put_image(&painted.buffer, ImageQuality::Low);
        self.context.set_transform(here);
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

/// The scaled pixels a run of steps draws into.
#[derive(Clone, Copy)]
struct Area {
    origin: [u32; 2],
    size: [u16; 2],
}

impl Area {
    /// From document pixels to this area's pixels.
    fn base(self, shrink: u32) -> Affine {
        Affine::translate((-f64::from(self.origin[0]), -f64::from(self.origin[1])))
            * Affine::scale(1.0 / f64::from(shrink))
    }
}

/// A buffer of `size` from `free`, or from those `cache` kept, or a new one.
fn take_buffer(free: &mut Vec<Pixmap>, cache: &DrawCache, size: [u16; 2]) -> Pixmap {
    match free
        .iter()
        .position(|buffer| [buffer.width(), buffer.height()] == size)
    {
        Some(index) => free.swap_remove(index),
        None => cache
            .take_spare(size)
            .unwrap_or_else(|| Pixmap::new(size[0], size[1])),
    }
}

/// What drawing `paint` takes, in order.
fn layer_steps<'a>(document: &'a Document, paint: &PaintLayer, masks: &DrawCache) -> Vec<Step<'a>> {
    let mut steps = Vec::new();
    let mut size = paint.initial_size;
    collect(
        &paint.ops,
        document,
        masks,
        paint.wobble.unwrap_or(document.wobble),
        &mut size,
        &mut steps,
    );
    steps
}

/// `transform` of the document as Vello's.
pub(crate) fn affine(transform: ops::Affine) -> Affine {
    let [a, b, c, d, e, f] = transform.0;
    Affine::new([a, d, b, e, c, f])
}

/// How a moved selection or placed image is sampled. Like 2.2.13, smooth
/// sampling is dropped where every pixel lands on a whole pixel: turns by a
/// quarter, flips and whole-pixel moves.
pub(crate) fn quality(sampling: Sampling, transform: ops::Affine) -> ImageQuality {
    let [a, b, c, d, e, f] = transform.0;
    let unit = |value: f64| value.abs() == 1.0;
    let square = (unit(a) && b == 0.0 && d == 0.0 && unit(e))
        || (a == 0.0 && unit(b) && unit(d) && e == 0.0);
    match sampling {
        Sampling::Nearest => ImageQuality::Low,
        Sampling::Smooth if square && c.fract() == 0.0 && f.fract() == 0.0 => ImageQuality::Low,
        Sampling::Smooth => ImageQuality::Medium,
    }
}

/// The bounds of `bounds` (left, top, right, bottom) moved by `transform`.
pub(crate) fn moved_bounds(bounds: [f64; 4], transform: Affine) -> [f64; 4] {
    let [left, top, right, bottom] = bounds;
    let corners = [(left, top), (right, top), (left, bottom), (right, bottom)]
        .map(|(x, y)| transform * Point::new(x, y));
    let (xs, ys) = (corners.map(|point| point.x), corners.map(|point| point.y));
    [
        xs.into_iter().fold(f64::INFINITY, f64::min),
        ys.into_iter().fold(f64::INFINITY, f64::min),
        xs.into_iter().fold(f64::NEG_INFINITY, f64::max),
        ys.into_iter().fold(f64::NEG_INFINITY, f64::max),
    ]
}

/// `dabs` taken by `here` into `placed`, in pixels of a target of `size`,
/// with the top left and size of the pixels they can paint there.
fn placed(
    dabs: &[Dab],
    here: Affine,
    size: [u16; 2],
    placed: &mut Vec<Dab>,
) -> Option<([u16; 2], [u16; 2])> {
    // Layers are drawn scaled and moved, never turned.
    let [scale, _, _, _, dx, dy] = here.as_coeffs();
    placed.clear();
    placed.extend(dabs.iter().map(|dab| Dab {
        center: [dab.center[0] * scale + dx, dab.center[1] * scale + dy],
        diameter: dab.diameter * scale,
        alpha: dab.alpha,
    }));
    let [left, top, right, bottom] = dab::bounds(placed);
    let [width, height] = size.map(f64::from);
    let left = left.floor().clamp(0.0, width);
    let top = top.floor().clamp(0.0, height);
    let right = right.ceil().clamp(0.0, width);
    let bottom = bottom.ceil().clamp(0.0, height);
    if right <= left || bottom <= top {
        return None;
    }
    for dab in placed.iter_mut() {
        dab.center = [dab.center[0] - left, dab.center[1] - top];
    }
    Some((
        [left as u16, top as u16],
        [(right - left) as u16, (bottom - top) as u16],
    ))
}

/// Bytes of the buffer the stroke of `dabs` is painted into.
fn dab_bytes(dabs: &[Dab], here: Affine, size: [u16; 2]) -> usize {
    if dabs.is_empty() {
        return 0;
    }
    let [scale, _, _, _, dx, dy] = here.as_coeffs();
    let [left, top, right, bottom] = dab::bounds(dabs);
    let [width, height] = size.map(f64::from);
    let wide =
        ((right * scale + dx).ceil().min(width) - (left * scale + dx).floor().max(0.0)).max(0.0);
    let tall =
        ((bottom * scale + dy).ceil().min(height) - (top * scale + dy).floor().max(0.0)).max(0.0);
    (wide * tall) as usize * 4
}

/// Where the steps that add pixels reach, in document pixels. Erasing and
/// clearing only take away.
fn step_bounds(steps: &[Step<'_>], out: &mut Vec<[f64; 4]>) {
    for step in steps {
        match step {
            Step::Draw {
                stroke,
                pen,
                erase: false,
                ..
            } => out.push(stroke::bounds(&stroke.points, pen)),
            Step::Fill { ready, .. } => {
                out.extend(ready.bounds.map(|bounds| bounds.map(f64::from)))
            }
            Step::Image {
                image, transform, ..
            } => out.push(moved_bounds(
                [
                    0.0,
                    0.0,
                    f64::from(image.width()),
                    f64::from(image.height()),
                ],
                *transform,
            )),
            Step::Move {
                ready, transform, ..
            } => out.extend(
                ready
                    .bounds
                    .map(|bounds| moved_bounds(bounds.map(f64::from), *transform)),
            ),
            Step::Section { steps, .. } => step_bounds(steps, out),
            Step::Canvas(reframe) => carry_all(out, reframe),
            _ => {}
        }
    }
}

/// Where `ops` may add pixels, from stroke bounds and mask bounds alone,
/// without reading mask bits; a fill may cover less.
fn op_bounds(
    ops: &[Op],
    document: &Document,
    wobble: Wobble,
    size: &mut [u32; 2],
    out: &mut Vec<[f64; 4]>,
) {
    let store = &document.store;
    for op in ops {
        match op {
            Op::Paint { stroke, .. } => {
                if let Some(stroke) = store.strokes.get(stroke) {
                    out.push(stroke::bounds(
                        &stroke.points,
                        &Pen::new(stroke, wobble, document.frames),
                    ));
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
            Op::PlaceImage {
                asset, transform, ..
            } => {
                if let Some(asset) = store.assets.get(asset) {
                    let size = asset.size.map(f64::from);
                    out.push(moved_bounds(
                        [0.0, 0.0, size[0], size[1]],
                        affine(*transform),
                    ));
                }
            }
            Op::TransformSelection {
                mask, transform, ..
            } => {
                if let Some(mask) = store.masks.get(mask) {
                    let [left, top, width, height] = mask.bounds.map(f64::from);
                    out.push(moved_bounds(
                        [left, top, left + width, top + height],
                        affine(*transform),
                    ));
                }
            }
            Op::Isolated(section) => op_bounds(
                &section.ops,
                document,
                section.wobble.unwrap_or(document.wobble),
                size,
                out,
            ),
            Op::Crop { .. } | Op::Resample { .. } => {
                if let Some(reframe) = Reframe::of(op, *size) {
                    carry_all(out, &reframe);
                    *size = reframe.to;
                }
            }
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
            let mut size = paint.initial_size;
            op_bounds(
                &paint.ops,
                document,
                paint.wobble.unwrap_or(document.wobble),
                &mut size,
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
pub(crate) struct DrawCache {
    prepared: std::sync::Mutex<HashMap<MaskKey, Prepared>>,
    /// Decoded assets and the render each was last used in; `None` when an
    /// asset cannot be decoded, which validation should have kept out.
    images: std::sync::Mutex<HashMap<AssetId, Decoded>>,
    /// Buffers left over from drawing layers that move a selection or
    /// resample, for the next such layers, whichever thread draws them,
    /// until `release_scratch`.
    spare: std::sync::Mutex<Vec<Pixmap>>,
    /// How many `spare` keeps: one per layer drawn at once.
    spare_count: usize,
    render: u64,
}

struct Decoded {
    image: Option<Arc<Pixmap>>,
    /// The render it was last used in.
    used: u64,
}

fn mask_key(mask: &Mask) -> (usize, [i32; 4]) {
    (mask.bits.as_ptr() as usize, mask.bounds)
}

impl DrawCache {
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
            let (area, fringe) = Runs::fill(coverage, clip, antialias);
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

    /// `asset`'s pixels; an asset never changes once stored, so its id names
    /// them.
    fn image(&self, id: AssetId, asset: &ugu_core::store::Asset) -> Option<Arc<Pixmap>> {
        let lock = || self.images.lock().expect("no panic while holding it");
        if let Some(decoded) = lock().get_mut(&id) {
            decoded.used = self.render;
            return decoded.image.clone();
        }
        let image = match crate::image::decode(asset) {
            Ok(pixmap) => Some(Arc::new(pixmap)),
            Err(error) => {
                tracing::warn!(%error, "an image is left out");
                None
            }
        };
        let decoded = Decoded {
            image: image.clone(),
            used: self.render,
        };
        lock().insert(id, decoded);
        image
    }

    fn take_spare(&self, size: [u16; 2]) -> Option<Pixmap> {
        let mut spare = self.spare.lock().expect("no panic while holding it");
        let index = spare
            .iter()
            .position(|buffer| [buffer.width(), buffer.height()] == size)?;
        Some(spare.swap_remove(index))
    }

    fn keep_spare(&self, pixmap: Pixmap) {
        let mut spare = self.spare.lock().expect("no panic while holding it");
        spare.push(pixmap);
        if spare.len() > self.spare_count {
            spare.remove(0);
        }
    }

    /// Starts a render, letting go of what the one before did not use.
    fn next_render(&mut self) {
        self.render += 1;
        let render = self.render;
        self.prepared
            .get_mut()
            .expect("no panic while holding it")
            .retain(|_, prepared| prepared.used + 1 >= render);
        self.images
            .get_mut()
            .expect("no panic while holding it")
            .retain(|_, decoded| decoded.used + 1 >= render);
    }
}

/// Whether `ops` move a selection, inside sections too.
fn moves_selection(ops: &[Op]) -> bool {
    ops.iter().any(|op| match op {
        Op::TransformSelection { .. } => true,
        Op::Isolated(section) => moves_selection(&section.ops),
        _ => false,
    })
}

fn collect<'a>(
    ops: &[Op],
    document: &'a Document,
    masks: &DrawCache,
    wobble: Wobble,
    size: &mut [u32; 2],
    steps: &mut Vec<Step<'a>>,
) {
    let store = &document.store;
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
                    pen: Pen::new(stroke, wobble, document.frames),
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
            Op::PlaceImage {
                asset,
                transform,
                sampling,
            } => {
                let image = store
                    .assets
                    .get(asset)
                    .and_then(|stored| masks.image(*asset, stored));
                if let Some(image) = image {
                    steps.push(Step::Image {
                        image,
                        transform: affine(*transform),
                        quality: quality(*sampling, *transform),
                    });
                }
            }
            Op::TransformSelection {
                mask,
                transform,
                sampling,
                keep_source,
            } => {
                let Some(mask) = store.masks.get(mask) else {
                    continue;
                };
                let ready = masks.area(mask);
                if ready.bounds.is_some() {
                    steps.push(Step::Move {
                        ready,
                        transform: affine(*transform),
                        quality: quality(*sampling, *transform),
                        keep_source: *keep_source,
                    });
                }
            }
            Op::Isolated(section) => {
                let wobble = section.wobble.unwrap_or(document.wobble);
                if moves_selection(&section.ops) {
                    let mut inner = Vec::new();
                    collect(&section.ops, document, masks, wobble, size, &mut inner);
                    steps.push(Step::Section {
                        steps: inner,
                        opacity: section.opacity,
                    });
                } else {
                    steps.push(Step::Push(section.opacity));
                    collect(&section.ops, document, masks, wobble, size, steps);
                    steps.push(Step::Pop);
                }
            }
            Op::Crop { .. } | Op::Resample { .. } => {
                if let Some(reframe) = Reframe::of(op, *size) {
                    *size = reframe.to;
                    steps.push(Step::Canvas(reframe));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use ugu_core::document::{Layer, LayerId};
    use ugu_core::motion::stepped_pose;
    use ugu_core::ops::{Blend, Motion, MotionStyle, Rgba8, Section, StrokeId, merge_down};
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
                    ..Brush::default()
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
    fn stepped_poses_hold_smooth_ones_blend_and_both_break_alike_everywhere() {
        let (document, mut below, above) = two_layers();
        below.wobble = Some(Wobble::classic(0.0));
        for style in [MotionStyle::Stepped, MotionStyle::Smooth] {
            let motion = Motion {
                style,
                poses: 6,
                broken: true,
                break_amount: 0.4,
                break_range: 4.0,
                ..Motion::DEFAULT
            };
            let moving = PaintLayer {
                wobble: Some(Wobble {
                    amount: 4.0,
                    motion,
                }),
                ..above.clone()
            };
            let under = render(&with_layers(&document, &[below.clone()]), 0, 0);
            let document = with_layers(&document, &[below.clone(), moving]);
            let frames = document.frames;
            let shown: Vec<Pixmap> = (0..frames)
                .map(|frame| render(&document, frame.into(), 0))
                .collect();
            for frame in 0..frames {
                // Some of the broken line shows on every pose.
                assert!(max_difference(&shown[frame as usize], &under) > 0);
                let next = (frame + 1) % frames;
                let same = shown[frame as usize].data_as_u8_slice()
                    == shown[next as usize].data_as_u8_slice();
                let held = style == MotionStyle::Stepped
                    && stepped_pose(frame.into(), frames, 6)
                        == stepped_pose(next.into(), frames, 6);
                assert_eq!(same, held, "{style:?} from frame {frame}");
            }
            let whole = Motion {
                broken: false,
                ..motion
            };
            let unbroken = PaintLayer {
                wobble: Some(Wobble {
                    amount: 4.0,
                    motion: whole,
                }),
                ..above.clone()
            };
            let unbroken = render(&with_layers(&document, &[below.clone(), unbroken]), 7, 0);
            assert!(max_difference(&unbroken, &shown[7]) > 0);
            for frame in [0, 7] {
                let single = &shown[frame];
                let threads = render(&document, frame as i64, 8);
                assert!(threads.data_as_u8_slice() == single.data_as_u8_slice());
                for edge in [16, 64] {
                    let tiled = render_tiled(&document, frame as i64, edge, 8);
                    assert!(max_difference(&tiled, single) <= 1);
                }
            }
        }
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

    /// Adds an image whose straight-alpha pixel at x, y is `pixel(x, y)`.
    fn add_image(
        document: &mut Document,
        size: [u32; 2],
        pixel: impl Fn(u32, u32) -> [u8; 4],
    ) -> ugu_core::ops::AssetId {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, size[0], size[1]);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        let data: Vec<u8> = (0..size[1])
            .flat_map(|y| (0..size[0]).map(move |x| (x, y)))
            .flat_map(|(x, y)| pixel(x, y))
            .collect();
        writer.write_image_data(&data).unwrap();
        writer.finish().unwrap();
        let id = ugu_core::ops::AssetId([document.store.assets.len() as u8 + 1; 32]);
        let asset = ugu_core::store::Asset {
            size,
            png: Arc::from(bytes),
        };
        document.store.assets.insert(id, asset);
        id
    }

    fn paint(stroke: StrokeId) -> Op {
        Op::Paint { stroke, clip: None }
    }

    fn moved(
        mask: ugu_core::ops::MaskId,
        transform: [f64; 6],
        sampling: ugu_core::ops::Sampling,
        keep_source: bool,
    ) -> Op {
        Op::TransformSelection {
            mask,
            transform: ugu_core::ops::Affine(transform),
            sampling,
            keep_source,
        }
    }

    /// `source` over `target`, premultiplied, as Vello rounds it to within a
    /// level.
    fn over(source: [u8; 4], target: [u8; 4]) -> [u8; 4] {
        let keep = 255 - u16::from(source[3]);
        std::array::from_fn(|c| source[c] + ((u16::from(target[c]) * keep + 127) / 255) as u8)
    }

    fn near(got: [u8; 4], expected: [u8; 4]) -> bool {
        (0..4).all(|c| got[c].abs_diff(expected[c]) <= 1)
    }

    #[test]
    fn drawing_again_from_the_pixels_so_far_changes_nothing() {
        for antialias in [true, false] {
            let mut document = document();
            let red = line(
                &mut document,
                8.0,
                88.0,
                24.0,
                [220, 30, 30, 150],
                antialias,
            );
            let blue = line(&mut document, 20.0, 76.0, 20.0, BLUE, antialias);
            // A corner nothing is drawn in, moved onto itself.
            let corner = add_mask(&mut document, [90, 0, 6, 4], |_, _| true);
            let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
            let nearest = ugu_core::ops::Sampling::Nearest;
            let plain = with_ops(&document, vec![paint(red), paint(blue)]);
            let broken = with_ops(
                &document,
                vec![
                    paint(red),
                    moved(corner, identity, nearest, true),
                    paint(blue),
                ],
            );
            for frame in [0, 3] {
                assert!(
                    render(&broken, frame, 0).data_as_u8_slice()
                        == render(&plain, frame, 0).data_as_u8_slice(),
                    "antialias {antialias}, frame {frame}"
                );
            }
        }
    }

    #[test]
    fn a_moved_selection_moves_each_frames_own_result() {
        let mut document = document();
        let red = line(&mut document, 8.0, 88.0, 24.0, RED, true);
        let blue = line(&mut document, 30.0, 60.0, 30.0, [30, 30, 220, 160], true);
        let mask = add_mask(&mut document, [20, 10, 40, 30], |x, y| {
            (x - 40) * (x - 40) + (y - 24) * (y - 24) < 150
        });
        let bits = document.store.masks[&mask].clone();
        let smooth = ugu_core::ops::Sampling::Smooth;
        let still = with_ops(&document, vec![paint(red), paint(blue)]);
        // Whole-pixel moves and a quarter turn about (40, 24): each pixel
        // lands on a whole pixel, so smooth sampling is not used.
        let cases = [
            ([1.0, 0.0, 17.0, 0.0, 1.0, 9.0], false),
            ([1.0, 0.0, -25.0, 0.0, 1.0, 3.0], true),
            ([0.0, -1.0, 64.0, 1.0, 0.0, -16.0], false),
        ];
        for (transform, keep_source) in cases {
            let [a, b, c, d, e, f] = transform;
            let det = a * e - b * d;
            let back = |x: f64, y: f64| {
                let (x, y) = (x - c, y - f);
                ((e * x - b * y) / det, (a * y - d * x) / det)
            };
            let moving = with_ops(
                &document,
                vec![
                    paint(red),
                    paint(blue),
                    moved(mask, transform, smooth, keep_source),
                ],
            );
            let mut frames = Vec::new();
            for frame in [0, 3] {
                let before = render(&still, frame, 0);
                let got = render(&moving, frame, 0);
                for y in 0..got.height() {
                    for x in 0..got.width() {
                        let (cx, cy) = (i32::from(x), i32::from(y));
                        let base = if !keep_source && bits.contains(cx, cy) {
                            [0; 4]
                        } else {
                            at(&before, x, y)
                        };
                        let (sx, sy) = back(f64::from(x) + 0.5, f64::from(y) + 0.5);
                        let (sx, sy) = (sx.floor() as i32, sy.floor() as i32);
                        let inside = (0..i32::from(got.width())).contains(&sx)
                            && (0..i32::from(got.height())).contains(&sy);
                        let source = if inside && bits.contains(sx, sy) {
                            at(&before, sx as u16, sy as u16)
                        } else {
                            [0; 4]
                        };
                        let expected = over(source, base);
                        let pixel = at(&got, x, y);
                        assert!(
                            near(pixel, expected),
                            "{transform:?} frame {frame} at {x}, {y}: {pixel:?} against {expected:?}"
                        );
                    }
                }
                frames.push(got);
            }
            // The strokes moved between the frames, and so did what was cut.
            assert!(max_difference(&frames[0], &frames[1]) > 0);
        }
    }

    #[test]
    fn an_image_is_placed_over_what_came_before() {
        let mut document = document();
        let red = line(&mut document, 8.0, 88.0, 24.0, [220, 30, 30, 200], true);
        let image = add_image(&mut document, [20, 10], |x, y| {
            let alpha = if (x + y) % 3 == 0 { 255 } else { 120 };
            [(x * 12) as u8, (y * 25) as u8, 100, alpha]
        });
        let decoded = crate::image::decode(&document.store.assets[&image]).unwrap();
        let placed = Op::PlaceImage {
            asset: image,
            transform: ugu_core::ops::Affine::translation(30.0, 18.0),
            sampling: ugu_core::ops::Sampling::Smooth,
        };
        for frame in [2, 5] {
            let before = render(&with_ops(&document, vec![paint(red)]), frame, 0);
            let got = render(
                &with_ops(&document, vec![paint(red), placed.clone()]),
                frame,
                0,
            );
            for y in 0..got.height() {
                for x in 0..got.width() {
                    let below = at(&before, x, y);
                    let pixel = at(&got, x, y);
                    if (30..50).contains(&x) && (18..28).contains(&y) {
                        let expected = over(at(&decoded, x - 30, y - 18), below);
                        assert!(near(pixel, expected), "at {x}, {y}");
                    } else {
                        assert_eq!(pixel, below, "outside at {x}, {y}");
                    }
                }
            }
        }
    }

    /// A stroke, a placed image moved half a pixel, a merged section that
    /// moves a selection, and a moved selection taken far outside where
    /// anything was drawn.
    fn moved_work() -> Document {
        let mut document = document();
        let red = line(&mut document, 8.0, 88.0, 24.0, RED, true);
        let blue = line(&mut document, 30.0, 60.0, 14.0, [30, 30, 220, 160], true);
        let mask = add_mask(&mut document, [20, 4, 50, 40], |x, y| {
            (x - 40) * (x - 40) + (y - 20) * (y - 20) < 220
        });
        let image = add_image(&mut document, [16, 12], |x, y| {
            [(x * 15) as u8, 200, (y * 20) as u8, 230]
        });
        let section = Op::Isolated(Box::new(Section {
            ops: vec![
                paint(blue),
                moved(
                    mask,
                    [1.0, 0.0, 9.5, 0.0, 1.0, 4.25],
                    ugu_core::ops::Sampling::Smooth,
                    false,
                ),
            ],
            opacity: 0.6,
            wobble: None,
        }));
        with_ops(
            &document,
            vec![
                paint(red),
                Op::PlaceImage {
                    asset: image,
                    transform: ugu_core::ops::Affine([0.8, 0.3, 50.5, -0.3, 0.8, 10.0]),
                    sampling: ugu_core::ops::Sampling::Smooth,
                },
                section,
                moved(
                    mask,
                    [1.0, 0.0, 40.0, 0.0, 1.0, 20.0],
                    ugu_core::ops::Sampling::Nearest,
                    true,
                ),
            ],
        )
    }

    #[test]
    fn a_merged_section_that_moves_a_selection_draws_as_its_own_layer_would() {
        let mut document = document();
        let red = line(&mut document, 8.0, 88.0, 24.0, RED, true);
        let blue = line(&mut document, 30.0, 60.0, 22.0, [30, 30, 220, 160], true);
        let mask = add_mask(&mut document, [20, 10, 40, 30], |x, y| {
            (x - 40) * (x - 40) + (y - 24) * (y - 24) < 150
        });
        let inner = vec![
            paint(blue),
            moved(
                mask,
                [1.0, 0.0, 10.0, 0.0, 1.0, 4.0],
                ugu_core::ops::Sampling::Nearest,
                false,
            ),
        ];
        let merged = with_ops(
            &document,
            vec![
                paint(red),
                Op::Isolated(Box::new(Section {
                    ops: inner.clone(),
                    opacity: 0.6,
                    wobble: None,
                })),
            ],
        );
        let mut below = paint_layer(&mut document).clone();
        below.ops = vec![paint(red)];
        let mut above = below.clone();
        above.ops = inner;
        above.opacity = 0.6;
        let apart = with_layers(&document, &[below, above]);
        for frame in [0, 3] {
            let most = max_difference(&render(&merged, frame, 0), &render(&apart, frame, 0));
            assert!(most <= 1, "frame {frame} differs by {most}");
        }
    }

    #[test]
    fn moved_selections_and_images_draw_alike_everywhere() {
        let document = moved_work();
        for frame in [0, 5] {
            let single = render(&document, frame, 0);
            assert!(render(&document, frame, 8).data_as_u8_slice() == single.data_as_u8_slice());
            for edge in [16, 64, 4096] {
                assert!(max_difference(&render_tiled(&document, frame, edge, 8), &single) <= 1);
            }
        }
    }

    /// `document` with its one layer drawing `ops` from a canvas of
    /// `initial`, and the canvas those ops end on.
    fn reframed(document: &Document, initial: [u32; 2], ops: Vec<Op>) -> Document {
        let mut document = with_ops(document, ops);
        let paint = paint_layer(&mut document);
        paint.initial_size = initial;
        document.canvas = paint.final_size();
        document
    }

    fn crop(offset: [i32; 2], size: [u32; 2]) -> Op {
        Op::Crop { offset, size }
    }

    fn resample(size: [u32; 2], sampling: ugu_core::ops::Sampling) -> Op {
        Op::Resample { size, sampling }
    }

    /// `before`'s pixel that a crop by `offset` puts at `x`, `y`.
    fn cropped_at(before: &Pixmap, offset: [i32; 2], x: u16, y: u16) -> [u8; 4] {
        let (from_x, from_y) = (i32::from(x) - offset[0], i32::from(y) - offset[1]);
        let inside = (0..i32::from(before.width())).contains(&from_x)
            && (0..i32::from(before.height())).contains(&from_y);
        if inside {
            at(before, from_x as u16, from_y as u16)
        } else {
            [0; 4]
        }
    }

    /// The pixel 2.2.13's `ImageResampler` makes at `x`, `y` when it resizes
    /// `before` to `size`, in f64.
    fn resampled_at(
        before: &Pixmap,
        size: [u32; 2],
        sampling: ugu_core::ops::Sampling,
        x: u16,
        y: u16,
    ) -> [f64; 4] {
        let from = [before.width(), before.height()].map(f64::from);
        let to = size.map(f64::from);
        let source = [
            (f64::from(x) + 0.5) * from[0] / to[0],
            (f64::from(y) + 0.5) * from[1] / to[1],
        ];
        let tap = |x: f64, y: f64| {
            let x = x.clamp(0.0, from[0] - 1.0) as u16;
            let y = y.clamp(0.0, from[1] - 1.0) as u16;
            at(before, x, y).map(f64::from)
        };
        if sampling == ugu_core::ops::Sampling::Nearest {
            return tap(source[0].floor(), source[1].floor());
        }
        let [sx, sy] = source.map(|value| value - 0.5);
        let (left, top) = (sx.floor(), sy.floor());
        let (across, down) = (sx - left, sy - top);
        let [a, b, c, d] = [
            tap(left, top),
            tap(left + 1.0, top),
            tap(left, top + 1.0),
            tap(left + 1.0, top + 1.0),
        ];
        std::array::from_fn(|channel| {
            let upper = a[channel] * (1.0 - across) + b[channel] * across;
            let lower = c[channel] * (1.0 - across) + d[channel] * across;
            upper * (1.0 - down) + lower * down
        })
    }

    #[test]
    fn a_crop_carries_the_pixels_so_far_and_later_strokes_use_the_new_canvas() {
        let mut document = document();
        let red = line(&mut document, 8.0, 88.0, 24.0, [220, 30, 30, 200], true);
        let blue = line(&mut document, 30.0, 60.0, 30.0, BLUE, false);
        let green = line(&mut document, 4.0, 40.0, 40.0, [30, 200, 30, 180], true);
        let initial = document.canvas;
        // Narrower and taller, moved left and down; then wider and moved
        // right, which shows again where the first crop cut away.
        let cases = [
            vec![crop([-10, 6], [80, 56])],
            vec![crop([-10, 6], [80, 56]), crop([20, 0], [110, 56])],
        ];
        for crops in cases {
            for frame in [0, 3] {
                let mut so_far = render(
                    &with_ops(&document, vec![paint(red), paint(blue)]),
                    frame,
                    0,
                );
                let mut ops = vec![paint(red), paint(blue)];
                for each in &crops {
                    let Op::Crop { offset, size } = *each else {
                        unreachable!()
                    };
                    ops.push(each.clone());
                    let got = render(&reframed(&document, initial, ops.clone()), frame, 0);
                    assert_eq!([got.width(), got.height()].map(u32::from), size);
                    for y in 0..got.height() {
                        for x in 0..got.width() {
                            let expected = cropped_at(&so_far, offset, x, y);
                            assert_eq!(at(&got, x, y), expected, "frame {frame} at {x}, {y}");
                        }
                    }
                    so_far = got;
                }
                ops.push(paint(green));
                let got = render(&reframed(&document, initial, ops), frame, 0);
                let mut alone = document.clone();
                alone.canvas = [u32::from(so_far.width()), u32::from(so_far.height())];
                paint_layer(&mut alone).initial_size = alone.canvas;
                let green_alone = render(&with_ops(&alone, vec![paint(green)]), frame, 0);
                for y in 0..got.height() {
                    for x in 0..got.width() {
                        let expected = over(at(&green_alone, x, y), at(&so_far, x, y));
                        assert!(near(at(&got, x, y), expected), "frame {frame} at {x}, {y}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_resample_scales_the_pixels_so_far_as_2_2_13_does() {
        use ugu_core::ops::Sampling::{Nearest, Smooth};
        let mut document = document();
        let red = line(&mut document, 8.0, 88.0, 24.0, [220, 30, 30, 200], true);
        let blue = line(&mut document, 30.0, 60.0, 30.0, BLUE, false);
        let green = line(&mut document, 4.0, 40.0, 20.0, [30, 200, 30, 180], true);
        let initial = document.canvas;
        // Nearest at 2/3, where no pixel centre falls on a pixel edge, is
        // exact; smooth both ways larger and smaller, with 8-bit weights like
        // 2.2.13's.
        let cases = [
            ([64, 32], Nearest, 0.0),
            ([120, 70], Smooth, 2.0),
            ([50, 31], Smooth, 2.0),
        ];
        for (size, sampling, limit) in cases {
            for frame in [0, 3] {
                let before = render(
                    &with_ops(&document, vec![paint(red), paint(blue)]),
                    frame,
                    0,
                );
                let ops = vec![paint(red), paint(blue), resample(size, sampling)];
                let got = render(&reframed(&document, initial, ops.clone()), frame, 0);
                assert_eq!([got.width(), got.height()].map(u32::from), size);
                for y in 0..got.height() {
                    for x in 0..got.width() {
                        let expected = resampled_at(&before, size, sampling, x, y);
                        let pixel = at(&got, x, y);
                        let most = (0..4)
                            .map(|c| (f64::from(pixel[c]) - expected[c]).abs())
                            .fold(0.0, f64::max);
                        assert!(
                            most <= limit,
                            "{size:?} frame {frame} at {x}, {y}: {pixel:?} against {expected:?}"
                        );
                    }
                }
                let mut later = ops;
                later.push(paint(green));
                let with_green = render(&reframed(&document, initial, later), frame, 0);
                let mut alone = document.clone();
                alone.canvas = size;
                paint_layer(&mut alone).initial_size = size;
                let green_alone = render(&with_ops(&alone, vec![paint(green)]), frame, 0);
                for y in 0..got.height() {
                    for x in 0..got.width() {
                        let expected = over(at(&green_alone, x, y), at(&got, x, y));
                        // Both sides of the expectation are rounded.
                        let pixel = at(&with_green, x, y);
                        let most = (0..4).map(|c| pixel[c].abs_diff(expected[c])).max();
                        assert!(most <= Some(2), "{size:?} at {x}, {y}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_selection_moved_right_after_a_crop_moves_the_cropped_pixels() {
        let mut document = document();
        let red = line(&mut document, 8.0, 88.0, 24.0, RED, true);
        let blue = line(&mut document, 30.0, 60.0, 30.0, [30, 30, 220, 160], true);
        // In the cropped canvas's pixels.
        let mask = add_mask(&mut document, [20, 10, 40, 30], |x, y| {
            (x - 40) * (x - 40) + (y - 24) * (y - 24) < 150
        });
        let bits = document.store.masks[&mask].clone();
        let initial = document.canvas;
        let cropped = vec![paint(red), paint(blue), crop([-6, 4], [90, 52])];
        let mut moving = cropped.clone();
        moving.push(moved(
            mask,
            [1.0, 0.0, 17.0, 0.0, 1.0, 9.0],
            ugu_core::ops::Sampling::Nearest,
            false,
        ));
        for frame in [0, 3] {
            let before = render(&reframed(&document, initial, cropped.clone()), frame, 0);
            let got = render(&reframed(&document, initial, moving.clone()), frame, 0);
            for y in 0..got.height() {
                for x in 0..got.width() {
                    let (cx, cy) = (i32::from(x), i32::from(y));
                    let base = if bits.contains(cx, cy) {
                        [0; 4]
                    } else {
                        at(&before, x, y)
                    };
                    let (sx, sy) = (cx - 17, cy - 9);
                    let source = if sx >= 0 && sy >= 0 && bits.contains(sx, sy) {
                        at(&before, sx as u16, sy as u16)
                    } else {
                        [0; 4]
                    };
                    let expected = over(source, base);
                    assert!(near(at(&got, x, y), expected), "frame {frame} at {x}, {y}");
                }
            }
        }
    }

    /// Strokes, a fill and a moved selection across two crops and, with
    /// `resamples`, two resamples, with an eraser after each change.
    fn reframed_work(resamples: bool) -> Document {
        use ugu_core::ops::Sampling::{Nearest, Smooth};
        let mut document = document();
        let red = line(&mut document, 8.0, 88.0, 24.0, RED, true);
        let blue = line(&mut document, 30.0, 60.0, 14.0, [30, 30, 220, 160], true);
        let green = line(&mut document, 4.0, 70.0, 40.0, [30, 200, 30, 220], true);
        let eraser = line(&mut document, 20.0, 50.0, 30.0, [0, 0, 0, 255], true);
        let mask = add_mask(&mut document, [20, 4, 50, 40], |x, y| {
            (x - 40) * (x - 40) + (y - 20) * (y - 20) < 220
        });
        let fill = Op::Fill {
            coverage: mask,
            color: Rgba8([250, 200, 0, 200]),
            antialias: true,
            clip: None,
        };
        let erase = Op::Erase {
            stroke: eraser,
            clip: None,
        };
        let initial = document.canvas;
        let mut ops = vec![
            paint(red),
            fill,
            crop([-6, 4], [90, 60]),
            moved(mask, [1.0, 0.0, 9.5, 0.0, 1.0, 4.25], Smooth, false),
            paint(blue),
            erase.clone(),
            resample([140, 84], Smooth),
            paint(green),
            erase.clone(),
            crop([13, -7], [120, 80]),
            paint(red),
            resample([100, 60], Nearest),
            erase,
        ];
        if !resamples {
            ops.retain(|op| !matches!(op, Op::Resample { .. }));
        }
        reframed(&document, initial, ops)
    }

    #[test]
    fn crops_and_resamples_draw_alike_everywhere() {
        for resamples in [false, true] {
            let document = reframed_work(resamples);
            document.validate().unwrap();
            for frame in [0, 5] {
                let single = render(&document, frame, 0);
                assert!(single.data_as_u8_slice().iter().any(|value| *value > 0));
                let threads = render(&document, frame, 8);
                assert!(threads.data_as_u8_slice() == single.data_as_u8_slice());
                for edge in [16, 64, 4096] {
                    assert!(max_difference(&render_tiled(&document, frame, edge, 8), &single) <= 1);
                }
            }
        }
    }

    /// A stroke of `brush` from `points` (x, y, pressure).
    fn brushed(
        document: &mut Document,
        brush: Brush,
        width: f32,
        color: [u8; 4],
        points: &[(f32, f32, f32)],
    ) -> StrokeId {
        let id = StrokeId(document.store.strokes.len() as u32);
        let points: Vec<Point> = points
            .iter()
            .map(|&(x, y, pressure)| Point { x, y, pressure })
            .collect();
        document.store.strokes.insert(
            id,
            Stroke {
                points: Arc::from(points),
                color: Rgba8(color),
                width,
                brush,
                seed: 0x1234_5678_9abc_def0,
            },
        );
        id
    }

    fn painted(pixmap: &Pixmap) -> bool {
        pixmap.data_as_u8_slice().iter().any(|value| *value > 0)
    }

    #[test]
    fn every_built_in_brush_and_eraser_draws_alike_everywhere() {
        use ugu_core::brush::{BRUSHES, ERASERS};
        for (preset, erase) in BRUSHES
            .iter()
            .map(|preset| (preset, false))
            .chain(ERASERS.iter().map(|preset| (preset, true)))
        {
            let mut document = Document::new([128, 96]);
            document.background = Rgba8([0, 0, 0, 0]);
            document.wobble = Wobble::classic(1.6);
            let under = line(&mut document, 8.0, 120.0, 48.0, RED, true);
            let width = preset.size.min(64.0);
            let stroke = brushed(
                &mut document,
                preset.brush,
                width,
                [20, 40, 80, 255],
                &[(24.0, 48.0, 0.45), (64.0, 40.0, 0.8), (104.0, 48.0, 1.0)],
            );
            let op = if erase {
                Op::Erase { stroke, clip: None }
            } else {
                Op::Paint { stroke, clip: None }
            };
            let document = with_ops(&document, vec![paint(under), op]);
            let below = render(&with_ops(&document, vec![paint(under)]), 3, 0);
            for frame in [0, 3] {
                let single = render(&document, frame, 0);
                assert!(
                    render(&document, frame, 0).data_as_u8_slice() == single.data_as_u8_slice()
                );
                assert!(
                    render(&document, frame, 8).data_as_u8_slice() == single.data_as_u8_slice(),
                    "{}",
                    preset.id
                );
                for edge in [16, 4096] {
                    let most = max_difference(&render_tiled(&document, frame, edge, 8), &single);
                    assert!(most <= 1, "{} tiles of {edge} differ by {most}", preset.id);
                }
            }
            let drawn = render(&document, 3, 0);
            assert!(painted(&drawn), "{}", preset.id);
            assert!(
                max_difference(&drawn, &below) > 0,
                "{} changes nothing",
                preset.id
            );
        }
    }

    #[test]
    fn a_soft_airbrush_fades_from_the_middle() {
        let mut document = Document::new([80, 80]);
        document.background = Rgba8([0, 0, 0, 0]);
        document.wobble = Wobble::classic(0.0);
        let brush = ugu_core::brush::find("soft-airbrush").unwrap().brush;
        let dab = brushed(
            &mut document,
            brush,
            48.0,
            [0, 0, 0, 255],
            &[(40.0, 40.0, 1.0)],
        );
        let result = render(&with_ops(&document, vec![paint(dab)]), 0, 0);
        let [center, middle, edge] = [40, 52, 64].map(|x| at(&result, x, 40)[3]);
        assert!(center > middle && middle > edge, "{center} {middle} {edge}");
        assert!(center > 0 && center < 255);
    }

    #[test]
    fn dab_strokes_past_the_budget_draw_the_same_in_runs() {
        let mut document = Document::new([96, 72]);
        document.wobble = Wobble::classic(1.0);
        let mut ops = Vec::new();
        let hard = Brush {
            engine: BrushEngine::Airbrush,
            antialias: true,
            ..Brush::DEFAULT
        };
        let brushes = [
            ugu_core::brush::find("soft-airbrush").unwrap().brush,
            ugu_core::brush::find("pixel-spray").unwrap().brush,
            hard,
        ];
        for (index, brush) in brushes.into_iter().enumerate() {
            let y = 12.0 + 18.0 * index as f32;
            let color = [200, 40, 60 * index as u8, 255];
            let dabs = brushed(
                &mut document,
                brush,
                20.0,
                color,
                &[(8.0, y, 0.5), (88.0, y, 1.0)],
            );
            let pen = line(&mut document, 8.0, 88.0, y + 6.0, BLUE, true);
            ops.extend([paint(dabs), paint(pen)]);
        }
        let eraser = ugu_core::brush::find("soft-eraser").unwrap().brush;
        let rub = brushed(
            &mut document,
            eraser,
            24.0,
            [0, 0, 0, 255],
            &[(48.0, 4.0, 1.0), (48.0, 68.0, 1.0)],
        );
        ops.push(Op::Erase {
            stroke: rub,
            clip: None,
        });
        let document = with_ops(&document, ops);
        let whole = render(&document, 2, 1);
        let mut runs = Pixmap::new(96, 72);
        let mut renderer = DocumentRenderer::new(1);
        renderer.main.dab_budget = 1;
        renderer.render(&document, 2, Purpose::Display, &mut runs);
        assert!(runs.data_as_u8_slice() == whole.data_as_u8_slice());
        // One layer on many threads paints its dab strokes side by side.
        assert!(render(&document, 2, 8).data_as_u8_slice() == whole.data_as_u8_slice());
    }

    #[test]
    fn without_wobble_only_an_animated_spray_moves() {
        let frames = |id: &str, wobble_scale: f32| {
            let mut document = Document::new([96, 72]);
            document.wobble = Wobble::classic(0.0);
            let mut brush = ugu_core::brush::find(id).unwrap().brush;
            brush.wobble_scale = wobble_scale;
            let spray = brushed(
                &mut document,
                brush,
                44.0,
                [0, 0, 0, 255],
                &[(18.0, 36.0, 1.0), (78.0, 36.0, 1.0)],
            );
            let document = with_ops(&document, vec![paint(spray)]);
            [render(&document, 0, 0), render(&document, 1, 0)]
        };
        let [a, b] = frames("pixel-spray", 1.0);
        assert!(a.data_as_u8_slice() == b.data_as_u8_slice());
        let [a, b] = frames("wobble-spray", 1.0);
        assert!(a.data_as_u8_slice() != b.data_as_u8_slice());
        let [a, b] = frames("wobble-spray", 0.0);
        assert!(a.data_as_u8_slice() == b.data_as_u8_slice());
    }

    #[test]
    fn a_marker_has_square_ends() {
        let mut document = Document::new([96, 48]);
        document.background = Rgba8([0, 0, 0, 0]);
        document.wobble = Wobble::classic(0.0);
        let end = |document: &mut Document, tip| {
            let brush = Brush {
                tip,
                size_dynamics: 0.0,
                ..Brush::default()
            };
            let stroke = brushed(
                document,
                brush,
                20.0,
                [0, 0, 0, 255],
                &[(20.0, 24.0, 1.0), (60.0, 24.0, 1.0)],
            );
            render(&with_ops(document, vec![paint(stroke)]), 0, 0)
        };
        let square = end(&mut document, ugu_core::store::TipShape::Square);
        let round = end(&mut document, ugu_core::store::TipShape::Round);
        // Past the end, near the edge: inside a square end, outside a round one.
        assert_eq!(at(&square, 68, 32)[3], 255);
        assert_eq!(at(&round, 68, 32)[3], 0);
        // Half a width past the end, nothing.
        assert_eq!(at(&square, 71, 24)[3], 0);
        assert_eq!(at(&square, 69, 24)[3], 255);
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
            (moved_work(), 3.0),
            (reframed_work(false), 3.0),
            // Resampling what is drawn smaller is not resampling what is
            // drawn at full size, as in 2.2.13: 5.4 at 1/2 and 8.5 at 1/4.
            (reframed_work(true), 10.0),
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
        for document in [many_layers(), reframed_work(true)] {
            over_the_budget(&document);
        }
    }

    fn over_the_budget(document: &Document) {
        let document = document.clone();
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
