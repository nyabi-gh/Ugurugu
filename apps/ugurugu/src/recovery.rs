// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Automatic recovery. Each window keeps its unsaved work in a folder of its
//! own under the recovery root, locked while it runs, each tab's in a folder
//! of the tab's in it, as two generations written in turn, so a crash loses
//! only the last few seconds and never another window's work. A window of
//! 3.0 before tabs wrote its one document in its own folder; that is still
//! read. Everything here touches the disk and runs on the file thread, in
//! order with saves.

use std::fs::{File, OpenOptions};
use std::io::{self, Write as _};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};
use ugu_core::document::{Document, LayerId};

/// How long editing pauses before the work is written.
pub const QUIET: Duration = Duration::from_secs(3);
/// How long work goes unwritten at most while editing goes on.
pub const LONGEST: Duration = Duration::from_secs(30);

const LOCK: &str = "lock";
/// Written as a window closes normally, whose work was saved or dropped.
const ENDED: &str = "ended";
/// Folders whose work could not be read; kept for the user, never offered.
const FAILED: &str = "failed-";
/// How long they are kept.
const FAILED_KEPT: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// A window makes its folder and locks it at once, so one this old without
/// a lock is not being made: its window stopped in between, or a normal end
/// stopped removing it.
const STARTING: Duration = Duration::from_secs(60);
const FORMAT: &str = "ugurugu.recovery";
const ERROR_SHARING_VIOLATION: i32 = 32;

/// `UGURUGU_RECOVERY_PATH`, else `recovery` in [`crate::logs::local_data`].
pub fn default_root() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("UGURUGU_RECOVERY_PATH") {
        return Some(PathBuf::from(path));
    }
    Some(crate::logs::local_data()?.join("recovery"))
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
    /// The layer being edited, chosen again when the work is recovered.
    pub layer: Option<LayerId>,
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
            layer: None,
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
            "layer": self.layer.map(|layer| layer.0),
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
            layer: value
                .get("layer")
                .and_then(Value::as_u64)
                .and_then(|layer| u32::try_from(layer).ok())
                .map(LayerId),
        })
    }
}

/// The folder of a window that is not running any more, locked by this one
/// until the user decides what becomes of all its work. It goes then if
/// nothing is left in it.
#[derive(Debug)]
struct Left {
    folder: PathBuf,
    lock: Option<File>,
}

impl Drop for Left {
    fn drop(&mut self) {
        drop(self.lock.take());
        if holds_nothing(&self.folder)
            && let Err(error) = std::fs::remove_dir_all(&self.folder)
        {
            tracing::warn!(%error, folder = %self.folder.display(), "cannot remove an emptied recovery folder");
        }
    }
}

/// Whether nothing but a lock and temporary files is in `folder`. Its
/// window has ended, so a temporary file is one a write left unfinished.
fn holds_nothing(folder: &Path) -> bool {
    std::fs::read_dir(folder).is_ok_and(|mut entries| {
        entries.all(|entry| {
            entry.is_ok_and(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name == LOCK || (name.starts_with('.') && name.ends_with(".tmp"))
            })
        })
    })
}

/// One document a window no longer running left: a tab's, or the window's
/// own as before tabs.
#[derive(Debug)]
pub struct Found {
    /// Where its generations are.
    pub folder: PathBuf,
    /// The window's folder, held for as long as any of its work is.
    window: Arc<Left>,
    /// The newest generation's.
    pub meta: Meta,
}

