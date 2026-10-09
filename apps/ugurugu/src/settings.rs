// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The user's settings, kept in one JSON file: `%APPDATA%\Ugurugu\3\settings.json`,
//! or `UGURUGU_SETTINGS_PATH`. 2.2.13's registry keys are not read.
//!
//! A value that cannot be read is left at its default and logged; keys this
//! version does not know are written back as they were, so a newer version's
//! settings survive this one. Changes are written by a thread of their own once
//! they have stopped for `QUIET`, and once more when the store is dropped.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{Map, Value};
use ugu_session::Tools;

use crate::shortcuts::Shortcuts;

mod tools;

/// How long settings must stay unchanged before they are written.
const QUIET: Duration = Duration::from_millis(500);
/// Marks the file as this program's, as `.ugurugu` files carry a format.
const FORMAT: &str = "ugurugu.settings";
const VERSION: u64 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Language {
    /// The first of the user's Windows display languages that has a
    /// translation.
    #[default]
    System,
    English,
    Korean,
    Japanese,
}

impl Language {
    pub const ALL: [Self; 4] = [Self::System, Self::English, Self::Korean, Self::Japanese];

    /// The code stored in the file, and the interface catalogue's for all
    /// but `System`.
    pub fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::English => "en",
            Self::Korean => "ko",
            Self::Japanese => "ja",
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|each| each.code() == code)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Takes effect at the next start, as in 2.2.13.
    pub language: Language,
    /// The interface accent, `None` for the default amber.
    pub accent: Option<[u8; 3]>,
    /// Off stops the canvas from playing the wobble.
    pub wobble_animation: bool,
    /// Where new documents are first saved and exported, `None` for
    /// Documents.
    pub default_save_folder: Option<PathBuf>,
    /// The tools and colour history as last left.
    pub tools: Tools,
    /// The shortcuts changed from their defaults.
    pub shortcuts: Shortcuts,
    /// Keys this version does not know, kept to be written back.
    unknown: Map<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Language::System,
            accent: None,
            wobble_animation: true,
            default_save_folder: None,
            tools: Tools::default(),
            shortcuts: Shortcuts::default(),
            unknown: Map::new(),
        }
    }
}

impl Settings {
    /// What the settings dialog restores: its own settings, shortcuts
    /// included, as on a fresh install. The tools, the colour history and
    /// what this version cannot show stay, as in 2.2.13.
    pub fn restored(&self) -> Self {
        Self {
            tools: self.tools.clone(),
            shortcuts: self.shortcuts.restored(),
            unknown: self.unknown.clone(),
            ..Self::default()
        }
    }

    /// Reads settings from `text`. Anything unreadable is left at its default
    /// and logged.
    fn parse(text: &str) -> Self {
        let mut settings = Self::default();
        // Notepad may have saved it with a byte order mark.
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut object = match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(object)) => object,
            Ok(_) => {
                tracing::warn!("the settings file is not a JSON object; using the defaults");
                return settings;
            }
            Err(error) => {
                tracing::warn!(%error, "the settings file cannot be read; using the defaults");
                return settings;
            }
        };
        if object.get("format").and_then(Value::as_str) != Some(FORMAT) {
            tracing::warn!("the settings file is not marked as Ugurugu's; reading it anyway");
        }
        let version = object.get("version").and_then(Value::as_u64);
        if version.is_none_or(|version| version > VERSION) {
            tracing::warn!(
                ?version,
                "settings from another version; reading what is known"
            );
        }
        object.remove("format");
        object.remove("version");

        if let Some(code) = take(&mut object, "language", string) {
            match Language::from_code(&code) {
                Some(language) => settings.language = language,
                None => tracing::warn!(code, "unknown interface language in the settings"),
            }
        }
        if let Some(text) = take(&mut object, "accent", string) {
            match parse_rgb(&text) {
                Some(rgb) => settings.accent = Some(rgb),
                None => tracing::warn!(text, "the accent in the settings is not #rrggbb"),
            }
        }
        if let Some(on) = take(&mut object, "wobbleAnimation", Value::as_bool) {
            settings.wobble_animation = on;
        }
        if let Some(folder) = take(&mut object, "defaultSaveFolder", string) {
            let folder = PathBuf::from(folder);
            if folder.is_absolute() {
                settings.default_save_folder = Some(folder);
            } else {
                tracing::warn!(?folder, "the default save folder is not a full path");
            }
        }
        if let Some(value) = object.remove("tools") {
            settings.tools = tools::parse(&value);
        }
        if let Some(value) = object.remove("colorHistory") {
            settings.tools.colors = tools::parse_history(&value);
        }
        if let Some(value) = object.remove("shortcuts") {
            settings.shortcuts = Shortcuts::parse(&value);
        }
        if !object.is_empty() {
            let keys: Vec<&String> = object.keys().collect();
            tracing::info!(?keys, "settings this version does not use are kept");
        }
        settings.unknown = object;
        settings
    }

    fn to_json(&self) -> String {
        let mut object = self.unknown.clone();
        object.insert("format".to_owned(), FORMAT.into());
        object.insert("version".to_owned(), VERSION.into());
        if self.language != Language::System {
            object.insert("language".to_owned(), self.language.code().into());
        }
        if let Some([r, g, b]) = self.accent {
            object.insert(
                "accent".to_owned(),
                format!("#{r:02x}{g:02x}{b:02x}").into(),
            );
        }
        object.insert("wobbleAnimation".to_owned(), self.wobble_animation.into());
        if let Some(folder) = &self.default_save_folder {
            object.insert(
                "defaultSaveFolder".to_owned(),
                folder.to_string_lossy().into_owned().into(),
            );
        }
        object.insert("tools".to_owned(), tools::to_json(&self.tools));
        if !self.tools.colors.colors().is_empty() {
            object.insert(
                "colorHistory".to_owned(),
                tools::history_to_json(&self.tools.colors),
            );
        }
        if let Some(shortcuts) = self.shortcuts.to_json() {
            object.insert("shortcuts".to_owned(), shortcuts);
        }
        let mut text =
            serde_json::to_string_pretty(&Value::Object(object)).expect("settings are plain JSON");
        text.push('\n');
        text
    }
}

