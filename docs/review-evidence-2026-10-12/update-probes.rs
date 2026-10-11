// SPDX-License-Identifier: GPL-3.0-or-later
// Review probes, appended to a module by run-probes.py; no product source changes.


#[cfg(test)]
mod review_audit {
    use super::*;

    #[test]
    fn downloaded_update_has_no_retry_after_install_was_deferred() {
        let mut updates = Updates::new(|_| panic!("no background work expected"));
        assert_eq!(updates.handle(Event::Downloaded(Ok(Box::default()))), Next::Install);
        // Files may decline Next::Install while saving or in a file dialog,
        // or the user can cancel its unsaved-changes confirmation.
        updates.check(true);
        println!("R06 ready={} checking={} offer={} notice={} busy={}",
            updates.ready.is_some(), updates.checking.is_some(), updates.offer.is_some(),
            updates.notice.is_some(), updates.busy());
        assert!(updates.ready.is_some());
        assert!(updates.checking.is_none() && updates.offer.is_none() && updates.notice.is_none());
    }
}
