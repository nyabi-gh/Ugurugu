// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Log files and crash records, for finding out what went wrong on a user's
//! PC. Every run writes its own log, since several windows may run at once.
//! A panic or a crash Rust cannot catch leaves a record next to the logs,
//! and the next start says so.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

use crate::i18n::tr;

/// A log grows to this size, then starts over after keeping one old part.
const LOG_BYTES: u64 = 2 * 1024 * 1024;
/// Logs of this many runs are kept.
const KEPT_RUNS: usize = 10;

/// The commit the app was built from, in CI.
pub const COMMIT: &str = match option_env!("GITHUB_SHA") {
    Some(commit) => commit,
    None => "local build",
};

static FOLDER: OnceLock<Option<PathBuf>> = OnceLock::new();
static GPU: Mutex<String> = Mutex::new(String::new());
/// Records left by earlier runs, not yet told about.
static EARLIER: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// `%LOCALAPPDATA%\Ugurugu Data\3`, for what stays on this PC. Not under
/// `%LOCALAPPDATA%\Ugurugu`: that is the 2.2.13 install, which its
/// uninstaller removes whole. Velopack package ids cannot hold a space, so
/// no install of either version can be this folder.
pub fn local_data() -> Option<PathBuf> {
    Some(
        ugu_platform::folder::local_app_data()?
            .join("Ugurugu Data")
            .join("3"),
    )
}

/// `UGURUGU_LOG_PATH`, else `logs` in [`local_data`].
pub fn folder() -> Option<PathBuf> {
    FOLDER
        .get_or_init(|| {
            std::env::var_os("UGURUGU_LOG_PATH")
                .map(PathBuf::from)
                .or_else(|| Some(local_data()?.join("logs")))
        })
        .clone()
}

/// Writes out what is still queued for the logs when dropped.
pub struct Logging {
    _flush: (
        tracing_appender::non_blocking::WorkerGuard,
        Option<tracing_appender::non_blocking::WorkerGuard>,
    ),
}

/// Logs to standard output and to this run's file, records panics and
/// crashes, and finds the records of earlier runs. Keep the result until
/// the end.
pub fn start() -> Logging {
    let started = run_name();
    let folder = folder().filter(|folder| std::fs::create_dir_all(folder).is_ok());
    let filter = || {
        EnvFilter::try_from_env("UGURUGU_LOG")
            .unwrap_or_else(|_| EnvFilter::new("info,wgpu_core=warn,wgpu_hal=warn"))
    };
    // Logging must never block the render thread on a slow or full output.
    let (stdout, stdout_flush) = tracing_appender::non_blocking(std::io::stdout());
    let file = folder.as_ref().map(|folder| {
        prune(folder, KEPT_RUNS - 1);
        tracing_appender::non_blocking(Rotating::new(folder.join(format!("{started}.log"))))
    });
    use std::io::IsTerminal;
    let (file_writer, file_flush) = file.unzip();
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(std::io::stdout().is_terminal())
                .with_writer(stdout)
                .with_filter(filter()),
        )
        .with(file_writer.map(|writer| {
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(writer)
                .with_filter(filter())
        }))
        .init();
    if let Some(folder) = &folder {
        *EARLIER.lock().expect("no panic while holding it") = unseen(folder);
        record_panics(folder.clone(), started.clone());
        ugu_platform::crash::note_crashes(
            &folder.join(format!("crash-{started}-native.txt")),
            &heading(),
        );
        tracing::info!(folder = %folder.display(), version = env!("CARGO_PKG_VERSION"), commit = COMMIT, "logging");
    } else {
        tracing::warn!("no folder for log files");
    }
    Logging {
        _flush: (stdout_flush, file_flush),
    }
}

/// `ugurugu-<local date and time>-<process id>`.
fn run_name() -> String {
    let stamp = ugu_platform::clock::local_stamp().unwrap_or_else(|| "unknown".to_owned());
    format!("ugurugu-{stamp}-{}", std::process::id())
}

/// Remembers the GPU shown on, for crash records and the about tab.
pub fn set_gpu(summary: String) {
    *GPU.lock().expect("no panic while holding it") = summary;
}

