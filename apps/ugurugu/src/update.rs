// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Checking for, downloading and applying updates through Velopack, as
//! 2.2.13 did (`src/app/UpdateControllerWindows.cpp`): "Check for
//! Updates…" in the Help menu, Update or Cancel when one is found, and
//! installing it once the window has closed. Unlike 2.2.13, which checked
//! at start only once six hours had passed since its last check, it checks
//! two seconds after every start and every six hours while it runs, so a
//! release is offered the next time the app starts; a check asked for while
//! an automatic one runs shows that check's result (D18), and the automatic
//! check can be turned off.
//!
//! The checks and the download run on threads of their own, so the file
//! thread's saves never wait for the network.

use std::sync::Arc;
use std::time::{Duration, Instant};

use velopack::sources::{FileSource, HttpSource};
use velopack::{UpdateCheck, UpdateInfo, UpdateManager};

use crate::i18n::{args, tr, tr_with};

/// The update packages and their list, kept on one GitHub release of their
/// own so that each version's release holds only its installer. Downloads
/// from there are counted with the rest (`.github/scripts`).
const FEED: &str = "https://github.com/nyabi-gh/Ugurugu/releases/download/updates";
const START_DELAY: Duration = Duration::from_secs(2);
const INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

pub enum Event {
    Checked(Result<Found, String>),
    Downloaded(Result<Box<UpdateInfo>, String>),
}

pub enum Found {
    Update(Box<UpdateInfo>),
    /// The version running is the newest.
    Latest(String),
    /// Not run from an installation, as when built from source.
    NotInstalled,
}

pub struct Updates {
    automatic: bool,
    due: Option<Instant>,
    /// Whether the check under way shows its result, while one is.
    checking: Option<bool>,
    downloading: bool,
    offer: Option<Box<UpdateInfo>>,
    ready: Option<Box<UpdateInfo>>,
    /// The downloaded update waits for the window to deal with unsaved
    /// changes, then `apply`.
    install: bool,
    /// A title and a text to show until dismissed.
    notice: Option<(String, String)>,
    events: Arc<dyn Fn(Event) + Send + Sync>,
}

/// The updates release, or the folder of packages `UGURUGU_UPDATE_FEED`
/// names, to try an update before it is published.
fn manager() -> Result<UpdateManager, velopack::Error> {
    match std::env::var_os("UGURUGU_UPDATE_FEED") {
        Some(folder) => UpdateManager::new(FileSource::new(folder), None, None),
        None => UpdateManager::new(HttpSource::new(FEED), None, None),
    }
}

fn check() -> Result<Found, String> {
    let manager = match manager() {
        Ok(manager) => manager,
        Err(velopack::Error::NotInstalled(_)) => return Ok(Found::NotInstalled),
        Err(error) => return Err(error.to_string()),
    };
    match manager
        .check_for_updates()
        .map_err(|error| error.to_string())?
    {
        UpdateCheck::UpdateAvailable(update) => Ok(Found::Update(update)),
        UpdateCheck::NoUpdateAvailable | UpdateCheck::RemoteIsEmpty => {
            Ok(Found::Latest(manager.get_current_version_as_string()))
        }
    }
}

impl Updates {
    pub fn new(events: impl Fn(Event) + Send + Sync + 'static) -> Self {
        Self {
            automatic: false,
            due: None,
            checking: None,
            downloading: false,
            offer: None,
            ready: None,
            install: false,
            notice: None,
            events: Arc::new(events),
        }
    }

    /// Follows the settings: whether to check by itself, first shortly
    /// after start or after being turned on.
    pub fn follow(&mut self, automatic: bool) {
        self.automatic = automatic;
        if !automatic {
            self.due = None;
        } else if self.due.is_none() {
            self.due = Some(Instant::now() + START_DELAY);
        }
    }

    /// When to wake for the automatic check.
    pub fn due(&self) -> Option<Instant> {
        self.due
    }

    /// Starts the automatic check once it is due, and makes the next one
    /// due an interval later; one that cannot start for an update already
    /// offered or downloading waits for the next.
    pub fn tick(&mut self, now: Instant) {
        if self.due.is_some_and(|due| due <= now) {
            self.due = Some(now + INTERVAL);
            self.check(false);
        }
    }

