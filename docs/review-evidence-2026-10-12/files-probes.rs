// SPDX-License-Identifier: GPL-3.0-or-later
// Review probes, appended to a module by run-probes.py; no product source changes.


#[cfg(test)]
mod review_audit {
    use super::*;
    use std::sync::mpsc::{Receiver, channel};
    use crate::recovery::tests::Root;

    fn setup() -> (Files, Canvas, Receiver<FileEvent>) {
        let (sender, events) = channel();
        let files = Files::new(0, move |event| { let _ = sender.send(event); });
        let canvas = Canvas::new(Document::new([8, 8]), |_| {});
        (files, canvas, events)
    }
    fn next(events: &Receiver<FileEvent>) -> FileEvent {
        events.recv_timeout(Duration::from_secs(20)).expect("file worker")
    }
    fn saved_layers(path: &Path) -> usize {
        ugu_io::read::read(File::open(path).unwrap()).unwrap().0.layers.len()
    }

    // These are observation probes: a passing assertion confirms the defect,
    // not the desired behavior. Production files are restored by run-probes.py.
    #[test]
    fn save_then_close_drops_edits_made_while_saving() {
        let root = Root::new("audit-close");
        let (mut files, mut canvas, events) = setup();
        let path = root.0.join("drawing.ugurugu");
        canvas.edit(Session::add_layer).unwrap();
        files.path = Some(path.clone());
        // State set by the unsaved-changes dialog's Save branch.
        files.after_save = Some(Action::Close);
        files.save(&mut canvas);
        assert!(!files.asking());
        canvas.edit(Session::add_layer).unwrap();
        files.handle(next(&events), &mut canvas);
        println!("R01 disk_layers={} live_layers={} dirty={} close={}",
            saved_layers(&path), canvas.session().document().layers.len(),
            canvas.session().is_dirty(), files.should_close());
        assert!(files.should_close() && canvas.session().is_dirty());
        assert_eq!(saved_layers(&path), 2);
        assert_eq!(canvas.session().document().layers.len(), 3);
        files.end();
    }

    #[test]
    fn open_completion_silently_replaces_new_edits() {
        let root = Root::new("audit-open");
        let path = root.0.join("other.ugurugu");
        ugu_io::save::save(&Document::new([9, 9]), [7; 16], &path,
            &ugu_win::file::replace_file).unwrap();
        let (mut files, mut canvas, events) = setup();
        // The user selected this path while the document was clean.
        files.open_path(path);
        assert!(!files.asking());
        canvas.edit(Session::add_layer).unwrap();
        assert!(canvas.session().is_dirty());
        files.handle(next(&events), &mut canvas);
        println!("R02 canvas={:?} layers={} dirty={} confirm={:?}",
            canvas.session().document().canvas, canvas.session().document().layers.len(),
            canvas.session().is_dirty(), files.confirm);
        assert_eq!(canvas.session().document().canvas, [9, 9]);
        assert!(!canvas.session().is_dirty());
        assert!(files.confirm.is_none());
        files.end();
    }

    #[test]
    fn recovered_original_is_deleted_even_if_handoff_write_fails() {
        let root = Root::new("audit-recovery");
        let crashed = root.0.join("crashed");
        let document = Document::new([8, 8]);
        let (lock, _) = recovery::begin(&root.0, &crashed).unwrap();
        let meta = Meta::new(1, "Recovered drawing".to_owned(), None, document.canvas);
        recovery::write(&crashed, &document, [3; 16], &meta).unwrap();
        drop(lock);
        let (mut files, mut canvas, events) = setup();
        files.start_recovery(Some(root.0.clone()));
        files.handle(next(&events), &mut canvas);
        let own = files.recovery.as_ref().unwrap().folder.clone();
        // Make writing the next generation fail without disk exhaustion.
        std::fs::create_dir(own.join("1.json")).unwrap();
        files.request(Action::Recover, &mut canvas);
        files.handle(next(&events), &mut canvas);
        let result = next(&events);
        assert!(matches!(result, FileEvent::RecoveryWritten(Err(_))));
        files.handle(result, &mut canvas);
        // FIFO barrier: reaches the worker after DiscardRecovery.
        files.open_path(root.0.join("missing.ugurugu"));
        assert!(matches!(next(&events), FileEvent::Opened(_, Err(_))));
        println!("R03 source_exists={} destination_document_exists={} dirty={}",
            crashed.exists(), own.join("1.ugurugu").exists(), canvas.session().is_dirty());
        assert!(!crashed.exists());
        assert!(!own.join("1.ugurugu").exists());
        files.end();
    }

    #[test]
    fn first_of_two_save_replies_unblocks_document_replacement() {
        let root = Root::new("audit-save-queue");
        let (mut files, mut canvas, events) = setup();
        let path = root.0.join("drawing.ugurugu");
        files.path = Some(path.clone());
        canvas.edit(Session::add_layer).unwrap();
        files.save(&mut canvas);
        canvas.edit(Session::add_layer).unwrap();
        files.save(&mut canvas);
        files.handle(next(&events), &mut canvas);
        assert!(!files.saving);
        // New/Discard/OK is now allowed although the second reply is pending.
        files.request(Action::New, &mut canvas);
        assert_eq!(files.confirm, Some(Action::New));
        files.confirm = None;
        files.start_new(&mut canvas, [5, 5]);
        files.handle(next(&events), &mut canvas);
        println!("R04 new_canvas={:?} old_path_attached={} dirty={}",
            canvas.session().document().canvas, files.path.as_ref() == Some(&path),
            canvas.session().is_dirty());
        assert_eq!(canvas.session().document().canvas, [5, 5]);
        assert_eq!(files.path, Some(path.clone()));
        files.save(&mut canvas);
        files.handle(next(&events), &mut canvas);
        assert_eq!(ugu_io::read::read(File::open(&path).unwrap()).unwrap().0.canvas, [5, 5]);
        files.end();
    }
}