/// Where the tab `tab` of the window whose folder is `window` keeps its work.
pub fn tab_folder(window: &Path, tab: u64) -> PathBuf {
    window.join(format!("tab-{tab}"))
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

/// As `lock`, for another window's folder. One without a lock made less
/// than `starting` ago is being made by a window starting now, so it is not
/// taken: `NotFound`.
fn lock_left(folder: &Path, starting: Duration) -> io::Result<File> {
    let made = std::fs::metadata(folder)?.created()?;
    if SystemTime::now()
        .duration_since(made)
        .is_ok_and(|age| age >= starting)
    {
        return lock(folder);
    }
    OpenOptions::new()
        .read(true)
        .write(true)
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
/// replaced, so each one listed has its document written in full. Without
/// any, as when the window stopped between the first document and its
/// description, the documents that read are listed, newest first, with
/// what they tell of themselves.
fn generations(folder: &Path) -> Vec<(u64, Meta)> {
    let mut found: Vec<_> = (0..2)
        .filter_map(|slot| {
            let text = std::fs::read(meta_path(folder, slot)).ok()?;
            let meta = Meta::parse(&serde_json::from_slice(&text).ok()?)?;
            (meta.sequence % 2 == slot).then_some((slot, meta))
        })
        .collect();
    if found.is_empty() {
        found = (0..2)
            .filter_map(|slot| Some((slot, undescribed(&document_path(folder, slot))?)))
            .collect();
        found.sort_by_key(|(_, meta)| std::cmp::Reverse(meta.time));
        return found;
    }
    found.sort_by_key(|(_, meta)| std::cmp::Reverse(meta.sequence));
    found
}

/// A description for the document at `path` written without one: untitled,
/// never saved, from when the file was written.
fn undescribed(path: &Path) -> Option<Meta> {
    let file = File::open(path).ok()?;
    let time = file
        .metadata()
        .and_then(|metadata| metadata.modified())
        .ok()?
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()?;
    let (document, _) = ugu_io::read::read(io::BufReader::new(file))
        .inspect_err(|error| tracing::warn!(%error, path = %path.display(), "a recovery document without a description does not read"))
        .ok()?;
    Some(Meta {
        sequence: 0,
        name: crate::i18n::tr("untitled").to_owned(),
        path: None,
        time: Duration::from_secs(time.as_secs()),
        canvas: document.canvas,
        layer: None,
    })
}

/// Makes and locks this window's folder, `session` under `root`, and finds
/// the work of windows no longer running, each document of theirs on its
/// own, newest first. Folders of running windows are left alone; left
/// folders with no document are removed, and documents that do not read are
/// set aside.
pub fn begin(root: &Path, session: &Path) -> io::Result<(File, Vec<Found>)> {
    begin_with(root, session, STARTING)
}

fn begin_with(root: &Path, session: &Path, starting: Duration) -> io::Result<(File, Vec<Found>)> {
    std::fs::create_dir_all(session)?;
    let own = lock(session)?;
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root)?.flatten() {
        let folder = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if folder == session || !folder.is_dir() {
            continue;
        }
        if name.starts_with(FAILED) {
            if failed_long_ago(&name)
                && let Err(error) = std::fs::remove_dir_all(&folder)
            {
                tracing::warn!(%error, folder = %folder.display(), "cannot remove old recovery work that did not read");
            }
            continue;
        }
        let lock = match lock_left(&folder, starting) {
            Ok(lock) => lock,
            Err(error)
                if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION)
                    || error.kind() == io::ErrorKind::NotFound =>
            {
                continue;
            }
            Err(error) => {
                tracing::warn!(%error, folder = %folder.display(), "cannot look at a recovery folder");
                continue;
            }
        };
        if folder.join(ENDED).exists() {
            drop(lock);
            if let Err(error) = std::fs::remove_dir_all(&folder) {
                tracing::warn!(%error, folder = %folder.display(), "cannot remove a recovery folder a window closed with");
            }
            continue;
        }
        let window = Arc::new(Left {
            folder: folder.clone(),
            lock: Some(lock),
        });
        // The window's own document from before tabs, then its tabs'.
        let mut places = vec![folder.clone()];
        match std::fs::read_dir(&folder) {
            Ok(entries) => places.extend(
                entries
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| path.is_dir()),
            ),
            // Its tabs are left for a later start; the other folders go on.
            Err(error) => {
                tracing::warn!(%error, folder = %folder.display(), "cannot look in a recovery folder");
                continue;
            }
        }
        for place in places {
            match generations(&place).into_iter().next() {
                Some((_, meta)) => found.push(Found {
                    folder: place,
                    window: window.clone(),
                    meta,
                }),
                None if (0..2).any(|slot| document_path(&place, slot).exists()) => {
                    let unread = Found {
                        folder: place,
                        window: window.clone(),
                        meta: Meta::new(0, String::new(), None, [0, 0]),
                    };
                    match set_aside(unread) {
                        Ok(aside) => {
                            tracing::warn!(folder = %aside.display(), "recovery documents that do not read were set aside")
                        }
                        Err(error) => tracing::warn!(%error, "cannot set aside recovery documents"),
                    }
                }
                None if place != folder => {
                    if let Err(error) = std::fs::remove_dir_all(&place) {
                        tracing::warn!(%error, folder = %place.display(), "cannot remove an empty recovery folder");
                    }
                }
                None => {}
            }
        }
    }
    found.sort_by_key(|found| std::cmp::Reverse(found.meta.time));
    Ok((own, found))
}