    pub fn busy(&self) -> bool {
        self.downloading
    }

    /// Checks now; `interactive` shows the result whatever it is. With an
    /// update downloaded and not installed, asks to install it again.
    pub fn check(&mut self, interactive: bool) {
        if self.ready.is_some() {
            self.install |= interactive;
            return;
        }
        if self.downloading || self.offer.is_some() {
            return;
        }
        if let Some(shows) = self.checking.as_mut() {
            *shows |= interactive;
            return;
        }
        self.checking = Some(interactive);
        let events = self.events.clone();
        let spawned = std::thread::Builder::new()
            .name("update check".to_owned())
            .spawn(move || events(Event::Checked(check())));
        if let Err(error) = spawned {
            self.checking = None;
            tracing::warn!(%error, "cannot start the update check");
        }
    }

    /// Whether the downloaded update is to be installed now, once.
    pub fn take_install(&mut self) -> bool {
        std::mem::take(&mut self.install)
    }

    pub fn handle(&mut self, event: Event) {
        match event {
            Event::Checked(result) => {
                let interactive = self.checking.take().unwrap_or(false);
                match result {
                    Ok(Found::Update(update)) => {
                        tracing::info!(version = update.TargetFullRelease.Version, "update found");
                        self.offer = Some(update);
                    }
                    Ok(Found::Latest(version)) => {
                        tracing::info!(version, "no update");
                        if interactive {
                            self.notice = Some((
                                tr("update-latest-title").to_owned(),
                                tr_with("update-latest", &args([("version", version)])),
                            ));
                        }
                    }
                    Ok(Found::NotInstalled) => {
                        tracing::debug!("not installed; no update check");
                        if interactive {
                            self.notice = Some((
                                tr("update-failed").to_owned(),
                                tr("update-not-installed").to_owned(),
                            ));
                        }
                    }
                    Err(error) => {
                        tracing::warn!(error, "update check failed");
                        if interactive {
                            self.notice = Some((
                                tr("update-failed").to_owned(),
                                format!("{}\n\n{error}", tr("update-check-failed")),
                            ));
                        }
                    }
                }
            }
            Event::Downloaded(result) => {
                self.downloading = false;
                match result {
                    Ok(update) => {
                        self.ready = Some(update);
                        self.install = true;
                    }
                    Err(error) => {
                        tracing::warn!(error, "update download failed");
                        self.notice = Some((
                            tr("update-failed").to_owned(),
                            format!("{}\n\n{error}", tr("update-download-failed")),
                        ));
                    }
                }
            }
        }
    }

    fn download(&mut self, update: Box<UpdateInfo>) {
        self.downloading = true;
        let events = self.events.clone();
        let spawned = std::thread::Builder::new()
            .name("update download".to_owned())
            .spawn(move || {
                let result = manager()
                    .and_then(|manager| manager.download_updates(&update, None))
                    .map(|()| update)
                    .map_err(|error| error.to_string());
                events(Event::Downloaded(result));
            });
        if let Err(error) = spawned {
            self.downloading = false;
            tracing::warn!(%error, "cannot start the update download");
        }
    }

    /// Hands the downloaded update to Velopack's updater, which installs it
    /// once this process has ended and starts the new version. The window
    /// must close at once after: the updater waits a minute at most. One
    /// that fails stays downloaded, to try again.
    pub fn apply(&mut self) -> Result<(), String> {
        let Some(update) = self.ready.as_deref() else {
            return Err("no update is downloaded".to_owned());
        };
        manager()
            .and_then(|manager| {
                manager.wait_exit_then_apply_updates(update, false, true, Vec::<String>::new())
            })
            .map(|()| self.ready = None)
            .map_err(|error| {
                let error = error.to_string();
                self.notice = Some((
                    tr("update-failed").to_owned(),
                    format!("{}\n\n{error}", tr("update-install-failed")),
                ));
                error
            })
    }

