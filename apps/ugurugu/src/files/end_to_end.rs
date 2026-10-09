// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! M2's end-to-end path without a window: strokes through the canvas input,
//! motion, save, reopen and PNG, each checked against the renderer.

use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use ugu_core::ops::Rgba8;
use ugu_render::document::{DocumentRenderer, Purpose as RenderPurpose};
use ugu_session::{Session, Tool};
use ugu_win::clock::Ticks;
use ugu_win::pointer::{Buttons, PointerKind, PointerSample};

use super::*;
use crate::cache::Rendered;
use crate::input::{CanvasInput, Gesture};

const SIZE: [u32; 2] = [320, 200];

fn sample(position: [f64; 2], time: u64) -> PointerSample {
    PointerSample {
        position,
        pressure: None,
        tilt: None,
        twist: None,
        buttons: Buttons::PRIMARY,
        in_contact: true,
        inverted: false,
        time: Ticks(time * 10_000),
        frame_id: time as u32,
    }
}

/// Takes cache renders until the canvas asks for no more. Waiting for a
/// pause instead gave up early on a busy machine.
fn settle(canvas: &mut Canvas, renders: &Receiver<Rendered>) {
    loop {
        canvas.sync();
        if !canvas.awaits_renders() {
            return;
        }
        let rendered = renders
            .recv_timeout(Duration::from_secs(10))
            .expect("the render worker answers");
        canvas.adopt(rendered);
    }
}

/// A wave across the canvas, as pointer input with the canvas at 100% in
/// the window's corner, so client pixels are document pixels.
fn stroke(canvas: &mut Canvas, renders: &Receiver<Rendered>, y: f64, phase: f64) {
    settle(canvas, renders);
    wave(canvas, y, phase);
}

/// `stroke` without waiting for renders first.
fn wave(canvas: &mut Canvas, y: f64, phase: f64) {
    let point = |step: u32| {
        let x = 20.0 + f64::from(step) * 7.0;
        [x, y + (f64::from(step) * 0.3 + phase).sin() * 25.0]
    };
    canvas.apply(CanvasInput::Begin(
        Gesture::Draw,
        PointerKind::Mouse,
        sample(point(0), 1),
    ));
    for step in 1..40 {
        canvas.apply(CanvasInput::Extend(sample(
            point(step),
            u64::from(step) + 1,
        )));
    }
    canvas.apply(CanvasInput::End(sample(point(40), 50)));
}

fn largest_difference(a: &[u8], b: &[u8]) -> u8 {
    a.iter().zip(b).map(|(a, b)| a.abs_diff(*b)).max().unwrap()
}

fn render(document: &Document, frame: i64, purpose: RenderPurpose) -> vello_cpu::Pixmap {
    let mut pixmap = vello_cpu::Pixmap::new(SIZE[0] as u16, SIZE[1] as u16);
    DocumentRenderer::new(0).render(document, frame, purpose, &mut pixmap);
    pixmap
}

#[test]
fn draw_move_save_reopen_and_export() {
    let (to_test, renders) = channel();
    let mut canvas = Canvas::new(Document::new(SIZE), move |rendered| {
        let _ = to_test.send(rendered);
    });
    let (to_files, file_events) = channel();
    let mut files = Files::new(0, move |event| {
        let _ = to_files.send(event);
    });
    let file_event = || {
        file_events
            .recv_timeout(Duration::from_secs(10))
            .expect("the file thread answers")
    };

    // Draw, erase across it, then draw translucent ink on a second layer.
    stroke(&mut canvas, &renders, 80.0, 0.0);
    canvas.edit(|session| session.set_tool(Tool::Eraser));
    canvas.edit(|session| session.eraser.width = 14.0);
    stroke(&mut canvas, &renders, 90.0, 2.0);
    canvas.edit(Session::add_layer).unwrap();
    canvas.edit(|session| {
        session.set_tool(Tool::Pen);
        session.pen.color = Rgba8([30, 90, 200, 140]);
        session.pen.antialias = true;
    });
    stroke(&mut canvas, &renders, 120.0, 1.0);
    let document = canvas.session().document().clone();
    assert_eq!(document.store.strokes.len(), 3);

    // What the canvas shows after adding strokes at pen-up is the frame a
    // full render gives, within the rounding of the split and the additions.
    let full = render(&document, 0, RenderPurpose::Display);
    let shown = largest_difference(canvas.display().data_as_u8_slice(), full.data_as_u8_slice());
    assert!(
        shown <= 3,
        "the canvas differs from a full render by {shown}"
    );

    // Another frame moves the strokes and is shown from a fresh split.
    canvas.edit(|session| session.set_frame(7));
    settle(&mut canvas, &renders);
    let moved = render(&document, 7, RenderPurpose::Display);
    assert!(largest_difference(moved.data_as_u8_slice(), full.data_as_u8_slice()) > 0);
    let shown = largest_difference(
        canvas.display().data_as_u8_slice(),
        moved.data_as_u8_slice(),
    );
    assert!(shown <= 2, "frame 7 differs from a full render by {shown}");

    // Save and open again into a new canvas.
    let folder = std::env::temp_dir().join(format!("ugurugu-end-to-end-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    files.path = Some(folder.join("drawing.ugurugu"));
    files.save(&mut canvas);
    files.handle(file_event(), &mut canvas);
    assert!(!canvas.session().is_dirty());

    let mut reopened = Canvas::new(Document::new([16, 16]), |_| {});
    files.open_path(folder.join("drawing.ugurugu"));
    files.handle(file_event(), &mut reopened);
    assert!(reopened.session().document() == &document);

    // The PNG of frame 7 is that frame as the renderer draws it for export.
    let png = folder.join("frame.png");
    let shared = Arc::new(reopened.session().document().clone());
    export(shared, 7, &png, &reopened.renders()).unwrap();
    let mut decoder =
        png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&png).unwrap()))
            .read_info()
            .unwrap();
    let mut decoded = vec![0; decoder.output_buffer_size().unwrap()];
    decoder.next_frame(&mut decoded).unwrap();
    let expected: Vec<u8> = render(&document, 7, RenderPurpose::Export)
        .data_as_u8_slice()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&pixel| ugu_io::image::unpremultiply(pixel))
        .collect();
    assert!(decoded == expected);
    let _ = std::fs::remove_dir_all(&folder);
}

