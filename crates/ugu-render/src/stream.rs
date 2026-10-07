// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Rendering when the surfaces of every layer would not fit in the memory
//! budget. A whole frame is put together one step at a time, each layer
//! drawn just before it is needed and let go after, as 2.2.13 did. For
//! editing, what does not depend on the edited layer is put together ahead
//! of time into a few surfaces the renderer keeps; the document's layers
//! stay as they are.

use std::sync::Arc;

use ugu_core::document::{Document, LayerId, LayerKind};
use ugu_core::ops::Blend;
use vello_cpu::Pixmap;

use crate::composite::{Kept, Put, Stream};
use crate::document::{DocumentRenderer, scaled_size};
use crate::plan::{Composite, RenderPlan, Step};
use crate::tile::{Pixel, TiledSurface};

/// A step's pixels kept by the editing split.
#[derive(Clone)]
pub enum Held {
    Tiles(Arc<TiledSurface>),
    Whole(Arc<Pixmap>),
}

impl From<Kept> for Held {
    fn from(kept: Kept) -> Self {
        match kept {
            Kept::Tiles(surface) => Self::Tiles(Arc::new(surface)),
            Kept::Whole(pixmap) => Self::Whole(Arc::new(pixmap)),
        }
    }
}

/// The plan as a tree of what is shown.
enum Node {
    Paint(LayerId, Composite),
    Group(Composite, Vec<Node>),
}

impl Node {
    fn composite(&self) -> Composite {
        match self {
            Self::Paint(_, composite) | Self::Group(composite, _) => *composite,
        }
    }
}

fn tree(steps: &[Step]) -> Vec<Node> {
    let mut levels: Vec<Vec<Node>> = vec![Vec::new()];
    for step in steps {
        match step {
            Step::Begin(_) => levels.push(Vec::new()),
            Step::End(_, composite) => {
                let children = levels.pop().expect("a group was begun");
                let parent = levels.last_mut().expect("the document level");
                parent.push(Node::Group(*composite, children));
            }
            Step::Paint(id, composite) => levels
                .last_mut()
                .expect("the document level")
                .push(Node::Paint(*id, *composite)),
        }
    }
    levels.pop().expect("the document level")
}

/// The child indices from the top that lead to paint layer `layer`.
fn path_to(nodes: &[Node], layer: LayerId) -> Option<Vec<usize>> {
    nodes
        .iter()
        .enumerate()
        .find_map(|(index, node)| match node {
            Node::Paint(id, _) => (*id == layer).then(|| vec![index]),
            Node::Group(_, children) => path_to(children, layer).map(|mut rest| {
                rest.insert(0, index);
                rest
            }),
        })
}

/// A program with its sources, and which source is the edited layer.
pub struct Program {
    pub puts: Vec<Put>,
    pub sources: Vec<Held>,
    pub edited: Option<usize>,
}

/// What the renderer needs to run nodes one step at a time.
struct Run<'a> {
    document: &'a Document,
    frame: u32,
    shrink: u32,
}

impl DocumentRenderer {
    /// Draws `frame` of `plan` one step at a time; `false` when stopped.
    pub(crate) fn render_streamed(
        &mut self,
        document: &Document,
        plan: &RenderPlan,
        frame: u32,
        shrink: u32,
        pixmap: &mut Pixmap,
    ) -> bool {
        let run = Run {
            document,
            frame,
            shrink,
        };
        let background = crate::compose::premultiplied(document.background.0);
        match self.stream(&run, &tree(&plan.steps), Some(background)) {
            Some(done) => {
                *pixmap = done;
                true
            }
            None => false,
        }
    }

    /// A program for editing `layer` that keeps a few surfaces put together
    /// ahead of time; `None` when stopped.
    pub(crate) fn reduced(
        &mut self,
        document: &Document,
        plan: &RenderPlan,
        frame: u32,
        layer: LayerId,
    ) -> Option<Program> {
        let run = Run {
            document,
            frame,
            shrink: 1,
        };
        let nodes = tree(&plan.steps);
        let background = crate::compose::premultiplied(document.background.0);
        let mut program = Program {
            puts: Vec::new(),
            sources: Vec::new(),
            edited: None,
        };
        match path_to(&nodes, layer) {
            Some(path) => {
                self.reduce(&run, &nodes, &path, Some(background), &mut program)?;
            }
            // Not shown: the frame as it is.
            None => {
                let whole = self.stream(&run, &nodes, Some(background))?;
                program.puts.push(Put::Backdrop);
                program.sources.push(Held::Whole(Arc::new(whole)));
            }
        }
        Some(program)
    }

