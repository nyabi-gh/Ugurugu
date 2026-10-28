// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The user's settings, kept in one JSON file: `%APPDATA%\Ugurugu\3\settings.json`,
//! or `UGURUGU_SETTINGS_PATH`. 2.2.13's registry keys are not read.
//!
//! A value that cannot be read is left at its default and logged; keys this
//! version does not know are written back as they were, so a newer version's
//! settings survive this one. Changes are written by a thread of their own once
//! they have stopped for `QUIET`, and once more when the store is dropped.
//!
//! Each window writes only the settings it changed over the file as it is
//! then, as 2.2.13's registry keys were written one by one, so one window
//! does not put back what another changed.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{Map, Value};
use ugu_core::document::limits;
use ugu_session::Tools;

use crate::layout::Layout;
use crate::shortcuts::Shortcuts;
use ugu_platform::placement::Placement;

pub mod preset;
mod tools;

/// How long settings must stay unchanged before they are written.
const QUIET: Duration = Duration::from_millis(500);
/// How long after a write that failed it is tried again.
const RETRY: Duration = Duration::from_secs(2);
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
    /// Simplified Chinese.
    Chinese,
}

impl Language {
    pub const ALL: [Self; 5] = [
        Self::System,
        Self::English,
        Self::Korean,
        Self::Japanese,
        Self::Chinese,
    ];

    /// The code stored in the file, and the interface catalogue's for all
    /// but `System`.
    pub fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::English => "en",
            Self::Korean => "ko",
            Self::Japanese => "ja",
            Self::Chinese => "zh-Hans",
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
    /// Dark, light or as Windows is.
    pub theme: crate::theme::Mode,
    /// Off stops the canvas from playing the wobble.
    pub wobble_animation: bool,
    /// The grid over the document's pixels when enlarged enough.
    pub pixel_grid: bool,
    /// Where new documents are first saved and exported, `None` for
    /// Documents.
    pub default_save_folder: Option<PathBuf>,
    /// The long edge animations were last exported at.
    pub export_edge: Option<u32>,
    /// How many times pixel art was last enlarged when exported, 1 for not.
    pub pixel_export_times: Option<u32>,
    /// The tools and colour history as last left.
    pub tools: Tools,
    /// The shortcuts changed from their defaults.
    pub shortcuts: Shortcuts,
    /// Whether to check for updates by itself.
    pub check_updates: bool,
    /// Where the panels are.
    pub layout: Layout,
    /// Where the window was when it last closed.
    pub window: Option<Placement>,
    /// Keys this version does not know, kept to be written back.
    unknown: Map<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Language::System,
            accent: None,
            theme: crate::theme::Mode::default(),
            wobble_animation: true,
            pixel_grid: true,
            default_save_folder: None,
            export_edge: None,
            pixel_export_times: None,
            tools: Tools::default(),
            shortcuts: Shortcuts::default(),
            check_updates: true,
            layout: Layout::default(),
            window: None,
            unknown: Map::new(),
        }
    }
}

impl Settings {
    /// What the settings dialog restores: its own settings, shortcuts
    /// included, as on a fresh install. The tools, the colour history and
    /// panel layout and what this version cannot show stay, as in 2.2.13.
    pub fn restored(&self) -> Self {
        Self {
            export_edge: self.export_edge,
            pixel_export_times: self.pixel_export_times,
            // Pen pressure is the one tool setting the dialog shows.
            tools: Tools {
                uses_pressure: Tools::default().uses_pressure,
                ..self.tools.clone()
            },
            shortcuts: self.shortcuts.restored(),
            layout: self.layout.clone(),
            window: self.window,
            unknown: self.unknown.clone(),
            ..Self::default()
        }
    }

    /// Reads settings from `text`, the defaults when it holds none.
    #[cfg(test)]
    fn parse(text: &str) -> Self {
        Self::read(text).unwrap_or_default()
    }