/// Removes `key` from `object` and reads it with `read`, logging a value of
/// the wrong type.
fn take<T>(
    object: &mut Map<String, Value>,
    key: &str,
    read: impl FnOnce(&Value) -> Option<T>,
) -> Option<T> {
    let value = object.remove(key)?;
    let read = read(&value);
    if read.is_none() {
        tracing::warn!(key, %value, "a setting has a value of the wrong type");
    }
    read
}

fn string(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

/// The folder save and export dialogs start in for a document that has never
/// been saved: the `configured` one while it exists, else Documents. Looks at
/// the disk.
pub fn save_folder(configured: Option<&Path>) -> Option<PathBuf> {
    configured
        .filter(|folder| folder.is_dir())
        .map(Path::to_path_buf)
        .or_else(ugu_win::folder::documents)
}

fn parse_rgb(text: &str) -> Option<[u8; 3]> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 || !digits.is_ascii() {
        return None;
    }
    let channel = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

/// Where the settings live: `UGURUGU_SETTINGS_PATH`, else the roaming
/// profile. `None` when Windows names no such folder.
fn default_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("UGURUGU_SETTINGS_PATH") {
        return Some(PathBuf::from(path));
    }
    Some(
        ugu_win::folder::roaming_app_data()?
            .join("Ugurugu")
            .join("3")
            .join("settings.json"),
    )
}