/// The canvas shows what a full render of its document gives, within the
/// rounding of the split and the strokes added to it.
fn shows_the_document(canvas: &Canvas) {
    let document = canvas.session().document();
    let [width, height] = document.canvas.map(|edge| edge as u16);
    let mut full = vello_cpu::Pixmap::new(width, height);
    DocumentRenderer::new(0).render(document, 0, RenderPurpose::Display, &mut full);
    let shown = canvas.display();
    assert_eq!([shown.width(), shown.height()], [width, height]);
    let most = largest_difference(shown.data_as_u8_slice(), full.data_as_u8_slice());
    assert!(most <= 3, "the canvas differs from a full render by {most}");
}

#[test]
fn strokes_after_a_crop_or_a_resample_draw_on_the_new_canvas() {
    let (to_test, renders) = channel();
    let mut canvas = Canvas::new(Document::new(SIZE), move |rendered| {
        let _ = to_test.send(rendered);
    });
    stroke(&mut canvas, &renders, 80.0, 0.0);
    canvas
        .edit(|session| session.crop_canvas([-30, 20], [260, 240]))
        .unwrap();
    // Before the split of the new canvas arrives, partly below the canvas
    // before.
    wave(&mut canvas, 210.0, 1.0);
    settle(&mut canvas, &renders);
    shows_the_document(&canvas);
    canvas
        .edit(|session| session.resample_image([390, 300], ugu_core::ops::Sampling::Smooth))
        .unwrap();
    stroke(&mut canvas, &renders, 200.0, 2.0);
    settle(&mut canvas, &renders);
    shows_the_document(&canvas);
    assert_eq!(canvas.session().document().store.strokes.len(), 3);
    for _ in 0..4 {
        assert!(canvas.edit(Session::undo).unwrap());
    }
    settle(&mut canvas, &renders);
    assert_eq!(canvas.session().document().canvas, SIZE);
    shows_the_document(&canvas);
}

#[test]
fn a_render_of_the_document_before_an_open_is_not_taken() {
    let (to_test, renders) = channel();
    let mut canvas = Canvas::new(Document::new([120, 90]), move |rendered| {
        let _ = to_test.send(rendered);
    });
    canvas.sync();
    let old = renders
        .recv_timeout(Duration::from_secs(10))
        .expect("the first document renders");
    // The new document starts at the same revision, layer id and frame.
    canvas.replace(Document::new(SIZE), true);
    canvas.adopt(old);
    assert_eq!(
        [canvas.display().width(), canvas.display().height()],
        [SIZE[0] as u16, SIZE[1] as u16]
    );
    stroke(&mut canvas, &renders, 80.0, 0.0);
    assert_eq!(canvas.session().document().store.strokes.len(), 1);
}

#[test]
fn playback_shows_every_frame_in_order_as_soon_as_it_is_ready() {
    let (to_test, renders) = channel();
    let mut canvas = Canvas::new(Document::new(SIZE), move |rendered| {
        let _ = to_test.send(rendered);
    });
    stroke(&mut canvas, &renders, 80.0, 0.0);
    canvas.edit(|session| session.set_frame(28));
    canvas.toggle_playback();
    // A clock that runs far ahead of rendering: the frames still come one
    // after another, none skipped, each as soon as it is there.
    let mut now = std::time::Instant::now();
    let mut seen = Vec::new();
    while seen.len() < 6 {
        canvas.tick(now);
        let frame = canvas.session().frame();
        if seen.last() != Some(&frame) {
            seen.push(frame);
        }
        now += Duration::from_millis(500);
        if let Ok(rendered) = renders.recv_timeout(Duration::from_millis(200)) {
            canvas.adopt(rendered);
        }
    }
    assert_eq!(seen, [28, 29, 30, 31, 32, 33]);
}