    /// Reads settings from `text`. A value that cannot be read is left at
    /// its default and logged; `None` when `text` is no JSON object at all.
    fn read(text: &str) -> Option<Self> {
        let mut settings = Self::default();
        // Notepad may have saved it with a byte order mark.
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut object = match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(object)) => object,
            Ok(_) => {
                tracing::warn!("the settings file is not a JSON object");
                return None;
            }
            Err(error) => {
                tracing::warn!(%error, "the settings file cannot be read");
                return None;
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
        if let Some(code) = take(&mut object, "theme", string) {
            match crate::theme::Mode::from_code(&code) {
                Some(theme) => settings.theme = theme,
                None => tracing::warn!(code, "unknown theme in the settings"),
            }
        }
        if let Some(on) = take(&mut object, "wobbleAnimation", Value::as_bool) {
            settings.wobble_animation = on;
        }
        if let Some(on) = take(&mut object, "pixelGrid", Value::as_bool) {
            settings.pixel_grid = on;
        }
        if let Some(folder) = take(&mut object, "defaultSaveFolder", string) {
            let folder = PathBuf::from(folder);
            if folder.is_absolute() {
                settings.default_save_folder = Some(folder);
            } else {
                tracing::warn!(?folder, "the default save folder is not a full path");
            }
        }
        if let Some(edge) = take(&mut object, "exportSize", Value::as_u64) {
            match u32::try_from(edge) {
                Ok(edge) if limits::CANVAS_EDGE.contains(&edge) => {
                    settings.export_edge = Some(edge)
                }
                _ => tracing::warn!(edge, "the export size in the settings is out of range"),
            }
        }
        if let Some(times) = take(&mut object, "pixelExportScale", Value::as_u64) {
            match u32::try_from(times) {
                Ok(times) if crate::export::offered_times(times) => {
                    settings.pixel_export_times = Some(times)
                }
                _ => tracing::warn!(
                    times,
                    "the pixel export scale in the settings is not offered"
                ),
            }
        }
        if let Some(on) = take(&mut object, "checkForUpdates", Value::as_bool) {
            settings.check_updates = on;
        }
        // When the last update check finished, before 3.3.2 checked at
        // every start.
        object.remove("updateChecked");
        if let Some(value) = object.remove("tools") {
            settings.tools = tools::parse(&value);
        }
        if let Some(value) = object.remove("colorHistory") {
            settings.tools.colors = tools::parse_history(&value);
        }
        if let Some(value) = object.remove("shortcuts") {
            settings.shortcuts = Shortcuts::parse(&value);
        }
        if let Some(value) = object.remove("panels") {
            settings.layout = Layout::parse(&value);
        }
        if let Some(value) = object.remove("window") {
            let number = |key| {
                value
                    .get(key)
                    .and_then(Value::as_i64)
                    .and_then(|n| i32::try_from(n).ok())
            };
            let placement = match ["left", "top", "right", "bottom"].map(number) {
                [Some(left), Some(top), Some(right), Some(bottom)] => Some(Placement {
                    normal: [left, top, right, bottom],
                    maximized: value
                        .get("maximized")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                }),
                _ => None,
            };
            match placement.filter(Placement::valid) {
                Some(placement) => settings.window = Some(placement),
                None => tracing::warn!(%value, "the window place in the settings is not one"),
            }
        }
        if !object.is_empty() {
            let keys: Vec<&String> = object.keys().collect();
            tracing::info!(?keys, "settings this version does not use are kept");
        }
        settings.unknown = object;
        Some(settings)
    }

    #[cfg(test)]
    fn to_json(&self) -> String {
        to_text(self.to_object())
    }

