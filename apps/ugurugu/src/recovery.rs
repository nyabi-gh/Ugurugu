// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Automatic recovery. Each window keeps its unsaved work in a folder of its
//! own under the recovery root, locked while it runs, as two generations
//! written in turn, so a crash loses only the last few seconds and never
//! another window's work. Everything here touches the disk and runs on the
//! file thread, in order with saves.

use std::fs::{File, OpenOptions};
use std::io::{self, Write as _};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};
use ugu_core::document::Document;

/// How long editing pauses before the work is written.
pub const QUIET: Duration = Duration::from_secs(3);
/// How long work goes unwritten at most while editing goes on.
pub const LONGEST: Duration = Duration::from_secs(30);

const LOCK: &str = "lock";
/// Folders whose work could not be read; kept for the user, never offered.
const FAILED: &str = "failed-";
const FORMAT: &str = "ugurugu.recovery";
const ERROR_SHARING_VIOLATION: i32 = 32;

/// `UGURUGU_RECOVERY_PATH`, else `%LOCALAPPDATA%\Ugurugu\3\recovery`.
pub fn default_root() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("UGURUGU_RECOVERY_PATH") {
        return Some(PathBuf::from(path));
    }
    Some(
        ugu_win::folder::local_app_data()?
            .join("Ugurugu")
            .join("3")
            .join("recovery"),
    )
}

/// What a generation holds, for choosing the newest and for the question
/// at start.
#[derive(Clone, Debug, PartialEq)]
pub struct Meta {
    /// Counts up with each write of the window.
    pub sequence: u64,
    /// The document's name as the title showed it.
    pub name: String,
    /// Where the document was saved, if it ever was.
    pub path: Option<PathBuf>,
    /// When it was written.
    pub time: Duration,
    pub canvas: [u32; 2],
}

impl Meta {
    pub fn new(sequence: u64, name: String, path: Option<PathBuf>, canvas: [u32; 2]) -> Self {
        // Whole seconds, as written.
        let time = Duration::from_secs(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        );
        Self {
            sequence,
            name,
            path,
            time,
            canvas,
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "format": FORMAT,
            "version": 1,
            "sequence": self.sequence,
            "name": self.name,
            "path": self.path.as_ref().map(|path| path.to_string_lossy()),
            "time": self.time.as_secs(),
            "canvas": self.canvas,
        })
    }

    fn parse(value: &Value) -> Option<Self> {
        if value.get("format")?.as_str()? != FORMAT || value.get("version")?.as_u64()? != 1 {
            return None;
        }
        let canvas = value.get("canvas")?.as_array()?;
        let edge = |index: usize| u32::try_from(canvas.get(index)?.as_u64()?).ok();
        Some(Self {
            sequence: value.get("sequence")?.as_u64()?,
            name: value.get("name")?.as_str()?.to_owned(),
            path: value.get("path").and_then(Value::as_str).map(PathBuf::from),
            time: Duration::from_secs(value.get("time")?.as_u64()?),
            canvas: [edge(0)?, edge(1)?],
        })
    }
}

/// The folder of a window that is not running any more, locked by this one
/// until the user decides what becomes of it.
#[derive(Debug)]
pub struct Found {
    pub folder: PathBuf,
    lock: File,
    /// The newest generation's.
    pub meta: Meta,
}

/// Opens a folder's lock with no sharing, which fails while another process
/// has it open, even one that crashed and has not been cleaned up yet.
fn lock(folder: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(folder.join(LOCK))
}

fn document_path(folder: &Path, slot: u64) -> PathBuf {
    folder.join(format!("{slot}.ugurugu"))
}

fn meta_path(folder: &Path, slot: u64) -> PathBuf {
    folder.join(format!("{slot}.json"))
}

/// The generations whose description was written, newest first. A
/// description is written after its document and removed before it is
/// replaced, so each one listed has its document written in full.
fn generations(folder: &Path) -> Vec<(u64, Meta)> {
    let mut found: Vec<_> = (0..2)
        .filter_map(|slot| {
            let text = std::fs::read(meta_path(folder, slot)).ok()?;
            let meta = Meta::parse(&serde_json::from_slice(&text).ok()?)?;
            (meta.sequence % 2 == slot).then_some((slot, meta))
        })
        .collect();
    found.sort_by_key(|(_, meta)| std::cmp::Reverse(meta.sequence));
    found
}