pub fn gpu() -> String {
    GPU.lock().expect("no panic while holding it").clone()
}

fn heading() -> String {
    format!(
        "Ugurugu {} ({COMMIT})\r\n{}\r\n",
        env!("CARGO_PKG_VERSION"),
        ugu_platform::locale::os_version()
    )
}

/// What a user can paste into a report.
pub fn summary() -> String {
    let workers = std::thread::available_parallelism().map_or(0, usize::from);
    format!(
        "Ugurugu {} ({COMMIT})\n{}\nGPU: {}\nLogical processors: {workers}\nLogs: {}",
        env!("CARGO_PKG_VERSION"),
        ugu_platform::locale::os_version(),
        gpu(),
        folder().map_or_else(|| "-".to_owned(), |folder| folder.display().to_string()),
    )
}

fn record_panics(folder: PathBuf, run: String) {
    let count = AtomicUsize::new(0);
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let index = count.fetch_add(1, Ordering::Relaxed);
        let path = folder.join(format!("crash-{run}-panic-{index}.txt"));
        let thread = std::thread::current();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|text| (*text).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let location = info
            .location()
            .map_or_else(String::new, |location| location.to_string());
        let text = format!(
            "{}GPU: {}\nThread: {}\nPanic: {message}\nAt: {location}\n\n{}\n",
            heading().replace("\r\n", "\n"),
            gpu(),
            thread.name().unwrap_or("unnamed"),
            std::backtrace::Backtrace::force_capture()
        );
        let _ = std::fs::write(&path, text);
        tracing::error!(thread = thread.name(), message, location, "panic");
        previous(info);
    }));
}

/// Crash records in `folder` not yet told about, oldest first.
fn unseen(folder: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("crash-")
                        && name.ends_with(".txt")
                        && !name.ends_with(".seen.txt")
                })
        })
        .collect();
    found.sort();
    found
}

/// Marks crash records as told about, so later starts do not tell again.
fn mark_seen(records: &[PathBuf]) {
    for path in records {
        let _ = std::fs::rename(path, path.with_extension("seen.txt"));
    }
}

/// Removes the logs and told-about records of all but the newest `keep`
/// runs. A log still open by a running window cannot be removed and stays.
fn prune(folder: &Path, keep: usize) {
    let mut runs: Vec<(String, Vec<PathBuf>)> = Vec::new();
    for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let run = if let Some(rest) = name.strip_prefix("crash-") {
            if !name.ends_with(".seen.txt") {
                continue;
            }
            rest
        } else if name.starts_with("ugurugu-") && name.ends_with(".log") {
            name
        } else {
            continue;
        };
        // `ugurugu-<stamp>-<pid>` names the run.
        let run: String = run
            .split('.')
            .next()
            .unwrap_or(run)
            .split('-')
            .take(4)
            .collect::<Vec<_>>()
            .join("-");
        match runs.iter_mut().find(|(name, _)| *name == run) {
            Some((_, paths)) => paths.push(path),
            None => runs.push((run, vec![path])),
        }
    }
    runs.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, paths) in runs.into_iter().skip(keep) {
        for path in paths {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// A log file that keeps one old part once it reaches `LOG_BYTES`.
struct Rotating {
    path: PathBuf,
    file: Option<std::fs::File>,
    written: u64,
}

impl Rotating {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            file: None,
            written: 0,
        }
    }
}

impl Write for Rotating {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.written + bytes.len() as u64 > LOG_BYTES && self.file.is_some() {
            self.file = None;
            std::fs::rename(&self.path, self.path.with_extension("old.log"))?;
            self.written = 0;
        }
        if self.file.is_none() {
            self.file = Some(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.path)?,
            );
        }
        let written = self.file.as_mut().expect("opened above").write(bytes)?;
        self.written += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.as_mut().map_or(Ok(()), std::fs::File::flush)
    }
}

