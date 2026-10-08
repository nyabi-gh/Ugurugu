// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Regression fixtures: documents made through the session as the app makes
//! them, saved, read back and drawn frame by frame, each frame pinned by its
//! digest. They cover what M4 must keep (docs/rust/m4-plan.md): a moved
//! selection moves each frame's own result, merging keeps every frame, an
//! eraser reaches only what came before it, and pending edits are applied in
//! order. Besides the digests, each fixture is checked for that meaning, so
//! new digests cannot quietly bless a wrong picture.
//!
//! Stroke seeds are random per session, so the documents are kept as files.
//! `UGU_BLESS=1 cargo test -p ugu-render --test regression` makes them anew
//! with their digests; review the pictures before committing them.

use std::fmt::Write as _;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest, Sha256};
use ugu_core::document::{Document, LayerId, LayerKind};
use ugu_core::ops::{Affine, Op, Rgba8, Sampling, Wobble};
use ugu_core::restyle::Style;
use ugu_core::selection::Combine;
use ugu_core::text::Outline;
use ugu_render::document::{DocumentRenderer, Purpose};
use ugu_session::{InputPoint, Session, ShapeKind, Tool};
use vello_cpu::Pixmap;

const CANVAS: [u32; 2] = [160, 120];
const FRAMES: u32 = 8;
const WHITE: [u8; 4] = [255; 4];
const BLUE: Rgba8 = Rgba8([0, 0, 255, 255]);

fn directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/regression")
}

fn blessing() -> bool {
    std::env::var_os("UGU_BLESS").is_some()
}

fn line(session: &mut Session, from: [f64; 2], to: [f64; 2]) {
    let steps = ((to[0] - from[0]).hypot(to[1] - from[1]) / 2.0)
        .ceil()
        .max(1.0);
    let at = |step: f64| InputPoint {
        position: [
            from[0] + (to[0] - from[0]) * step / steps,
            from[1] + (to[1] - from[1]) * step / steps,
        ],
        pressure: None,
        time: step * 8.0,
    };
    session.begin_stroke(at(0.0)).expect("a shown paint layer");
    for step in 1..steps as u32 {
        session.extend_stroke(at(f64::from(step)));
    }
    session.end_stroke(at(steps)).expect("valid");
}

fn select(session: &mut Session, kind: ShapeKind, from: [f64; 2], to: [f64; 2]) {
    session.selection_shape = kind;
    session.begin_selection(from, Combine::Replace);
    session.end_selection(to).expect("valid");
}

fn new_session() -> Session {
    let mut session = Session::new(Document::new(CANVAS), false);
    session
        .set_animation(FRAMES, 12.0, Wobble::classic(1.6))
        .expect("valid");
    session
}

fn pixels(document: &Document, frame: u32, threads: u16) -> Pixmap {
    let [width, height] = document.canvas.map(|edge| edge as u16);
    let mut pixmap = Pixmap::new(width, height);
    DocumentRenderer::new(threads).render(
        document,
        i64::from(frame),
        Purpose::Display,
        &mut pixmap,
    );
    pixmap
}

fn at(pixmap: &Pixmap, x: i32, y: i32) -> [u8; 4] {
    let index = y as usize * usize::from(pixmap.width()) + x as usize;
    pixmap.data_as_u8_slice().as_chunks::<4>().0[index]
}

/// The document with only the first `count` operations of `layer`, a
/// top-level layer.
fn before(document: &Document, layer: LayerId, count: usize) -> Document {
    let mut earlier = document.clone();
    let (_, index) = earlier.position(layer).expect("there");
    if let LayerKind::Paint(paint) = &mut earlier.layers[index].kind {
        paint.ops.truncate(count);
    }
    earlier
}

fn ops(document: &Document, layer: LayerId) -> &[Op] {
    match &document.layer(layer).expect("there").kind {
        LayerKind::Paint(paint) => &paint.ops,
        LayerKind::Group(_) => unreachable!(),
    }
}