    fn to_object(&self) -> Map<String, Value> {
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
        if self.theme != crate::theme::Mode::default() {
            object.insert("theme".to_owned(), self.theme.code().into());
        }
        object.insert("wobbleAnimation".to_owned(), self.wobble_animation.into());
        object.insert("pixelGrid".to_owned(), self.pixel_grid.into());
        if let Some(folder) = &self.default_save_folder {
            object.insert(
                "defaultSaveFolder".to_owned(),
                folder.to_string_lossy().into_owned().into(),
            );
        }
        if let Some(edge) = self.export_edge {
            object.insert("exportSize".to_owned(), edge.into());
        }
        if let Some(times) = self.pixel_export_times {
            object.insert("pixelExportScale".to_owned(), times.into());
        }
        if !self.check_updates {
            object.insert("checkForUpdates".to_owned(), false.into());
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
        if self.layout != Layout::default() {
            object.insert("panels".to_owned(), self.layout.to_json());
        }
        if let Some(Placement {
            normal: [left, top, right, bottom],
            maximized,
        }) = self.window
        {
            object.insert(
                "window".to_owned(),
                serde_json::json!({
                    "left": left,
                    "top": top,
                    "right": right,
                    "bottom": bottom,
                    "maximized": maximized,
                }),
            );
        }
        object
    }
}

fn to_text(object: Map<String, Value>) -> String {
    let mut text =
        serde_json::to_string_pretty(&Value::Object(object)).expect("settings are plain JSON");
    text.push('\n');
    text
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
        .or_else(ugu_platform::folder::documents)
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
pub fn default_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("UGURUGU_SETTINGS_PATH") {
        return Some(PathBuf::from(path));
    }
    Some(
        ugu_platform::folder::roaming_app_data()?
            .join("Ugurugu")
            .join("3")
            .join("settings.json"),
    )
}

/// The settings at `path`, and where a file there that held none was moved
/// to, so that writing the defaults does not lose it.
fn load(path: &Path) -> (Settings, Option<PathBuf>) {
    let text = match read_text(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (Settings::default(), None);
        }
        // Left as it is: writing reads it again and fails the same way.
        Err(error) => {
            tracing::warn!(%error, ?path, "the settings file cannot be opened; using the defaults");
            return (Settings::default(), None);
        }
    };
    if let Some(settings) = text.as_deref().and_then(Settings::read) {
        return (settings, None);
    }
    match set_aside(path) {
        Ok(kept) => {
            tracing::warn!(?kept, "using the default settings; the file is kept");
            (Settings::default(), Some(kept))
        }
        Err(error) => {
            tracing::warn!(%error, ?path, "using the default settings; the file could not be kept");
            (Settings::default(), None)
        }
    }
}

/// The file at `path` as text; `None` when it is not UTF-8, which holds no
/// settings either, as Notepad's UTF-16 or ANSI.
fn read_text(path: &Path) -> std::io::Result<Option<String>> {
    Ok(String::from_utf8(std::fs::read(path)?).ok())
}

/// Moves the file at `path` beside it, named for now, and says where.
fn set_aside(path: &Path) -> std::io::Result<PathBuf> {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let stamp = ugu_platform::clock::local_stamp().unwrap_or_else(|| "unknown".to_owned());
    let mut kept = path.with_file_name(format!("{stem}-unreadable-{stamp}.json"));
    for again in 2.. {
        if !kept.exists() {
            break;
        }
        kept = path.with_file_name(format!("{stem}-unreadable-{stamp}-{again}.json"));
    }
    std::fs::rename(path, &kept)?;
    Ok(kept)
}

/// Writes what changed from `base` to `settings` over the file at `path` as
/// it is now, leaving the rest as another window may have left it.
fn write_changes_over(
    path: &Path,
    base: &Map<String, Value>,
    settings: &Settings,
) -> std::io::Result<()> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let _lock = lock(path)?;
    let current = settings.to_object();
    let on_disk = match read_text(path) {
        Ok(Some(text)) => {
            match serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) {
                Ok(Value::Object(object)) => Some(object),
                _ => None,
            }
        }
        Ok(None) => None,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    // One that cannot be read is kept beside it and replaced, as at start;
    // left in place, and not written over, if it cannot be moved.
    let Some(mut merged) = on_disk else {
        if path.exists() {
            let kept = set_aside(path)?;
            tracing::warn!(?kept, "the settings file had become unreadable; it is kept");
        }
        return write_text(path, &to_text(current));
    };
    for key in base.keys().chain(current.keys()) {
        if base.get(key) != current.get(key) {
            match current.get(key) {
                Some(value) => merged.insert(key.clone(), value.clone()),
                None => merged.remove(key),
            };
        }
    }
    write_text(path, &to_text(merged))
}