    /// The update question or a notice, while one is open and nothing else
    /// is asked.
    pub fn ask(&mut self, ctx: &egui::Context, asking: bool) {
        if asking {
            return;
        }
        if let Some(update) = &self.offer {
            let version = update.TargetFullRelease.Version.clone();
            let mut answer = None;
            let offered = tr_with("update-available", &args([("version", version)]));
            let modal = crate::widgets::dialog(
                ctx,
                "update offer",
                400.0,
                tr("update-available-title"),
                Some(&offered),
                |ui| {
                    ui.label(tr("update-ask"));
                    crate::widgets::dialog_buttons(ui, |ui| {
                        use crate::widgets::{Emphasis, button};
                        if button(ui, tr("update-install"), Emphasis::Primary, true).clicked() {
                            answer = Some(true);
                        }
                        if button(ui, tr("unsaved-cancel"), Emphasis::Secondary, true).clicked() {
                            answer = Some(false);
                        }
                    });
                },
            );
            if modal.should_close() && answer.is_none() {
                answer = Some(false);
            }
            match answer {
                Some(true) => {
                    let update = self.offer.take().expect("shown above");
                    self.download(update);
                }
                Some(false) => self.offer = None,
                None => {}
            }
            return;
        }
        if let Some((title, text)) = &self.notice {
            let mut close = false;
            let modal =
                crate::widgets::dialog(ctx, "update notice", 400.0, title, Some(text), |ui| {
                    crate::widgets::dialog_buttons(ui, |ui| {
                        close = crate::widgets::button(
                            ui,
                            tr("crash-close"),
                            crate::widgets::Emphasis::Primary,
                            true,
                        )
                        .clicked();
                    });
                });
            if close || modal.should_close() {
                self.notice = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_start_checks_soon_and_then_every_interval() {
        let mut updates = Updates::new(|_| {});
        let started = Instant::now();
        updates.follow(true);
        let first = updates.due.unwrap();
        assert!(first <= started + START_DELAY + Duration::from_secs(1));
        // The settings followed again change nothing.
        updates.follow(true);
        assert_eq!(updates.due, Some(first));
        // An update offered keeps the check from starting, and the next
        // is due an interval later all the same.
        updates.offer = Some(Box::default());
        updates.tick(first);
        assert!(updates.checking.is_none());
        assert_eq!(updates.due, Some(first + INTERVAL));
        updates.tick(first + INTERVAL - Duration::from_secs(1));
        assert_eq!(updates.due, Some(first + INTERVAL));
    }

    #[test]
    fn a_check_asked_for_during_an_automatic_one_shows_its_result() {
        let mut updates = Updates::new(|_| {});
        updates.checking = Some(false);
        updates.check(true);
        assert_eq!(updates.checking, Some(true));
        updates.handle(Event::Checked(Ok(Found::Latest("3.0.0".to_owned()))));
        assert!(updates.notice.is_some());
    }

    #[test]
    fn an_automatic_check_that_fails_says_nothing() {
        let mut updates = Updates::new(|_| {});
        updates.checking = Some(false);
        updates.handle(Event::Checked(Err("offline".to_owned())));
        assert!(updates.notice.is_none());
    }

    #[test]
    fn turning_the_automatic_check_off_cancels_the_one_due() {
        let mut updates = Updates::new(|_| {});
        updates.follow(true);
        assert!(updates.due.is_some());
        updates.follow(false);
        assert!(updates.due.is_none());
        updates.follow(true);
        assert!(updates.due.is_some());
    }

    #[test]
    fn a_downloaded_update_not_installed_is_asked_about_again() {
        let mut updates = Updates::new(|_| panic!("nothing runs in the background"));
        updates.handle(Event::Downloaded(Ok(Box::default())));
        assert!(updates.take_install());
        // Not installed then: the window was busy, or the user cancelled.
        assert!(!updates.take_install());
        updates.check(false);
        assert!(!updates.take_install());
        updates.check(true);
        assert!(updates.take_install());
        assert!(updates.ready.is_some());
        assert!(updates.checking.is_none());
    }

    #[test]
    fn outside_an_installation_the_check_says_so_only_when_asked() {
        // A test binary is never installed.
        assert!(matches!(check(), Ok(Found::NotInstalled)));
    }
}