/// Strokes on a wobbling layer, part of them selected and moved by a whole
/// number of pixels, then the moved part copied and turned.
fn selection_frames() -> Document {
    let mut session = new_session();
    for y in [30.0, 45.0, 60.0, 75.0] {
        line(&mut session, [10.0, y], [100.0, y + 8.0]);
    }
    line(&mut session, [15.0, 95.0], [90.0, 20.0]);
    select(
        &mut session,
        ShapeKind::Rectangle,
        [20.0, 20.0],
        [70.0, 90.0],
    );
    session.pen.color = Rgba8([220, 0, 0, 255]);
    line(&mut session, [5.0, 52.0], [150.0, 52.0]);
    session.begin_transform().expect("a selection");
    session.set_transform(Affine::translation(60.0, 10.0));
    session.apply_transform().expect("valid");
    session.begin_transform().expect("a selection");
    session.set_transform(Affine::rotation_about(0.5, [110.0, 65.0]));
    session.set_keep_source(true);
    session.apply_transform().expect("valid");
    session.document().clone()
}

/// The first move is by whole pixels, so on every frame each selected pixel
/// of that frame lands exactly where it was moved to, and what it left is
/// clear.
fn check_selection_frames(document: &Document) {
    let layer = document.layers[0].id;
    let moves: Vec<usize> = ops(document, layer)
        .iter()
        .enumerate()
        .filter(|(_, op)| matches!(op, Op::TransformSelection { .. }))
        .map(|(index, _)| index)
        .collect();
    let Op::TransformSelection {
        mask, transform, ..
    } = &ops(document, layer)[moves[0]]
    else {
        unreachable!();
    };
    let [dx, dy] = transform.apply([0.0, 0.0]).map(|value| value as i32);
    let mask = &document.store.masks[mask];
    let from = before(document, layer, moves[0]);
    let to = before(document, layer, moves[0] + 1);
    let mut first = None;
    for frame in 0..FRAMES {
        let (a, b) = (pixels(&from, frame, 0), pixels(&to, frame, 0));
        let [left, top, width, height] = mask.bounds;
        let mut moved = 0;
        for y in top..top + height {
            for x in left..left + width {
                if !mask.contains(x, y) {
                    continue;
                }
                let source = at(&a, x, y);
                if source != WHITE {
                    assert_eq!(at(&b, x + dx, y + dy), source, "frame {frame} at {x}, {y}");
                    moved += 1;
                }
                if !mask.contains(x - dx, y - dy) {
                    assert_eq!(at(&b, x, y), WHITE, "frame {frame}: {x}, {y} is left clear");
                }
            }
        }
        assert!(moved > 100, "frame {frame} moves strokes");
        match &first {
            None => first = Some(a.data_as_u8_slice().to_vec()),
            Some(first) => assert_ne!(first, a.data_as_u8_slice(), "frame {frame} wobbles"),
        }
    }
}

/// Two layers with their own erasers, the upper one see-through, merged.
fn merge() -> (Document, Document) {
    let mut session = new_session();
    for y in [30.0, 60.0, 90.0] {
        line(&mut session, [10.0, y], [150.0, y]);
    }
    session.set_tool(Tool::Eraser);
    session.eraser.width = 14.0;
    line(&mut session, [40.0, 10.0], [40.0, 110.0]);
    session.set_tool(Tool::Pen);
    session.add_layer().expect("valid");
    let upper = session.current_layer();
    session
        .update_layer(upper, "Opacity", |layer| {
            if let LayerKind::Paint(paint) = &mut layer.kind {
                paint.opacity = 0.6;
            }
        })
        .expect("valid");
    session.pen.color = BLUE;
    session.pen.width = 10.0;
    for x in [60.0, 100.0, 130.0] {
        line(&mut session, [x, 10.0], [x, 110.0]);
    }
    session.set_tool(Tool::Eraser);
    line(&mut session, [10.0, 60.0], [150.0, 60.0]);
    let separate = session.document().clone();
    session.merge_down().expect("mergeable");
    (separate, session.document().clone())
}

fn check_merge(separate: &Document, merged: &Document) {
    assert_eq!(merged.layers.len(), 1);
    for frame in 0..FRAMES {
        let (a, b) = (pixels(separate, frame, 0), pixels(merged, frame, 0));
        let most = a
            .data_as_u8_slice()
            .iter()
            .zip(b.data_as_u8_slice())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        assert!(most <= 1, "frame {frame} differs by {most} after merging");
    }
}