    fn reduce(
        &mut self,
        run: &Run<'_>,
        siblings: &[Node],
        path: &[usize],
        background: Option<Pixel>,
        program: &mut Program,
    ) -> Option<()> {
        let at = path[0];
        let element = &siblings[at];
        let before = self.stream(run, &siblings[..at], background)?;
        program.puts.push(Put::Backdrop);
        program.sources.push(Held::Whole(Arc::new(before)));
        if element.composite().clipped
            && let Some(base) = siblings[..at]
                .iter()
                .rposition(|node| !node.composite().clipped)
        {
            let kept = self.alone(run, &siblings[base])?;
            program
                .puts
                .push(Put::Base(siblings[base].composite().opacity));
            program.sources.push(kept.into());
        }
        match element {
            Node::Paint(_, composite) => {
                let kept = self.alone(run, element)?;
                program.edited = Some(program.sources.len());
                program.puts.push(Put::Over(*composite));
                program.sources.push(kept.into());
            }
            Node::Group(composite, children) => {
                program.puts.push(Put::Begin);
                self.reduce(run, children, &path[1..], None, program)?;
                program.puts.push(Put::End(*composite));
            }
        }
        // What comes after: anything clipped to the element on its own, runs
        // of Normal siblings put together, and other blend modes on their
        // own with what is clipped to them.
        let rest = &siblings[at + 1..];
        let mut index = self.each_clipped(run, rest, 0, program)?;
        while index < rest.len() {
            let node = &rest[index];
            if node.composite().blend == Blend::Normal {
                let start = index;
                index += 1;
                while rest
                    .get(index)
                    .is_some_and(|node| node.composite().blend == Blend::Normal)
                {
                    index += 1;
                }
                let together = self.stream(run, &rest[start..index], None)?;
                program.puts.push(Put::Over(Composite {
                    blend: Blend::Normal,
                    opacity: 1.0,
                    clipped: false,
                }));
                program.sources.push(Held::Whole(Arc::new(together)));
            } else {
                let kept = self.alone(run, node)?;
                program.puts.push(Put::Over(node.composite()));
                program.sources.push(kept.into());
                index = self.each_clipped(run, rest, index + 1, program)?;
            }
        }
        Some(())
    }

    /// Puts the clipped nodes of `nodes` from `index` on into `program` on
    /// their own; returns the index after them.
    fn each_clipped(
        &mut self,
        run: &Run<'_>,
        nodes: &[Node],
        mut index: usize,
        program: &mut Program,
    ) -> Option<usize> {
        while let Some(node) = nodes.get(index).filter(|node| node.composite().clipped) {
            let kept = self.alone(run, node)?;
            program.puts.push(Put::Over(node.composite()));
            program.sources.push(kept.into());
            index += 1;
        }
        Some(index)
    }

    /// `node` alone on a transparent surface.
    fn alone(&mut self, run: &Run<'_>, node: &Node) -> Option<Kept> {
        match node {
            Node::Paint(id, _) => self.layer_alone(run, *id).map(Kept::Tiles),
            Node::Group(_, children) => self.stream(run, children, None).map(Kept::Whole),
        }
    }

    fn layer_alone(&mut self, run: &Run<'_>, id: LayerId) -> Option<TiledSurface> {
        if self.is_stopped() {
            return None;
        }
        let Some(LayerKind::Paint(paint)) = run.document.layer(id).map(|layer| &layer.kind) else {
            panic!("the plan was made from another document");
        };
        Some(self.draw_alone(run.document, run.frame, run.shrink, paint))
    }

    /// `nodes` as siblings over `background` (transparent without one).
    fn stream(
        &mut self,
        run: &Run<'_>,
        nodes: &[Node],
        background: Option<Pixel>,
    ) -> Option<Pixmap> {
        fn program(nodes: &[Node], puts: &mut Vec<Put>) {
            for node in nodes {
                match node {
                    Node::Paint(_, composite) => puts.push(Put::Over(*composite)),
                    Node::Group(composite, children) => {
                        puts.push(Put::Begin);
                        program(children, puts);
                        puts.push(Put::End(*composite));
                    }
                }
            }
        }
        let mut puts = Vec::new();
        program(nodes, &mut puts);
        let size = scaled_size(run.document.canvas, run.shrink);
        let mut stream = Stream::new(&puts, size, background, self.thread_count());
        self.feed(run, nodes, &mut stream)?;
        Some(stream.finish())
    }

    fn feed(&mut self, run: &Run<'_>, nodes: &[Node], stream: &mut Stream) -> Option<()> {
        for node in nodes {
            match node {
                Node::Paint(id, composite) => {
                    let surface = self.layer_alone(run, *id)?;
                    stream.step(Put::Over(*composite), Some(Kept::Tiles(surface)));
                }
                Node::Group(composite, children) => {
                    stream.step(Put::Begin, None);
                    self.feed(run, children, stream)?;
                    stream.step(Put::End(*composite), None);
                }
            }
        }
        Some(())
    }
}