fn load(path: &Path) -> Settings {
    match std::fs::read_to_string(path) {
        Ok(text) => Settings::parse(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Settings::default(),
        Err(error) => {
            tracing::warn!(%error, ?path, "the settings file cannot be opened; using the defaults");
            Settings::default()
        }
    }
}

/// Writes `settings` beside `path` and then puts it in place, so a reader or
/// a crash sees the old file or the new one, never part of one.
fn write(path: &Path, settings: &Settings) -> std::io::Result<()> {
    use std::io::Write;

    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let mut name = path.file_name().unwrap_or_default().to_owned();
    // Per process: several windows may save at once.
    name.push(format!(".{}.tmp", std::process::id()));
    let temporary = path.with_file_name(name);
    let written = (|| {
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(settings.to_json().as_bytes())?;
        file.sync_all()?;
        drop(file);
        ugu_win::file::replace_file(&temporary, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    written
}

/// The settings and the thread that writes them.
pub struct Store {
    settings: Settings,
    writer: Option<Writer>,
}

struct Writer {
    to_thread: Sender<Settings>,
    thread: Option<JoinHandle<()>>,
    /// Files written, for tests.
    #[cfg_attr(not(test), expect(dead_code, reason = "read by tests"))]
    writes: Arc<AtomicUsize>,
}

impl Store {
    /// Reads the settings from their usual place.
    pub fn load() -> Self {
        let path = default_path();
        if path.is_none() {
            tracing::warn!("no folder for settings; they will not be kept");
        }
        Self::open(path)
    }

    /// Reads the settings at `path`; `None` keeps them only in memory.
    pub fn open(path: Option<PathBuf>) -> Self {
        let settings = path.as_deref().map(load).unwrap_or_default();
        tracing::debug!(?path, ?settings, "settings read");
        Self {
            settings,
            writer: path.map(Writer::spawn),
        }
    }

    pub fn get(&self) -> &Settings {
        &self.settings
    }

    /// Changes the settings with `change` and has them written if that
    /// changed anything. Never touches the file on this thread.
    pub fn change(&mut self, change: impl FnOnce(&mut Settings)) {
        let before = self.settings.clone();
        change(&mut self.settings);
        if self.settings != before
            && let Some(writer) = &self.writer
        {
            let _ = writer.to_thread.send(self.settings.clone());
        }
    }

    #[cfg(test)]
    fn writes(&self) -> usize {
        self.writer
            .as_ref()
            .map_or(0, |writer| writer.writes.load(Ordering::Relaxed))
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        if let Some(mut writer) = self.writer.take() {
            // Closing the channel has the thread write what waits and end.
            drop(writer.to_thread);
            if let Some(thread) = writer.thread.take() {
                let _ = thread.join();
            }
        }
    }
}

impl Writer {
    fn spawn(path: PathBuf) -> Self {
        let (to_thread, messages) = mpsc::channel();
        let writes = Arc::new(AtomicUsize::new(0));
        let counted = writes.clone();
        let thread = std::thread::Builder::new()
            .name("settings".to_owned())
            .spawn(move || write_changes(&path, &messages, &counted))
            .map_err(|error| tracing::error!(%error, "cannot start the settings thread"))
            .ok();
        Self {
            to_thread,
            thread,
            writes,
        }
    }
}

/// Writes each change once `QUIET` passes without another, until the
/// channel closes.
fn write_changes(path: &Path, messages: &Receiver<Settings>, writes: &AtomicUsize) {
    let mut waiting: Option<Settings> = None;
    let save = |settings: &Settings| {
        let started = std::time::Instant::now();
        match write(path, settings) {
            Ok(()) => tracing::debug!(elapsed = ?started.elapsed(), "settings written"),
            Err(error) => tracing::warn!(%error, ?path, "the settings were not written"),
        }
        writes.fetch_add(1, Ordering::Relaxed);
    };
    loop {
        let message = if waiting.is_some() {
            messages.recv_timeout(QUIET)
        } else {
            messages.recv().map_err(|_| RecvTimeoutError::Disconnected)
        };
        match message {
            Ok(settings) => waiting = Some(settings),
            Err(RecvTimeoutError::Timeout) => {
                if let Some(settings) = waiting.take() {
                    save(&settings);
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                if let Some(settings) = waiting.take() {
                    save(&settings);
                }
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("ugurugu-settings-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            Self(path)
        }

        fn settings(&self) -> PathBuf {
            self.0.join("3").join("settings.json")
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn changed() -> Settings {
        Settings {
            language: Language::Korean,
            accent: Some([0x12, 0xab, 0xef]),
            wobble_animation: false,
            default_save_folder: Some(PathBuf::from(r"C:\Drawings\새 폴더")),
            tools: {
                let mut tools = Tools {
                    tool: ugu_session::Tool::Fill,
                    ..Tools::default()
                };
                tools.colors.record(ugu_core::ops::Rgba8([1, 2, 3, 4]));
                tools
            },
            shortcuts: {
                let mut shortcuts = Shortcuts::default();
                let fit = crate::shortcuts::Chord::parse("F").unwrap();
                shortcuts
                    .assign(crate::shortcuts::Action::Fit, Some(fit))
                    .unwrap();
                shortcuts
            },
            unknown: Map::new(),
        }
    }

    #[test]
    fn settings_written_are_read_back() {
        let folder = Folder::new("round-trip");
        {
            let mut store = Store::open(Some(folder.settings()));
            assert_eq!(*store.get(), Settings::default());
            store.change(|settings| *settings = changed());
        }
        let store = Store::open(Some(folder.settings()));
        assert_eq!(*store.get(), changed());
        let text = std::fs::read_to_string(folder.settings()).unwrap();
        assert!(text.contains("\"format\": \"ugurugu.settings\""), "{text}");
        assert!(text.contains("\"accent\": \"#12abef\""), "{text}");
        assert!(text.contains("\"fit\": \"F\""), "{text}");
    }

    #[test]
    fn defaults_leave_optional_keys_out() {
        let text = Settings::default().to_json();
        let object: Map<String, Value> = serde_json::from_str(&text).unwrap();
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["format", "tools", "version", "wobbleAnimation"]);
    }

    #[test]
    fn a_missing_or_broken_file_gives_the_defaults() {
        let folder = Folder::new("broken");
        assert_eq!(
            *Store::open(Some(folder.settings())).get(),
            Settings::default()
        );
        for text in [
            "",
            "{",
            "[1, 2]",
            "null",
            "\u{feff}{}",
            "{\"language\": \"ko\"",
        ] {
            assert_eq!(Settings::parse(text), Settings::default(), "{text:?}");
        }
        let marked = Settings::parse("\u{feff}{\"language\": \"ko\"}");
        assert_eq!(marked.language, Language::Korean);
    }

    #[test]
    fn a_bad_value_falls_back_alone() {
        let settings = Settings::parse(
            r##"{"format": "ugurugu.settings", "version": 1, "language": "fr",
                "accent": "#12abef", "wobbleAnimation": "yes",
                "defaultSaveFolder": "relative\\folder"}"##,
        );
        assert_eq!(
            settings,
            Settings {
                accent: Some([0x12, 0xab, 0xef]),
                ..Settings::default()
            }
        );
        for accent in [
            "12abef",
            "#12abe",
            "#12abeg",
            "#12abef0",
            "#ｆｆｆ",
            7.to_string().as_str(),
        ] {
            assert_eq!(parse_rgb(accent), None, "{accent}");
        }
        assert_eq!(parse_rgb("#FFc94a"), Some([0xff, 0xc9, 0x4a]));
    }

    #[test]
    fn unknown_keys_are_written_back() {
        let text = r#"{"format": "ugurugu.settings", "version": 2, "language": "ja",
            "dock": {"left": ["layers"]}, "future": 3}"#;
        let mut settings = Settings::parse(text);
        assert_eq!(settings.language, Language::Japanese);
        settings.wobble_animation = false;
        let again: Value = serde_json::from_str(&settings.to_json()).unwrap();
        assert_eq!(again["dock"]["left"][0], "layers");
        assert_eq!(again["future"], 3);
        assert_eq!(again["version"], 1);
        assert_eq!(Settings::parse(&settings.to_json()), settings);
        assert_eq!(
            settings.restored().to_json(),
            Settings {
                unknown: settings.unknown.clone(),
                ..Settings::default()
            }
            .to_json()
        );
    }

    #[test]
    fn changes_are_written_together_after_a_quiet_spell_off_this_thread() {
        let folder = Folder::new("quiet");
        let mut store = Store::open(Some(folder.settings()));
        for on in [false, true, false] {
            store.change(|settings| settings.wobble_animation = on);
        }
        // The change returned without writing; the folder is made on write.
        assert!(!folder.settings().exists());
        std::thread::sleep(QUIET * 3);
        assert_eq!(store.writes(), 1);
        assert!(
            !Settings::parse(&std::fs::read_to_string(folder.settings()).unwrap()).wobble_animation
        );
        // No change, no write.
        store.change(|settings| settings.wobble_animation = false);
        std::thread::sleep(QUIET * 3);
        assert_eq!(store.writes(), 1);
    }

    #[test]
    fn closing_writes_at_once() {
        let folder = Folder::new("close");
        let started = std::time::Instant::now();
        {
            let mut store = Store::open(Some(folder.settings()));
            store.change(|settings| settings.language = Language::English);
        }
        assert!(started.elapsed() < QUIET, "{:?}", started.elapsed());
        assert_eq!(
            Store::open(Some(folder.settings())).get().language,
            Language::English
        );
    }

    #[test]
    fn a_failed_write_leaves_the_old_file() {
        let folder = Folder::new("fail");
        let path = folder.settings();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, changed().to_json()).unwrap();
        // Another program holding the file open, sharing nothing.
        let held = {
            use std::os::windows::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(&path)
                .unwrap()
        };
        assert!(write(&path, &Settings::default()).is_err());
        drop(held);
        assert_eq!(
            Settings::parse(&std::fs::read_to_string(&path).unwrap()),
            changed()
        );
        let leftovers = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(leftovers, 1, "the temporary file is removed");
    }

    #[test]
    fn a_missing_save_folder_falls_back_to_documents() {
        let folder = Folder::new("save-folder");
        let gone = folder.0.join("gone");
        assert_eq!(save_folder(Some(&gone)), ugu_win::folder::documents());
        std::fs::create_dir_all(&gone).unwrap();
        assert_eq!(save_folder(Some(&gone)), Some(gone));
        assert_eq!(save_folder(None), ugu_win::folder::documents());
    }
}