/// Held while the file is read and written, so windows take turns; removed
/// as it closes.
fn lock(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    const ERROR_SHARING_VIOLATION: i32 = 32;
    const FILE_FLAG_DELETE_ON_CLOSE: u32 = 0x0400_0000;
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(".lock");
    let lock = path.with_file_name(name);
    let started = std::time::Instant::now();
    loop {
        match std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .custom_flags(FILE_FLAG_DELETE_ON_CLOSE)
            .open(&lock)
        {
            Err(error)
                if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION)
                    && started.elapsed() < Duration::from_secs(2) =>
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            opened => return opened,
        }
    }
}

/// Writes `text` beside `path` and then puts it in place, so a reader or a
/// crash sees the old file or the new one, never part of one.
fn write_text(path: &Path, text: &str) -> std::io::Result<()> {
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
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        ugu_platform::file::replace_file(&temporary, path)
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
    /// Where an unreadable settings file found at start was moved to.
    set_aside: Option<PathBuf>,
    /// Whether the writes failing now were told of.
    told_failure: bool,
}

struct Writer {
    to_thread: Sender<Settings>,
    thread: Option<JoinHandle<()>>,
    /// Why the last write failed, until one is written.
    failed: Arc<Mutex<Option<String>>>,
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
        let (settings, set_aside) = path.as_deref().map(load).unwrap_or_default();
        tracing::debug!(?path, ?settings, "settings read");
        let base = settings.to_object();
        Self {
            settings,
            writer: path.map(|path| Writer::spawn(path, base)),
            set_aside,
            told_failure: false,
        }
    }

    /// Why the settings could not be written, once each time writing them
    /// starts failing; they are tried again until they are.
    pub fn write_failure(&mut self) -> Option<String> {
        let failed = self.writer.as_ref()?.failed.lock().ok()?.clone();
        let Some(error) = failed else {
            self.told_failure = false;
            return None;
        };
        (!std::mem::replace(&mut self.told_failure, true)).then_some(error)
    }

    /// Where an unreadable settings file found at start was moved to, once.
    pub fn take_set_aside(&mut self) -> Option<PathBuf> {
        self.set_aside.take()
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
    /// `base` is the settings as read, which this window's changes are of.
    fn spawn(path: PathBuf, base: Map<String, Value>) -> Self {
        let (to_thread, messages) = mpsc::channel();
        let writes = Arc::new(AtomicUsize::new(0));
        let counted = writes.clone();
        let failed = Arc::new(Mutex::new(None));
        let failing = failed.clone();
        let thread = std::thread::Builder::new()
            .name("settings".to_owned())
            .spawn(move || write_changes(&path, base, &messages, &counted, &failing))
            .map_err(|error| tracing::error!(%error, "cannot start the settings thread"))
            .ok();
        if thread.is_none() {
            *failed.lock().expect("not yet shared") = Some("no settings thread".to_owned());
        }
        Self {
            to_thread,
            thread,
            failed,
            writes,
        }
    }
}

