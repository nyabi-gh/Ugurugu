// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Saving without risking the file already on disk.
//!
//! The document goes to a new temporary file next to the target, which is
//! finished, flushed to disk and checked before it replaces the target. Any
//! failure removes the temporary file and leaves the target as it was. The
//! replacing itself is the platform's (`ugu_win::file::replace_file` on
//! Windows), passed in so that it can be tested.

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Seek, Write};
use std::path::{Path, PathBuf};

use ugu_core::document::{Document, LayerId};

use crate::write::{WriteError, write_with};

/// Puts a finished temporary file in place of the target.
pub type Replace<'a> = &'a dyn Fn(&Path, &Path) -> io::Result<()>;

#[derive(Debug)]
pub enum SaveError {
    Write(WriteError),
    /// The written file did not read back as expected.
    Check(String),
    Replace(io::Error),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Write(error) => error.fmt(f),
            Self::Check(reason) => write!(f, "the saved file could not be checked: {reason}"),
            Self::Replace(error) => {
                write!(f, "the saved file could not replace the old one: {error}")
            }
        }
    }
}

impl std::error::Error for SaveError {}

impl From<io::Error> for SaveError {
    fn from(error: io::Error) -> Self {
        Self::Write(WriteError::Io(error))
    }
}

/// Saves `document` to `target`.
pub fn save(
    document: &Document,
    document_id: [u8; 16],
    target: &Path,
    replace: Replace<'_>,
) -> Result<(), SaveError> {
    save_with(document, document_id, None, target, replace)
}

/// [`save`] with the layer being edited, to choose again when opened.
pub fn save_with(
    document: &Document,
    document_id: [u8; 16],
    active_layer: Option<LayerId>,
    target: &Path,
    replace: Replace<'_>,
) -> Result<(), SaveError> {
    save_through(document, document_id, active_layer, target, replace, Ok)
}

/// `wrap` stands between the writer and the file, for tests to inject
/// failures such as a full disk.
fn save_through<W: Write + Seek>(
    document: &Document,
    document_id: [u8; 16],
    active_layer: Option<LayerId>,
    target: &Path,
    replace: Replace<'_>,
    wrap: impl FnOnce(File) -> io::Result<W>,
) -> Result<(), SaveError> {
    replace_with(target, replace, |file, temporary| {
        let out = write_with(
            document,
            document_id,
            active_layer,
            BufWriter::new(wrap(file)?),
        )
        .map_err(SaveError::Write)?;
        out.into_inner()
            .map_err(|error| SaveError::Write(WriteError::Io(error.into_error())))?;
        let entries = zip::ZipArchive::new(File::open(temporary)?)
            .map_err(|error| SaveError::Check(error.to_string()))?
            .len();
        if entries < 2 {
            return Err(SaveError::Check(format!("{entries} entries")));
        }
        Ok(())
    })
}

/// Makes a new file next to `target` with `fill`, which gets it and its
/// path, flushes it to disk and puts it in place of `target`. On any failure
/// the new file is removed and `target` stays as it was.
pub fn replace_with(
    target: &Path,
    replace: Replace<'_>,
    fill: impl FnOnce(File, &Path) -> Result<(), SaveError>,
) -> Result<(), SaveError> {
    let temporary = temporary_path(target);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        fill(file, &temporary)?;
        // Flush through any cache to the disk before the old file is given up.
        OpenOptions::new()
            .write(true)
            .open(&temporary)?
            .sync_all()?;
        replace(&temporary, target).map_err(SaveError::Replace)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// A name in the target's folder, so that replacing is a rename on one
/// volume, and unlikely to collide: `.<name>.<pid>-<n>.tmp`.
/// Temporary files for `target` that another process started, with that
/// process's id. One killed while writing leaves its file behind; the caller
/// removes those whose process has ended.
pub fn others_temporaries(target: &Path) -> Vec<(PathBuf, u32)> {
    let (Some(folder), Some(name)) = (target.parent(), target.file_name()) else {
        return Vec::new();
    };
    let prefix = format!(".{}.", name.to_string_lossy());
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let file = path.file_name()?.to_str()?;
            let (pid, _) = file
                .strip_prefix(&prefix)?
                .strip_suffix(".tmp")?
                .split_once('-')?;
            let pid: u32 = pid.parse().ok()?;
            (pid != std::process::id()).then_some((path, pid))
        })
        .collect()
}

fn temporary_path(target: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let unique = NEXT.fetch_add(1, Ordering::Relaxed);
    target.with_file_name(format!(".{name}.{}-{unique}.tmp", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read::read;
    use crate::tests::sample;

    #[test]
    fn temporaries_of_other_processes_are_found_by_their_name() {
        let folder = Folder::new("others");
        let target = folder.0.join("drawing.ugurugu");
        let other = folder.0.join(".drawing.ugurugu.4242-0.tmp");
        let own = temporary_path(&target);
        for path in [
            &other,
            &own,
            &folder.0.join(".other.ugurugu.4242-0.tmp"),
            &folder.0.join(".drawing.ugurugu.x-0.tmp"),
        ] {
            std::fs::write(path, b"part").unwrap();
        }
        assert_eq!(others_temporaries(&target), [(other, 4242)]);
    }

    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("ugu-io-{name}-{}", std::process::id()));
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

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn rename(from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }

    /// Fails every write after `left` bytes, as a full disk does.
    struct FullDisk {
        file: File,
        left: usize,
    }

    impl Write for FullDisk {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if buf.len() > self.left {
                return Err(io::Error::from(io::ErrorKind::StorageFull));
            }
            self.left -= buf.len();
            self.file.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.file.flush()
        }
    }

    impl Seek for FullDisk {
        fn seek(&mut self, position: io::SeekFrom) -> io::Result<u64> {
            self.file.seek(position)
        }
    }

    #[test]
    fn a_saved_document_reads_back() {
        let folder = Folder::new("save");
        let target = folder.0.join("doc.ugurugu");
        save(&sample(), [1; 16], &target, &rename).unwrap();
        let (document, id) = read(File::open(&target).unwrap()).unwrap();
        assert_eq!(id, [1; 16]);
        assert_eq!(document.layers, sample().layers);
        assert_eq!(folder.names(), ["doc.ugurugu"]);
    }

    #[test]
    fn a_failed_replace_keeps_the_old_file_and_no_temporary() {
        let folder = Folder::new("replace-fails");
        let target = folder.0.join("doc.ugurugu");
        std::fs::write(&target, b"old").unwrap();
        let refuse = |_: &Path, _: &Path| Err(io::Error::from(io::ErrorKind::PermissionDenied));
        assert!(matches!(
            save(&sample(), [1; 16], &target, &refuse),
            Err(SaveError::Replace(_))
        ));
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        assert_eq!(folder.names(), ["doc.ugurugu"]);
    }

    #[test]
    fn a_full_disk_keeps_the_old_file_and_no_temporary() {
        let folder = Folder::new("disk-full");
        let target = folder.0.join("doc.ugurugu");
        std::fs::write(&target, b"old").unwrap();
        let replaced = std::cell::Cell::new(false);
        let replace = |from: &Path, to: &Path| {
            replaced.set(true);
            rename(from, to)
        };
        let result = save_through(&sample(), [1; 16], None, &target, &replace, |file| {
            Ok(FullDisk { file, left: 300 })
        });
        assert!(matches!(result, Err(SaveError::Write(_))), "{result:?}");
        assert!(!replaced.get());
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        assert_eq!(folder.names(), ["doc.ugurugu"]);
    }
}