/// A line, an eraser across it, and a blue line drawn over the eraser.
fn erase() -> Document {
    let mut session = new_session();
    session.pen.width = 10.0;
    line(&mut session, [10.0, 40.0], [150.0, 40.0]);
    line(&mut session, [10.0, 80.0], [150.0, 80.0]);
    session.set_tool(Tool::Eraser);
    session.eraser.width = 20.0;
    line(&mut session, [80.0, 15.0], [80.0, 105.0]);
    session.set_tool(Tool::Pen);
    session.pen.color = BLUE;
    session.pen.width = 12.0;
    line(&mut session, [10.0, 60.0], [150.0, 60.0]);
    session.document().clone()
}

fn check_erase(document: &Document) {
    for frame in 0..FRAMES {
        let shown = pixels(document, frame, 0);
        assert_eq!(
            at(&shown, 30, 40),
            [0, 0, 0, 255],
            "frame {frame}: the line"
        );
        assert_eq!(at(&shown, 80, 40), WHITE, "frame {frame}: erased");
        assert_eq!(at(&shown, 80, 80), WHITE, "frame {frame}: erased");
        assert_eq!(
            at(&shown, 80, 60),
            [0, 0, 255, 255],
            "frame {frame}: drawn after"
        );
    }
}

/// A turned and scaled copy left pending when the next stroke is drawn, and
/// text left placed when the tool changes: both are applied first, and both
/// are cut to the selection as it was moved.
fn pending() -> Document {
    let mut session = new_session();
    line(&mut session, [20.0, 30.0], [90.0, 50.0]);
    line(&mut session, [30.0, 90.0], [80.0, 25.0]);
    select(&mut session, ShapeKind::Ellipse, [15.0, 20.0], [95.0, 95.0]);
    session.transform_sampling = Sampling::Smooth;
    session.begin_transform().expect("a selection");
    let turn = Affine::rotation_about(0.52, [55.0, 57.0])
        .then(Affine::scaling_about([1.2, 1.2], [55.0, 57.0]))
        .then(Affine::translation(45.0, 5.0));
    session.set_transform(turn);
    session.set_keep_source(true);
    session.pen.color = BLUE;
    line(&mut session, [10.0, 60.0], [150.0, 75.0]);
    session.set_tool(Tool::Text);
    session.text.filled = true;
    let outline = Outline {
        contours: vec![
            vec![[0.0, 0.0], [30.0, 0.0], [30.0, 16.0], [0.0, 16.0]],
            vec![[8.0, 4.0], [8.0, 12.0], [22.0, 12.0], [22.0, 4.0]],
        ],
        size: [30.0, 16.0],
    };
    session
        .place_text([80.0, 30.0], Arc::new(outline))
        .expect("paintable");
    session.set_tool(Tool::Pen);
    session.document().clone()
}

fn check_pending(document: &Document) {
    let kinds: Vec<&str> = ops(document, document.layers[0].id)
        .iter()
        .map(|op| match op {
            Op::Paint { .. } => "paint",
            Op::Fill { .. } => "fill",
            Op::TransformSelection {
                keep_source: true, ..
            } => "copy",
            _ => "other",
        })
        .collect();
    // Two lines, the copy, the line that applied it, then the text: its
    // fill and one closed stroke per outline.
    assert_eq!(
        kinds,
        ["paint", "paint", "copy", "paint", "fill", "paint", "paint"]
    );
    // The line is cut to where the copy went, not to where it came from.
    let layer_ops = ops(document, document.layers[0].id);
    let (
        Op::TransformSelection { mask: source, .. },
        Op::Paint {
            clip: Some(clip), ..
        },
    ) = (&layer_ops[2], &layer_ops[3])
    else {
        panic!("a copy, then a clipped line");
    };
    let bounds = |id| document.store.masks[id].bounds;
    assert!(
        bounds(clip)[0] > bounds(source)[0] + 30,
        "the moved selection"
    );
}

/// A filled selection, a crop, a resample and strokes between them, then
/// everything touched given a new colour and width.
fn canvas() -> Document {
    let mut session = new_session();
    line(&mut session, [10.0, 20.0], [150.0, 100.0]);
    select(
        &mut session,
        ShapeKind::Rectangle,
        [30.0, 30.0],
        [90.0, 70.0],
    );
    session.pen.color = Rgba8([0, 160, 60, 255]);
    session.fill_selection().expect("a selection");
    session.deselect();
    session.crop_canvas([-10, -5], [150, 110]).expect("valid");
    session.pen.color = BLUE;
    line(&mut session, [5.0, 100.0], [145.0, 10.0]);
    session
        .resample_image([120, 88], Sampling::Smooth)
        .expect("valid");
    session.pen.color = Rgba8([0, 0, 0, 255]);
    line(&mut session, [10.0, 44.0], [110.0, 44.0]);
    select(&mut session, ShapeKind::Rectangle, [0.0, 0.0], [60.0, 88.0]);
    session
        .restyle_selected(Style {
            color: Some(Rgba8([150, 0, 170, 255])),
            width: Some(9.0),
        })
        .expect("strokes there");
    session.document().clone()
}