/// Says that an earlier run crashed, once nothing else is asked; whether
/// it does.
pub fn tell_about_crashes(ctx: &egui::Context, asking: bool) -> bool {
    let mut earlier = EARLIER.lock().expect("no panic while holding it");
    if earlier.is_empty() || asking {
        return false;
    }
    let mut close = false;
    let modal = crate::widgets::dialog(
        ctx,
        "crashed before",
        420.0,
        tr("crash-title"),
        Some(tr("crash-text")),
        |ui| {
            crate::widgets::dialog_buttons(ui, |ui| {
                use crate::widgets::{Emphasis, button};
                if button(ui, tr("crash-close"), Emphasis::Primary, true).clicked() {
                    close = true;
                }
                if button(ui, tr("crash-open-logs"), Emphasis::Secondary, true).clicked() {
                    show_folder();
                    close = true;
                }
            });
        },
    );
    if close || modal.should_close() {
        mark_seen(&earlier);
        earlier.clear();
    }
    true
}

/// Opens the log folder in File Explorer.
pub fn show_folder() {
    if let Some(folder) = folder()
        && let Err(error) = ugu_platform::folder::show(&folder)
    {
        tracing::warn!(error, "cannot show the log folder");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("ugu-logs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn a_full_log_keeps_one_old_part() {
        let folder = scratch("rotate");
        let path = folder.join("ugurugu-20261011-120000-1.log");
        let mut log = Rotating::new(path.clone());
        let line = vec![b'x'; 1024 * 1024];
        for _ in 0..5 {
            log.write_all(&line).unwrap();
        }
        log.flush().unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 1024 * 1024);
        assert_eq!(
            std::fs::metadata(path.with_extension("old.log"))
                .unwrap()
                .len(),
            2 * 1024 * 1024
        );
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn only_the_newest_runs_are_kept_and_unseen_records_stay() {
        let folder = scratch("prune");
        let touch = |name: &str| std::fs::write(folder.join(name), b"").unwrap();
        for minute in 0..5 {
            touch(&format!("ugurugu-20261011-12{minute:02}00-7.log"));
        }
        touch("ugurugu-20261011-120000-7.old.log");
        touch("crash-ugurugu-20261011-120000-7-panic-0.seen.txt");
        touch("crash-ugurugu-20261011-120100-7-native.txt");
        touch("notes.txt");
        prune(&folder, 2);
        let mut left: Vec<String> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "crash-ugurugu-20261011-120100-7-native.txt",
                "notes.txt",
                "ugurugu-20261011-120300-7.log",
                "ugurugu-20261011-120400-7.log",
            ]
        );
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_panic_leaves_a_record() {
        let folder = scratch("panic");
        record_panics(folder.clone(), "ugurugu-20261011-120000-9".to_owned());
        let _ = std::thread::Builder::new()
            .name("doomed".to_owned())
            .spawn(|| panic!("on purpose"))
            .unwrap()
            .join();
        let _ = std::panic::take_hook();
        let record = folder.join("crash-ugurugu-20261011-120000-9-panic-0.txt");
        let text = std::fs::read_to_string(&record).unwrap();
        assert!(text.starts_with("Ugurugu "), "{text}");
        assert!(text.contains("Thread: doomed"), "{text}");
        assert!(text.contains("Panic: on purpose"), "{text}");
        assert!(text.contains("logs.rs"), "{text}");
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn records_are_told_about_once() {
        let folder = scratch("unseen");
        std::fs::write(
            folder.join("crash-ugurugu-20261011-120000-7-panic-0.txt"),
            b"",
        )
        .unwrap();
        std::fs::write(
            folder.join("crash-ugurugu-20261011-110000-3-native.txt"),
            b"",
        )
        .unwrap();
        std::fs::write(folder.join("ugurugu-20261011-120000-7.log"), b"").unwrap();
        let found = unseen(&folder);
        assert_eq!(found.len(), 2);
        assert!(found[0].ends_with("crash-ugurugu-20261011-110000-3-native.txt"));
        mark_seen(&found);
        assert!(unseen(&folder).is_empty());
        assert!(
            folder
                .join("crash-ugurugu-20261011-120000-7-panic-0.seen.txt")
                .exists()
        );
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
