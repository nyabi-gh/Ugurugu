
#[cfg(test)]
mod review_audit {
    use super::*;

    #[test]
    fn another_window_overwrites_an_unrelated_setting() {
        let root = crate::recovery::tests::Root::new("audit-settings");
        let path = root.0.join("settings.json");
        let mut first = Store::open(Some(path.clone()));
        let mut second = Store::open(Some(path.clone()));
        first.change(|s| s.check_updates = false);
        drop(first); // joins writer; false is now durable
        assert!(!Store::open(Some(path.clone())).get().check_updates);
        second.change(|s| s.pixel_grid = false);
        drop(second); // writes its stale whole Settings value
        let final_store = Store::open(Some(path));
        println!("R12 check_updates={} pixel_grid={}",
            final_store.get().check_updates, final_store.get().pixel_grid);
        assert!(final_store.get().check_updates);
        assert!(!final_store.get().pixel_grid);
    }
}