fn check_canvas(document: &Document) {
    assert_eq!(document.canvas, [120, 88]);
    let layer = document.layers[0].id;
    let purple = ops(document, layer)
        .iter()
        .filter(|op| match op {
            Op::Paint { stroke, .. } => {
                document.store.strokes[stroke].color == Rgba8([150, 0, 170, 255])
            }
            Op::Fill { color, .. } => *color == Rgba8([150, 0, 170, 255]),
            _ => false,
        })
        .count();
    // Every stroke and the fill reach the left half.
    assert_eq!(purple, 4);
}

fn digests(document: &Document) -> Vec<String> {
    (0..FRAMES)
        .map(|frame| {
            let one = pixels(document, frame, 0);
            let eight = pixels(document, frame, 8);
            assert_eq!(
                one.data_as_u8_slice(),
                eight.data_as_u8_slice(),
                "frame {frame} on 8 threads"
            );
            let digest = Sha256::digest(one.data_as_u8_slice());
            digest.iter().fold(String::new(), |mut out, byte| {
                let _ = write!(out, "{byte:02x}");
                out
            })
        })
        .collect()
}

fn save(name: &str, document: &Document) {
    let bytes = ugu_io::write::write(document, [7; 16], Cursor::new(Vec::new()))
        .expect("valid")
        .into_inner();
    std::fs::write(directory().join(format!("{name}.ugurugu")), bytes).expect("writable");
}

fn read(name: &str) -> Document {
    let path = directory().join(format!("{name}.ugurugu"));
    let bytes = std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "{}: {error}; make the fixtures with UGU_BLESS=1",
            path.display()
        )
    });
    ugu_io::read::read(Cursor::new(bytes)).expect("readable").0
}

#[test]
fn fixtures_draw_as_pinned_and_keep_their_meaning() {
    let goldens = directory().join("digests.txt");
    if blessing() {
        std::fs::create_dir_all(directory()).expect("writable");
        let (separate, merged) = merge();
        for (name, document) in [
            ("selection-frames", selection_frames()),
            ("merge-separate", separate),
            ("merge-merged", merged),
            ("erase", erase()),
            ("pending", pending()),
            ("canvas", canvas()),
        ] {
            save(name, &document);
        }
    }
    let names = [
        "selection-frames",
        "merge-separate",
        "merge-merged",
        "erase",
        "pending",
        "canvas",
    ];
    let documents: Vec<Document> = names.iter().map(|name| read(name)).collect();
    check_selection_frames(&documents[0]);
    check_merge(&documents[1], &documents[2]);
    check_erase(&documents[3]);
    check_pending(&documents[4]);
    check_canvas(&documents[5]);

    if let Some(out) = std::env::var_os("UGU_PICTURES") {
        for (name, document) in names.iter().zip(&documents) {
            let shown = pixels(document, 0, 0);
            let file = std::fs::File::create(Path::new(&out).join(format!("{name}.png")))
                .expect("writable");
            let mut encoder =
                png::Encoder::new(file, u32::from(shown.width()), u32::from(shown.height()));
            encoder.set_color(png::ColorType::Rgba);
            encoder
                .write_header()
                .and_then(|mut writer| writer.write_image_data(shown.data_as_u8_slice()))
                .expect("writable");
        }
    }
    let mut drawn = String::new();
    for (name, document) in names.iter().zip(&documents) {
        for (frame, digest) in digests(document).iter().enumerate() {
            let _ = writeln!(drawn, "{name} {frame} {digest}");
        }
    }
    if blessing() {
        std::fs::write(&goldens, &drawn).expect("writable");
        return;
    }
    let pinned = std::fs::read_to_string(&goldens)
        .expect("digests.txt; make the fixtures with UGU_BLESS=1")
        .replace("\r\n", "\n");
    for (want, got) in pinned.lines().zip(drawn.lines()) {
        assert_eq!(got, want, "drawn differently");
    }
    assert_eq!(pinned.lines().count(), drawn.lines().count());
}
