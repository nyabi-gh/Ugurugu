// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Automatic recovery through `Files`: when work is written and removed,
//! in order with saves, and recovering work found at start.

use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use ugu_core::ops::Op;
use ugu_session::Session;

use super::*;
use crate::recovery::tests::Root;

fn next(events: &Receiver<FileEvent>) -> FileEvent {
    events
        .recv_timeout(Duration::from_secs(10))
        .expect("the file thread answers")
}

/// Files keeping work under `root`, its recovery folder made.
fn setup(root: &Root) -> (Files, Canvas, Receiver<FileEvent>) {
    let (sender, events) = channel();
    let mut files = Files::new(0, move |event| {
        let _ = sender.send(event);
    });
    let mut canvas = Canvas::new(Files::new_canvas(), |_| {});
    files.start_recovery(Some(root.0.clone()));
    let begun = next(&events);
    assert!(matches!(begun, FileEvent::RecoveryBegun(Ok(_))));
    files.handle(begun, &mut canvas);
    (files, canvas, events)
}

fn folder(files: &Files) -> PathBuf {
    files.recovery.as_ref().unwrap().folder.clone()
}

/// The sequences of the generations on disk, waiting first for the jobs
/// already sent: a missing file opens only after them.
fn written(files: &mut Files, canvas: &mut Canvas, events: &Receiver<FileEvent>) -> Vec<u64> {
    files.open_path(PathBuf::from("missing.ugurugu"));
    loop {
        match next(events) {
            FileEvent::Opened(..) => break,
            event => files.handle(event, canvas),
        }
    }
    let folder = folder(files);
    let mut sequences: Vec<u64> = (0..2)
        .filter_map(|slot| {
            let text = std::fs::read(folder.join(format!("{slot}.json"))).ok()?;
            let value: serde_json::Value = serde_json::from_slice(&text).ok()?;
            value["sequence"].as_u64()
        })
        .collect();
    sequences.sort_unstable();
    sequences
}

#[test]
fn unsaved_work_is_written_once_editing_pauses_and_removed_once_saved() {
    let root = Root::new("files-pause");
    let (mut files, mut canvas, events) = setup(&root);
    let start = Instant::now();
    files.keep_recovery(&mut canvas, start);
    assert_eq!(files.recovery_due(), None, "nothing unsaved");
    canvas.edit(Session::add_layer).unwrap();
    files.keep_recovery(&mut canvas, start);
    assert_eq!(files.recovery_due(), Some(start + recovery::QUIET));
    files.keep_recovery(&mut canvas, start + recovery::QUIET / 2);
    assert_eq!(written(&mut files, &mut canvas, &events), [0u64; 0]);
    files.keep_recovery(&mut canvas, start + recovery::QUIET);
    assert_eq!(written(&mut files, &mut canvas, &events), [1]);
    // Written, nothing is due until the work changes again.
    files.keep_recovery(&mut canvas, start + recovery::LONGEST * 2);
    assert_eq!(files.recovery_due(), None);
    canvas.edit(Session::add_layer).unwrap();
    files.keep_recovery(&mut canvas, start + recovery::LONGEST * 2);
    files.keep_recovery(&mut canvas, start + recovery::LONGEST * 3);
    assert_eq!(written(&mut files, &mut canvas, &events), [1, 2]);
    // A save that fails keeps the work.
    files.path = Some(root.0.join("no such folder").join("drawing.ugurugu"));
    files.save(&mut canvas);
    files.handle(next(&events), &mut canvas);
    assert!(files.message().unwrap().starts_with("Not saved"));
    files.keep_recovery(&mut canvas, start + recovery::LONGEST * 4);
    assert_eq!(written(&mut files, &mut canvas, &events), [1, 2]);
    // One that succeeds removes it.
    files.path = Some(root.0.join("drawing.ugurugu"));
    files.save(&mut canvas);
    files.handle(next(&events), &mut canvas);
    files.keep_recovery(&mut canvas, start + recovery::LONGEST * 4);
    assert_eq!(written(&mut files, &mut canvas, &events), [0u64; 0]);
    // A normal close removes the folder.
    let own = folder(&files);
    assert!(own.exists());
    files.end();
    assert!(!own.exists());
}

#[test]
fn work_still_changing_is_written_within_the_longest_wait() {
    let root = Root::new("files-longest");
    let (mut files, mut canvas, events) = setup(&root);
    let start = Instant::now();
    let mut now = start;
    while now < start + recovery::LONGEST {
        canvas.edit(Session::add_layer).unwrap();
        files.keep_recovery(&mut canvas, now);
        now += recovery::QUIET / 2;
    }
    assert_eq!(written(&mut files, &mut canvas, &events), [0u64; 0]);
    files.keep_recovery(&mut canvas, start + recovery::LONGEST);
    assert_eq!(written(&mut files, &mut canvas, &events), [1]);
    files.end();
}