/// Makes and locks this window's folder, `session` under `root`, and finds
/// the folders of windows no longer running, newest first. Folders of
/// running windows are left alone; left folders with nothing written are
/// removed.
pub fn begin(root: &Path, session: &Path) -> io::Result<(File, Vec<Found>)> {
    std::fs::create_dir_all(session)?;
    let own = lock(session)?;
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root)?.flatten() {
        let folder = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if folder == session || name.starts_with(FAILED) || !folder.is_dir() {
            continue;
        }
        let lock = match lock(&folder) {
            Ok(lock) => lock,
            Err(error) if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION) => continue,
            Err(error) => {
                tracing::warn!(%error, folder = %folder.display(), "cannot look at a recovery folder");
                continue;
            }
        };
        match generations(&folder).into_iter().next() {
            Some((_, meta)) => found.push(Found { folder, lock, meta }),
            None => {
                drop(lock);
                if let Err(error) = std::fs::remove_dir_all(&folder) {
                    tracing::warn!(%error, folder = %folder.display(), "cannot remove an empty recovery folder");
                }
            }
        }
    }
    found.sort_by_key(|found| std::cmp::Reverse(found.meta.time));
    Ok((own, found))
}

/// Writes `document` as the generation `meta.sequence` picks, over the older
/// of the two.
pub fn write(folder: &Path, document: &Document, id: [u8; 16], meta: &Meta) -> Result<(), String> {
    let slot = meta.sequence % 2;
    remove(&meta_path(folder, slot)).map_err(|error| error.to_string())?;
    let replace = &ugu_win::file::replace_file;
    ugu_io::save::save(document, id, &document_path(folder, slot), replace)
        .map_err(|error| error.to_string())?;
    let text = meta.to_json().to_string();
    ugu_io::save::replace_with(&meta_path(folder, slot), replace, |mut file, _| {
        file.write_all(text.as_bytes())?;
        Ok(())
    })
    .map_err(|error| error.to_string())
}

/// Removes the generations, keeping the folder and its lock.
pub fn forget(folder: &Path) -> io::Result<()> {
    for slot in 0..2 {
        remove(&meta_path(folder, slot))?;
        remove(&document_path(folder, slot))?;
    }
    Ok(())
}

