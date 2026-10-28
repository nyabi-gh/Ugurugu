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
    canvas.file_mut().path = Some(path.clone());
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
fn what_is_told_at_start_outlasts_opening_a_document() {
    let root = Root::new("told-at-start");
    let path = root.0.join("opened.ugurugu");
    ugu_io::save::save(
        &Document::new([9, 9]),
        [7; 16],
        &path,
        &ugu_platform::file::replace_file,
    )
    .unwrap();
    let (mut files, mut canvas, events) = setup();
    files.tell(
        "Default settings".to_owned(),
        "The settings could not be read".to_owned(),
    );
    files.open_path(path, &mut canvas);
    files.handle(next(&events), &mut canvas);
    assert_eq!(canvas.session().document().canvas, [9, 9]);
    let told = files.told.as_ref().map(|(_, text)| text.as_str());
    assert_eq!(told, Some("The settings could not be read"));
    files.end();
}

#[test]
fn a_document_read_while_the_shown_one_is_edited_opens_beside_it() {
    let root = Root::new("late-open");
    let path = root.0.join("other.ugurugu");
    ugu_io::save::save(
        &Document::new([9, 9]),
        [7; 16],
        &path,
        &ugu_platform::file::replace_file,
    )
    .unwrap();
    let (mut files, mut canvas, events) = setup();
    files.open_path(path.clone(), &mut canvas);
    canvas.edit(Session::add_layer).unwrap();
    // Nothing else starts while it is read.
    files.request(Action::New, &mut canvas);
    assert!(files.new_document.is_none());
    files.handle(next(&events), &mut canvas);
    assert_eq!(files.confirm, None);
    assert_eq!((canvas.tab_count(), canvas.tab_position()), (2, 1));
    assert_eq!(canvas.session().document().canvas, [9, 9]);
    assert!(!canvas.session().is_dirty());
    assert_eq!(canvas.file().path, Some(path.clone()));
    // The edited one waits in its tab as it was.
    assert!(canvas.tab_infos()[0].dirty);

    // Opening it again shows its tab without reading it.
    assert!(canvas.show_tab(0));
    files.open_path(path.clone(), &mut canvas);
    assert!(!files.busy());
    assert_eq!(canvas.tab_position(), 1);

    // A window holding nothing yet shows it in place of the empty document.
    let (mut files, mut canvas, events) = setup();
    files.open_path(path, &mut canvas);
    files.handle(next(&events), &mut canvas);
    assert_eq!(canvas.tab_count(), 1);
    assert_eq!(canvas.session().document().canvas, [9, 9]);
    files.end();
}

#[test]
fn a_late_save_answer_leaves_a_newer_document_alone() {
    let root = Root::new("late-save");
    let (mut files, mut canvas, events) = setup();
    let path = root.0.join("drawing.ugurugu");
    canvas.file_mut().path = Some(path.clone());
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

    // A save still running when another tab is shown, as a new document's,
    // is recorded on its own tab.
    files.new_document = None;
    canvas.edit(Session::add_layer).unwrap();
    files.save(&mut canvas);
    files.start_new(&mut canvas, [5, 5], false);
    files.handle(next(&events), &mut canvas);
    assert_eq!(canvas.file().path, None);
    assert_eq!(canvas.session().document().canvas, [5, 5]);
    assert_eq!(layers_in(&path), 4);
    let saved = &canvas.tab_infos()[0];
    assert_eq!((saved.name.as_str(), saved.dirty), ("drawing", false));
    files.end();
}

#[test]
fn an_image_read_for_a_document_replaced_since_is_not_inserted() {
    let (mut files, mut canvas, _events) = setup();
    let generation = canvas.generation();
    let image = ugu_io::import::from_rgba([2, 2], vec![255; 16], [8, 8]).unwrap();
    files.start_new(&mut canvas, [5, 5], false);
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
    canvas.file_mut().path = Some(root.0.join("drawing.ugurugu"));
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