#[test]
fn a_write_queued_before_the_work_was_dropped_does_not_bring_it_back() {
    let root = Root::new("files-order");
    let (mut files, mut canvas, events) = setup(&root);
    let start = Instant::now();
    canvas.edit(Session::add_layer).unwrap();
    files.keep_recovery(&mut canvas, start);
    files.keep_recovery(&mut canvas, start + recovery::QUIET);
    // Undone to the state on disk before the write is done.
    canvas.edit(Session::undo).unwrap();
    files.keep_recovery(&mut canvas, start + recovery::QUIET);
    assert_eq!(written(&mut files, &mut canvas, &events), [0u64; 0]);
    files.end();
}

#[test]
fn a_pending_transform_is_written_in_and_stays_pending() {
    let root = Root::new("files-pending");
    let (mut files, mut canvas, events) = setup(&root);
    canvas.edit(Session::select_all);
    canvas.begin_transform();
    canvas.edit(|session| session.set_transform(Affine::translation(10.0, 0.0)));
    let start = Instant::now();
    files.keep_recovery(&mut canvas, start);
    files.keep_recovery(&mut canvas, start + recovery::QUIET);
    assert_eq!(written(&mut files, &mut canvas, &events), [1]);
    assert!(canvas.session().pending().is_some());
    let file = File::open(folder(&files).join("1.ugurugu")).unwrap();
    let (document, _) = ugu_io::read::read(std::io::BufReader::new(file)).unwrap();
    let ugu_core::document::LayerKind::Paint(paint) = &document.layers[0].kind else {
        panic!("a paint layer");
    };
    assert!(matches!(
        paint.ops.last(),
        Some(Op::TransformSelection { .. })
    ));
    // Moving it further is a change to write.
    canvas.edit(|session| session.set_transform(Affine::translation(20.0, 0.0)));
    let later = start + recovery::LONGEST;
    files.keep_recovery(&mut canvas, later);
    files.keep_recovery(&mut canvas, later + recovery::QUIET);
    assert_eq!(written(&mut files, &mut canvas, &events), [1, 2]);
    files.end();
}

#[test]
fn work_left_by_a_crash_is_recovered_under_a_new_name() {
    let root = Root::new("files-recover");
    // A window that crashed with a layer added to an unsaved drawing.
    let crashed = root.0.join("crashed");
    let (lock, _) = recovery::begin(&root.0, &crashed).unwrap();
    let mut session = Session::new(Files::new_canvas(), true);
    session.add_layer().unwrap();
    let document = session.document().clone();
    let meta = Meta::new(1, "Cat".to_owned(), None, document.canvas);
    recovery::write(&crashed, &document, [3; 16], &meta).unwrap();
    drop(lock);

    let (mut files, mut canvas, events) = setup(&root);
    assert_eq!(files.found.len(), 1);
    assert_eq!(files.found[0].meta.name, "Cat");
    files.request(Action::Recover, &mut canvas);
    files.handle(next(&events), &mut canvas);
    assert_eq!(canvas.session().document().layers.len(), 2);
    assert!(canvas.session().is_dirty());
    assert_eq!(files.title(&canvas), "Cat-recovered* - Ugurugu");
    assert_eq!(files.id, [3; 16]);
    // Kept in this window's folder, and the crashed one is gone.
    assert_eq!(written(&mut files, &mut canvas, &events), [1]);
    assert!(!crashed.exists());
    // Saving offers the new name, never the old file.
    assert_eq!(files.display_name(), "Cat-recovered");
    files.end();
}

#[test]
fn work_that_does_not_read_is_set_aside_and_said() {
    let root = Root::new("files-unreadable");
    let crashed = root.0.join("crashed");
    let (lock, _) = recovery::begin(&root.0, &crashed).unwrap();
    let meta = Meta::new(1, "Cat".to_owned(), None, [10, 10]);
    recovery::write(&crashed, &Files::new_canvas(), [3; 16], &meta).unwrap();
    drop(lock);
    std::fs::write(crashed.join("1.ugurugu"), b"not a document").unwrap();

    let (mut files, mut canvas, events) = setup(&root);
    files.request(Action::Recover, &mut canvas);
    files.handle(next(&events), &mut canvas);
    let said = files.message().unwrap();
    assert!(said.starts_with(tr("recovery-failed")), "{said}");
    assert!(!canvas.session().is_dirty());
    assert!(!crashed.exists());
    let aside = std::fs::read_dir(&root.0)
        .unwrap()
        .filter(|entry| {
            entry
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("failed-")
        })
        .count();
    assert_eq!(aside, 1);
    files.end();
}