fn remove(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// The newest generation of `found` that reads, with its description.
pub fn read(found: &Found) -> Result<(Document, [u8; 16], Meta), String> {
    let mut last_error = "nothing was written".to_owned();
    for (slot, meta) in generations(&found.folder) {
        let result = File::open(document_path(&found.folder, slot))
            .map_err(|error| error.to_string())
            .and_then(|file| {
                ugu_io::read::read(io::BufReader::new(file)).map_err(|error| error.to_string())
            });
        match result {
            Ok((document, id)) => return Ok((document, id, meta)),
            Err(error) => {
                tracing::warn!(%error, slot, "a recovery generation does not read");
                last_error = error;
            }
        }
    }
    Err(last_error)
}

/// Removes a folder whose work is not wanted, or that this window took over.
pub fn discard(found: Found) -> io::Result<()> {
    let Found { folder, lock, .. } = found;
    drop(lock);
    std::fs::remove_dir_all(folder)
}

/// Moves a folder whose work could not be read out of the way, where the
/// user can still find it; returns where.
pub fn set_aside(found: Found) -> io::Result<PathBuf> {
    let Found { folder, lock, .. } = found;
    drop(lock);
    let time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let name = folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let aside = folder.with_file_name(format!("{FAILED}{time}-{name}"));
    std::fs::rename(&folder, &aside)?;
    Ok(aside)
}

/// Removes this window's own folder as it closes normally.
pub fn end(folder: &Path, lock: File) -> io::Result<()> {
    drop(lock);
    std::fs::remove_dir_all(folder)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A folder of its own under the temporary folder, removed when dropped.
    pub(crate) struct Root(pub PathBuf);

    impl Root {
        pub(crate) fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("ugurugu-recovery-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn names(&self) -> Vec<String> {
            let mut names: Vec<_> = std::fs::read_dir(&self.0)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }
    }

    impl Drop for Root {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn document(width: u32) -> Document {
        Document::new([width, 10])
    }

    fn meta(sequence: u64) -> Meta {
        Meta::new(sequence, format!("work {sequence}"), None, [10, 10])
    }

    #[test]
    fn the_newest_generation_that_reads_is_used() {
        let root = Root::new("generations");
        let session = root.0.join("crashed");
        let (lock, found) = begin(&root.0, &session).unwrap();
        assert!(found.is_empty());
        for sequence in 1..=3 {
            write(
                &session,
                &document(10 + sequence as u32),
                [7; 16],
                &meta(sequence),
            )
            .unwrap();
        }
        // The window crashes: its lock goes with it.
        drop(lock);
        let (_own, mut found) = begin(&root.0, &root.0.join("next")).unwrap();
        assert_eq!(found.len(), 1);
        let found = found.remove(0);
        assert_eq!(found.meta.sequence, 3);
        let (read_back, id, meta) = read(&found).unwrap();
        assert_eq!((read_back.canvas[0], id, meta.sequence), (13, [7; 16], 3));
        // A cut-off newest generation falls back to the one before.
        let newest = document_path(&session, 1);
        let length = std::fs::metadata(&newest).unwrap().len();
        OpenOptions::new()
            .write(true)
            .open(&newest)
            .unwrap()
            .set_len(length / 2)
            .unwrap();
        let (read_back, _, meta) = read(&found).unwrap();
        assert_eq!((read_back.canvas[0], meta.sequence), (12, 2));
        // A generation whose description was not written yet is not used.
        std::fs::remove_file(meta_path(&session, 0)).unwrap();
        assert!(read(&found).is_err());
        let aside = set_aside(found).unwrap();
        assert!(
            aside
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(FAILED)
        );
        // Set aside, it is not offered again.
        let (_own, found) = begin(&root.0, &root.0.join("third")).unwrap();
        assert!(found.is_empty());
    }

    #[test]
    fn running_windows_keep_their_folders() {
        let root = Root::new("running");
        let running = root.0.join("running");
        let (running_lock, _) = begin(&root.0, &running).unwrap();
        write(&running, &document(10), [1; 16], &meta(1)).unwrap();
        // Left with nothing written: removed by the next start.
        let (empty_lock, _) = begin(&root.0, &root.0.join("empty")).unwrap();
        drop(empty_lock);
        let (own, found) = begin(&root.0, &root.0.join("new")).unwrap();
        assert!(found.is_empty(), "{found:?}");
        assert_eq!(root.names(), ["new", "running"]);
        // Nor can a running window's folder be taken over.
        assert_eq!(
            lock(&running).unwrap_err().raw_os_error(),
            Some(ERROR_SHARING_VIOLATION)
        );
        end(&root.0.join("new"), own).unwrap();
        drop(running_lock);
        assert_eq!(root.names(), ["running"]);
    }

    #[test]
    fn forgetting_removes_the_generations_and_keeps_the_lock() {
        let root = Root::new("forget");
        let session = root.0.join("session");
        let (lock, _) = begin(&root.0, &session).unwrap();
        write(&session, &document(10), [1; 16], &meta(1)).unwrap();
        write(&session, &document(10), [1; 16], &meta(2)).unwrap();
        forget(&session).unwrap();
        assert!(generations(&session).is_empty());
        let mut left: Vec<_> = std::fs::read_dir(&session)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, [LOCK]);
        drop(lock);
    }

    #[test]
    fn descriptions_read_back_and_odd_ones_are_refused() {
        let meta = Meta::new(
            5,
            "Cat".to_owned(),
            Some(PathBuf::from(r"C:\a\Cat.ugurugu")),
            [3, 4],
        );
        assert_eq!(Meta::parse(&meta.to_json()), Some(meta.clone()));
        let mut other = meta.to_json();
        other["format"] = json!("something else");
        assert_eq!(Meta::parse(&other), None);
        let mut large = meta.to_json();
        large["canvas"] = json!([u64::MAX, 4]);
        assert_eq!(Meta::parse(&large), None);
    }
}