/// Writes `document` as the generation `meta.sequence` picks, over the older
/// of the two.
pub fn write(folder: &Path, document: &Document, id: [u8; 16], meta: &Meta) -> Result<(), String> {
    std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    let slot = meta.sequence % 2;
    remove(&meta_path(folder, slot)).map_err(|error| error.to_string())?;
    let replace = &ugu_platform::file::replace_file;
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

/// The newest generation of `found` that reads, with its description; if
/// none does, a document left without its description that reads.
pub fn read(found: &Found) -> Result<(Document, [u8; 16], Meta), String> {
    let mut last_error = "nothing was written".to_owned();
    let described = generations(&found.folder);
    let rest: Vec<(u64, Meta)> = (0..2)
        .filter(|slot| described.iter().all(|(each, _)| each != slot))
        .filter_map(|slot| Some((slot, undescribed(&document_path(&found.folder, slot))?)))
        .collect();
    for (slot, meta) in described.into_iter().chain(rest) {
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

/// Removes a tab's folder, as the tab closes.
pub fn remove_tab(folder: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(folder) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Removes work that is not wanted, or that this window took over; its
/// window's folder goes with the last of it.
pub fn discard(found: Found) -> io::Result<()> {
    if found.folder == found.window.folder {
        forget(&found.folder)
    } else {
        remove_tab(&found.folder)
    }
}

/// Whether a folder set aside as `name` was set aside longer ago than it is
/// kept.
fn failed_long_ago(name: &str) -> bool {
    let Some(seconds) = name
        .strip_prefix(FAILED)
        .and_then(|rest| rest.split('-').next())
        .and_then(|time| time.parse::<u64>().ok())
    else {
        return false;
    };
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    now.saturating_sub(Duration::from_secs(seconds)) > FAILED_KEPT
}

/// Moves a folder whose work could not be read out of the way, where the
/// user can still find it for `FAILED_KEPT`; returns where.
pub fn set_aside(found: Found) -> io::Result<PathBuf> {
    let window = &found.window.folder;
    let time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let name_of = |folder: &Path| {
        folder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let mut name = format!("{FAILED}{time}-{}", name_of(window));
    if found.folder != *window {
        name = format!("{name}-{}", name_of(&found.folder));
    }
    let aside = window.with_file_name(name);
    std::fs::create_dir_all(&aside)?;
    for slot in 0..2 {
        for path in [
            document_path(&found.folder, slot),
            meta_path(&found.folder, slot),
        ] {
            if path.exists() {
                std::fs::rename(&path, aside.join(path.file_name().unwrap_or_default()))?;
            }
        }
    }
    if found.folder != *window {
        remove_tab(&found.folder)?;
    }
    Ok(aside)
}

/// Removes this window's own folder as it closes normally, the lock last,
/// so a removal that stops part way leaves a locked folder marked as ended,
/// which a later start removes without offering what is left in it.
pub fn end(folder: &Path, lock: File) -> io::Result<()> {
    File::create(folder.join(ENDED))?;
    for entry in std::fs::read_dir(folder)? {
        let entry = entry?;
        if entry.file_name() == LOCK || entry.file_name() == ENDED {
            continue;
        }
        if entry.file_type()?.is_dir() {
            std::fs::remove_dir_all(entry.path())?;
        } else {
            std::fs::remove_file(entry.path())?;
        }
    }
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
        // Without any description, the documents that read are used.
        std::fs::remove_file(meta_path(&session, 0)).unwrap();
        let (read_back, _, meta) = read(&found).unwrap();
        assert_eq!((read_back.canvas[0], meta.canvas[0]), (12, 12));
        assert_eq!(meta.name, crate::i18n::tr("untitled"));
        // Nor with none that reads.
        std::fs::remove_file(document_path(&session, 0)).unwrap();
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
    fn a_document_written_without_its_description_is_offered_not_removed() {
        let root = Root::new("undescribed");
        let crashed = root.0.join("crashed");
        let (lock, _) = begin(&root.0, &crashed).unwrap();
        write(&crashed, &document(21), [3; 16], &meta(1)).unwrap();
        // Stopped before the description of the first document was written.
        std::fs::remove_file(meta_path(&crashed, 1)).unwrap();
        drop(lock);
        let (own, mut found) = begin(&root.0, &root.0.join("next")).unwrap();
        assert_eq!(found.len(), 1);
        let found = found.remove(0);
        assert_eq!(found.meta.canvas, [21, 10]);
        assert_eq!(read(&found).unwrap().0.canvas, [21, 10]);
        drop(found);
        drop(own);

        // One that does not read either is set aside, never removed.
        let broken = root.0.join("broken");
        std::fs::create_dir_all(&broken).unwrap();
        File::create(broken.join(LOCK)).unwrap();
        std::fs::write(document_path(&broken, 0), b"not a document").unwrap();
        let (_own, found) = begin(&root.0, &root.0.join("third")).unwrap();
        assert!(found.iter().all(|found| found.folder != broken));
        assert!(!broken.exists());
        assert!(
            root.names()
                .iter()
                .any(|name| name.starts_with(FAILED) && name.ends_with("broken"))
        );
    }

    #[test]
    fn each_tab_s_work_is_found_on_its_own_and_the_window_goes_with_the_last() {
        let root = Root::new("tabs");
        let crashed = root.0.join("crashed");
        let (held, _) = begin(&root.0, &crashed).unwrap();
        write(&tab_folder(&crashed, 1), &document(11), [1; 16], &meta(1)).unwrap();
        write(&tab_folder(&crashed, 4), &document(14), [4; 16], &meta(1)).unwrap();
        // A tab closed before the crash left nothing.
        std::fs::create_dir_all(tab_folder(&crashed, 7)).unwrap();
        drop(held);
        let (_own, mut found) = begin(&root.0, &root.0.join("next")).unwrap();
        assert!(!tab_folder(&crashed, 7).exists());
        found.sort_by_key(|found| found.folder.clone());
        let widths: Vec<u32> = found
            .iter()
            .map(|found| read(found).unwrap().0.canvas[0])
            .collect();
        assert_eq!(widths, [11, 14]);
        let last = found.pop().unwrap();
        discard(found.pop().unwrap()).unwrap();
        // The other is still there, and its window's folder with it.
        assert!(crashed.exists() && last.folder.exists());
        assert_eq!(
            lock(&crashed).unwrap_err().raw_os_error(),
            Some(ERROR_SHARING_VIOLATION)
        );
        discard(last).unwrap();
        assert!(!crashed.exists());
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
    fn a_folder_a_starting_window_has_made_but_not_locked_is_left_to_it() {
        let root = Root::new("starting");
        let starting = root.0.join("starting");
        std::fs::create_dir_all(&starting).unwrap();
        let (_own, found) = begin(&root.0, &root.0.join("next")).unwrap();
        assert!(found.is_empty());
        assert!(starting.exists());
        assert!(begin(&root.0, &starting).is_ok());
    }

    #[test]
    fn a_folder_left_without_a_lock_is_taken_once_no_window_can_be_making_it() {
        let root = Root::new("lockless");
        let crashed = root.0.join("crashed");
        write(&tab_folder(&crashed, 1), &document(11), [1; 16], &meta(1)).unwrap();
        std::fs::create_dir_all(root.0.join("emptied")).unwrap();
        let (_own, mut found) = begin_with(&root.0, &root.0.join("next"), Duration::ZERO).unwrap();
        assert_eq!(found.len(), 1);
        discard(found.pop().unwrap()).unwrap();
        assert_eq!(root.names(), ["next"]);
    }

    #[test]
    fn a_normal_end_removes_the_lock_last() {
        let root = Root::new("end");
        let session = root.0.join("session");
        let (own, _) = begin(&root.0, &session).unwrap();
        write(&tab_folder(&session, 1), &document(11), [1; 16], &meta(1)).unwrap();
        // Held open, as by a virus scanner, so the removal stops.
        let held = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(document_path(&tab_folder(&session, 1), 1))
            .unwrap();
        assert!(end(&session, own).is_err());
        assert!(session.join(LOCK).exists());
        drop(held);
        let (_own, found) = begin(&root.0, &root.0.join("next")).unwrap();
        assert!(found.is_empty());
        assert_eq!(root.names(), ["next"]);
    }

    #[test]
    fn a_left_folder_that_cannot_be_looked_in_does_not_stop_the_start() {
        let root = Root::new("unlisted");
        let crashed = root.0.join("crashed");
        let (held, _) = begin(&root.0, &crashed).unwrap();
        write(&tab_folder(&crashed, 1), &document(11), [1; 16], &meta(1)).unwrap();
        drop(held);
        let everyone_may_not_list = |how: &str| {
            let status = std::process::Command::new("icacls")
                .arg(&crashed)
                .args(how.split(' '))
                .stdout(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(status.success(), "icacls {how}");
        };
        everyone_may_not_list("/deny *S-1-1-0:(RD)");
        let begun = begin(&root.0, &root.0.join("next"));
        everyone_may_not_list("/remove:d *S-1-1-0");
        let (_own, found) = begun.unwrap();
        assert!(found.is_empty());
        // Its work is offered at a later start.
        let (_own, found) = begin(&root.0, &root.0.join("later")).unwrap();
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn a_left_window_folder_goes_with_its_work_despite_unfinished_writes() {
        let root = Root::new("unfinished");
        let crashed = root.0.join("crashed");
        let (held, _) = begin(&root.0, &crashed).unwrap();
        // As a window from before tabs wrote it.
        write(&crashed, &document(11), [1; 16], &meta(1)).unwrap();
        std::fs::write(crashed.join(".1.ugurugu.4242-0.tmp"), b"part").unwrap();
        drop(held);
        let (_own, mut found) = begin(&root.0, &root.0.join("next")).unwrap();
        discard(found.pop().unwrap()).unwrap();
        assert!(!crashed.exists());
    }

    #[test]
    fn work_that_did_not_read_is_kept_thirty_days() {
        let root = Root::new("failed-age");
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let day = 24 * 60 * 60;
        for (name, age) in [("old", 31 * day), ("recent", 29 * day)] {
            std::fs::create_dir_all(root.0.join(format!("{FAILED}{}-{name}", now - age))).unwrap();
        }
        let (_own, _) = begin(&root.0, &root.0.join("next")).unwrap();
        assert_eq!(
            root.names(),
            [
                format!("{FAILED}{}-recent", now - 29 * day),
                "next".to_owned()
            ]
        );
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
        let meta = Meta {
            layer: Some(LayerId(7)),
            ..Meta::new(
                5,
                "Cat".to_owned(),
                Some(PathBuf::from(r"C:\a\Cat.ugurugu")),
                [3, 4],
            )
        };
        assert_eq!(Meta::parse(&meta.to_json()), Some(meta.clone()));
        let mut without = meta.to_json();
        without.as_object_mut().unwrap().remove("layer");
        assert_eq!(Meta::parse(&without).unwrap().layer, None);
        let mut other = meta.to_json();
        other["format"] = json!("something else");
        assert_eq!(Meta::parse(&other), None);
        let mut large = meta.to_json();
        large["canvas"] = json!([u64::MAX, 4]);
        assert_eq!(Meta::parse(&large), None);
    }
}
