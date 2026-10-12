// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Answers from the file thread that come after the work has moved on:
//! edits made while a save or a read runs, and a document replaced since.

use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use ugu_session::Session;

use super::*;
use crate::recovery::tests::Root;

fn setup() -> (Files, Canvas, Receiver<FileEvent>) {
    let (sender, events) = channel();
    let files = Files::new(0, move |event| {
        let _ = sender.send(event);
    });
    let canvas = Canvas::new(Document::new([8, 8]), |_| {});
    (files, canvas, events)
}

fn next(events: &Receiver<FileEvent>) -> FileEvent {
    events
        .recv_timeout(Duration::from_secs(10))
        .expect("the file thread answers")
}

fn layers_in(path: &Path) -> usize {
    let file = File::open(path).unwrap();
    ugu_io::read::read(std::io::BufReader::new(file))
        .unwrap()
        .0
        .layers
        .len()
}

/// As Save in the question about unsaved changes before `action`.
fn save_before(files: &mut Files, canvas: &mut Canvas, action: Action) {
    files.request(action, canvas);
    assert_eq!(files.confirm, Some(action));
    files.confirm = None;
    files.after_save = Some((None, action));
    files.save(canvas);
}

#[test]
fn edits_made_while_saving_before_closing_are_asked_about_again() {
    let root = Root::new("late-close");
    let (mut files, mut canvas, events) = setup();
    let path = root.0.join("drawing.ugurugu");
    files.path = Some(path.clone());
    canvas.edit(Session::add_layer).unwrap();
    save_before(&mut files, &mut canvas, Action::Close);
    canvas.edit(Session::add_layer).unwrap();
    files.handle(next(&events), &mut canvas);
    assert_eq!(layers_in(&path), 2);
    assert!(!files.should_close());
    assert_eq!(files.confirm, Some(Action::Close));
    assert_eq!(canvas.session().document().layers.len(), 3);
    assert!(canvas.session().is_dirty());

    // Without edits meanwhile, it closes once saved.
    files.confirm = None;
    files.after_save = Some((None, Action::Close));
    files.save(&mut canvas);
    files.handle(next(&events), &mut canvas);
    assert!(files.should_close());
    assert_eq!(layers_in(&path), 3);
    files.end();
}

#[test]
fn edits_made_while_opening_are_asked_about_before_it_is_shown() {
    let root = Root::new("late-open");
    let path = root.0.join("other.ugurugu");
    ugu_io::save::save(
        &Document::new([9, 9]),
        [7; 16],
        &path,
        &ugu_win::file::replace_file,
    )
    .unwrap();
    let (mut files, mut canvas, events) = setup();
    files.open_path(path.clone(), &canvas);
    canvas.edit(Session::add_layer).unwrap();
    // Nothing else replaces the document while it is read.
    files.request(Action::New, &mut canvas);
    assert_eq!(files.confirm, None);
    files.handle(next(&events), &mut canvas);
    assert_eq!(files.confirm, Some(Action::Replace));
    assert_eq!(canvas.session().document().canvas, [8, 8]);
    assert_eq!(canvas.session().document().layers.len(), 2);

    // Cancel keeps the edits and drops what was read.
    files.confirm = None;
    files.forget(Action::Replace);
    assert!(files.opened.is_none());
    assert_eq!(canvas.session().document().layers.len(), 2);

    // Don't Save shows it.
    files.open_path(path.clone(), &canvas);
    canvas.edit(Session::add_layer).unwrap();
    files.handle(next(&events), &mut canvas);
    assert_eq!(files.confirm, Some(Action::Replace));
    files.confirm = None;
    files.proceed(Action::Replace, &mut canvas);
    assert_eq!(canvas.session().document().canvas, [9, 9]);
    assert!(!canvas.session().is_dirty());
    assert_eq!(files.path, Some(path.clone()));

    // Unedited, it is shown at once.
    let (mut files, mut canvas, events) = setup();
    files.open_path(path, &canvas);
    files.handle(next(&events), &mut canvas);
    assert_eq!(files.confirm, None);
    assert_eq!(canvas.session().document().canvas, [9, 9]);
    files.end();
}

#[test]
fn a_late_save_answer_leaves_a_newer_document_alone() {
    let root = Root::new("late-save");
    let (mut files, mut canvas, events) = setup();
    let path = root.0.join("drawing.ugurugu");
    files.path = Some(path.clone());
    canvas.edit(Session::add_layer).unwrap();
    files.save(&mut canvas);
    canvas.edit(Session::add_layer).unwrap();
    files.save(&mut canvas);
    files.handle(next(&events), &mut canvas);
    // One save still runs, so nothing replaces the document yet.
    files.request(Action::New, &mut canvas);
    assert_eq!(files.confirm, None);
    assert!(files.new_document.is_none());
    files.handle(next(&events), &mut canvas);
    assert!(!canvas.session().is_dirty());
    files.request(Action::New, &mut canvas);
    assert!(files.new_document.is_some());

    // A save still running when the document is replaced anyway, as by the
    // new document's size being chosen, keeps to its own document.
    files.new_document = None;
    canvas.edit(Session::add_layer).unwrap();
    files.save(&mut canvas);
    files.start_new(&mut canvas, [5, 5]);
    files.handle(next(&events), &mut canvas);
    assert_eq!(files.path, None);
    assert_eq!(canvas.session().document().canvas, [5, 5]);
    assert_eq!(layers_in(&path), 4);
    files.end();
}

#[test]
fn an_image_read_for_a_document_replaced_since_is_not_inserted() {
    let (mut files, mut canvas, _events) = setup();
    let generation = canvas.generation();
    let image = ugu_io::import::from_rgba([2, 2], vec![255; 16], [8, 8]).unwrap();
    files.start_new(&mut canvas, [5, 5]);
    files.handle(
        FileEvent::Inserted(PathBuf::from("a.png"), generation, Ok(image.clone())),
        &mut canvas,
    );
    assert_eq!(canvas.session().document().layers.len(), 1);
    files.handle(
        FileEvent::Inserted(PathBuf::from("a.png"), canvas.generation(), Ok(image)),
        &mut canvas,
    );
    assert_eq!(canvas.session().document().layers.len(), 2);
    files.end();
}

#[test]
fn a_downloaded_update_waits_for_a_save_to_end_then_asks() {
    let root = Root::new("late-update");
    let (mut files, mut canvas, events) = setup();
    let ctx = egui::Context::default();
    let ask = |files: &mut Files, canvas: &mut Canvas| {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            files.ask_update(ui.ctx(), false, canvas);
        });
        output.textures_delta.clear();
    };
    files.path = Some(root.0.join("drawing.ugurugu"));
    canvas.edit(Session::add_layer).unwrap();
    files.save(&mut canvas);
    canvas.edit(Session::add_layer).unwrap();
    files.handle(
        FileEvent::Update(crate::update::Event::Downloaded(Ok(Box::default()))),
        &mut canvas,
    );
    ask(&mut files, &mut canvas);
    assert_eq!(files.confirm, None);
    files.handle(next(&events), &mut canvas);
    ask(&mut files, &mut canvas);
    assert_eq!(files.confirm, Some(Action::Update));
    // Cancelled, it is asked again from Check for Updates.
    files.confirm = None;
    ask(&mut files, &mut canvas);
    assert_eq!(files.confirm, None);
    files.check_for_updates();
    ask(&mut files, &mut canvas);
    assert_eq!(files.confirm, Some(Action::Update));
    files.end();
}