/// Writes each change once `QUIET` passes without another, and one that
/// failed again each `RETRY` until it is written, until the channel closes;
/// `failed` says why the last one failed.
fn write_changes(
    path: &Path,
    mut base: Map<String, Value>,
    messages: &Receiver<Settings>,
    writes: &AtomicUsize,
    failed: &Mutex<Option<String>>,
) {
    let mut waiting: Option<Settings> = None;
    let mut retrying = false;
    let mut save = |settings: &Settings| {
        let started = std::time::Instant::now();
        let written = match write_changes_over(path, &base, settings) {
            Ok(()) => {
                base = settings.to_object();
                tracing::debug!(elapsed = ?started.elapsed(), "settings written");
                None
            }
            Err(error) => {
                tracing::warn!(%error, ?path, "the settings were not written");
                Some(error.to_string())
            }
        };
        let ok = written.is_none();
        if let Ok(mut failed) = failed.lock() {
            *failed = written;
        }
        writes.fetch_add(1, Ordering::Relaxed);
        ok
    };
    loop {
        let message = if waiting.is_some() {
            messages.recv_timeout(if retrying { RETRY } else { QUIET })
        } else {
            messages.recv().map_err(|_| RecvTimeoutError::Disconnected)
        };
        match message {
            Ok(settings) => {
                waiting = Some(settings);
                retrying = false;
            }
            Err(RecvTimeoutError::Timeout) => {
                if let Some(settings) = waiting.take() {
                    retrying = !save(&settings);
                    // Still a change from `base`.
                    if retrying {
                        waiting = Some(settings);
                    }
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
            theme: crate::theme::Mode::Light,
            wobble_animation: false,
            pixel_grid: false,
            default_save_folder: Some(PathBuf::from(r"C:\Drawings\새 폴더")),
            export_edge: Some(384),
            pixel_export_times: Some(4),
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
            check_updates: false,
            layout: {
                let mut layout = Layout::default();
                layout.move_to(
                    crate::layout::Panel::Layers,
                    crate::layout::Place::Float([12.0, 34.0]),
                );
                layout
            },
            window: Some(Placement {
                normal: [-1700, 40, -100, 1040],
                maximized: true,
            }),
            unknown: Map::new(),
        }
    }

    #[test]
    fn a_window_place_that_is_not_one_is_dropped_and_restoring_keeps_it() {
        for bad in [
            r#"{"window": {"left": 0, "top": 0, "right": 50, "bottom": 50}}"#,
            r#"{"window": {"left": 0, "top": 0, "right": 5000000000, "bottom": 900}}"#,
            r#"{"window": [0, 0, 900, 900]}"#,
        ] {
            assert_eq!(Settings::parse(bad).window, None, "{bad}");
        }
        let settings = Settings::parse(
            r#"{"window": {"left": 10, "top": 20, "right": 1610, "bottom": 1020}}"#,
        );
        let placement = Some(Placement {
            normal: [10, 20, 1610, 1020],
            maximized: false,
        });
        assert_eq!(settings.window, placement);
        assert_eq!(settings.restored().window, placement);
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
        assert_eq!(
            keys,
            ["format", "pixelGrid", "tools", "version", "wobbleAnimation"]
        );
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
    fn an_unreadable_file_is_kept_beside_the_defaults_written_over_it() {
        let folder = Folder::new("unreadable");
        let path = folder.settings();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let broken = "{\"language\": \"ko\", \"shortcuts\": {";
        std::fs::write(&path, broken).unwrap();
        let mut store = Store::open(Some(path.clone()));
        assert_eq!(*store.get(), Settings::default());
        let kept = store.take_set_aside().expect("the file is kept");
        assert_eq!(store.take_set_aside(), None);
        store.change(|settings| settings.pixel_grid = false);
        drop(store);
        assert_eq!(std::fs::read_to_string(&kept).unwrap(), broken);
        assert!(!Settings::parse(&std::fs::read_to_string(&path).unwrap()).pixel_grid);

        // Broken while the window runs: kept again before it is written over.
        let mut store = Store::open(Some(path.clone()));
        std::fs::write(&path, "not json").unwrap();
        store.change(|settings| settings.pixel_grid = true);
        drop(store);
        let names: Vec<String> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        let kept_again: Vec<&String> = names
            .iter()
            .filter(|name| name.starts_with("settings-unreadable-"))
            .collect();
        assert_eq!(kept_again.len(), 2, "{names:?}");
        assert!(Settings::parse(&std::fs::read_to_string(&path).unwrap()).pixel_grid);
    }

    #[test]
    fn a_file_that_is_not_utf_8_is_kept_as_an_unreadable_one() {
        let folder = Folder::new("not-utf-8");
        let path = folder.settings();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // As Notepad saves it in UTF-16.
        let utf16: Vec<u8> = [0xff, 0xfe]
            .into_iter()
            .chain(
                "{\"language\": \"ko\"}"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes),
            )
            .collect();
        std::fs::write(&path, &utf16).unwrap();
        let mut store = Store::open(Some(path.clone()));
        let kept = store.take_set_aside().expect("the file is kept");
        assert_eq!(std::fs::read(&kept).unwrap(), utf16);
        store.change(|settings| settings.pixel_grid = false);
        drop(store);
        assert!(!Settings::parse(&std::fs::read_to_string(&path).unwrap()).pixel_grid);

        // Turned so while the window runs: kept before it is written over.
        let mut store = Store::open(Some(path.clone()));
        std::fs::write(&path, &utf16).unwrap();
        store.change(|settings| settings.pixel_grid = true);
        drop(store);
        assert!(Settings::parse(&std::fs::read_to_string(&path).unwrap()).pixel_grid);
    }

    #[test]
    fn the_last_update_check_kept_before_is_dropped() {
        let settings = Settings::parse(
            r#"{"format": "ugurugu.settings", "version": 1, "updateChecked": 1790000000}"#,
        );
        assert!(!settings.to_json().contains("updateChecked"));
    }

    #[test]
    fn a_bad_value_falls_back_alone() {
        let settings = Settings::parse(
            r##"{"format": "ugurugu.settings", "version": 1, "language": "fr",
                "accent": "#12abef", "wobbleAnimation": "yes",
                "defaultSaveFolder": "relative\\folder", "exportSize": 9000,
                "pixelExportScale": 3}"##,
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
    fn restoring_the_defaults_turns_pen_pressure_back_on_and_keeps_the_brushes() {
        let mut settings = Settings::default();
        settings.tools.uses_pressure = false;
        settings.tools.plain.pen.width = 31.0;
        let restored = settings.restored();
        assert!(restored.tools.uses_pressure);
        assert_eq!(restored.tools.plain, settings.tools.plain);
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
        assert!(write_text(&path, &Settings::default().to_json()).is_err());
        drop(held);
        assert_eq!(
            Settings::parse(&std::fs::read_to_string(&path).unwrap()),
            changed()
        );
        let leftovers = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(leftovers, 1, "the temporary file is removed");
    }

    #[test]
    fn settings_not_written_are_written_once_the_file_can_be() {
        let folder = Folder::new("retry");
        let path = folder.settings();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, Settings::default().to_json()).unwrap();
        let held = {
            use std::os::windows::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(&path)
                .unwrap()
        };
        let mut store = Store::open(Some(path.clone()));
        store.change(|settings| settings.pixel_grid = false);
        std::thread::sleep(QUIET * 3);
        drop(held);
        let failure = store.write_failure().expect("told");
        assert!(!failure.is_empty());
        assert_eq!(store.write_failure(), None, "told once");
        // Nothing else changes: it is tried again by itself.
        let started = std::time::Instant::now();
        while Settings::parse(&std::fs::read_to_string(&path).unwrap()).pixel_grid {
            assert!(started.elapsed() < RETRY * 3, "never written");
            std::thread::sleep(QUIET);
        }
        std::thread::sleep(QUIET);
        assert_eq!(store.write_failure(), None);
    }

    #[test]
    fn windows_keep_what_the_other_changed() {
        let folder = Folder::new("windows");
        let path = folder.settings();
        let mut first = Store::open(Some(path.clone()));
        let mut second = Store::open(Some(path.clone()));
        first.change(|settings| settings.check_updates = false);
        drop(first);
        second.change(|settings| settings.pixel_grid = false);
        drop(second);
        let read = Store::open(Some(path.clone()));
        assert!(!read.get().check_updates);
        assert!(!read.get().pixel_grid);
        // The same setting changed in both is the one changed last.
        let mut first = Store::open(Some(path.clone()));
        let mut second = Store::open(Some(path.clone()));
        first.change(|settings| settings.export_edge = Some(256));
        drop(first);
        second.change(|settings| settings.export_edge = Some(512));
        drop(second);
        let read = Store::open(Some(path.clone()));
        assert_eq!(read.get().export_edge, Some(512));
        assert!(!read.get().check_updates);
        assert!(!read.get().pixel_grid);
        let files = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(files, 1, "the lock is removed");
    }

    #[test]
    fn a_missing_save_folder_falls_back_to_documents() {
        let folder = Folder::new("save-folder");
        let gone = folder.0.join("gone");
        assert_eq!(save_folder(Some(&gone)), ugu_platform::folder::documents());
        std::fs::create_dir_all(&gone).unwrap();
        assert_eq!(save_folder(Some(&gone)), Some(gone));
        assert_eq!(save_folder(None), ugu_platform::folder::documents());
    }
}
