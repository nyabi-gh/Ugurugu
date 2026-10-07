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

use ugu_core::document::Document;

use crate::write::{WriteError, write};

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
    save_through(document, document_id, target, replace, Ok)
}

/// `wrap` stands between the writer and the file, for tests to inject
/// failures such as a full disk.
fn save_through<W: Write + Seek>(
    document: &Document,
    document_id: [u8; 16],
    target: &Path,
    replace: Replace<'_>,
    wrap: impl FnOnce(File) -> io::Result<W>,
) -> Result<(), SaveError> {
    let temporary = temporary_path(target);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        let entries = {
            let out = write(document, document_id, BufWriter::new(wrap(file)?))
                .map_err(SaveError::Write)?;
            out.into_inner()
                .map_err(|error| SaveError::Write(WriteError::Io(error.into_error())))?;
            let file = File::open(&temporary)?;
            zip::ZipArchive::new(file)
                .map_err(|error| SaveError::Check(error.to_string()))?
                .len()
        };
        if entries < 2 {
            return Err(SaveError::Check(format!("{entries} entries")));
        }
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
        let target = folder.0.join("doc.ugu2");
        save(&sample(), [1; 16], &target, &rename).unwrap();
        let (document, id) = read(File::open(&target).unwrap()).unwrap();
        assert_eq!(id, [1; 16]);
        assert_eq!(document.layers, sample().layers);
        assert_eq!(folder.names(), ["doc.ugu2"]);
    }

    #[test]
    fn a_failed_replace_keeps_the_old_file_and_no_temporary() {
        let folder = Folder::new("replace-fails");
        let target = folder.0.join("doc.ugu2");
        std::fs::write(&target, b"old").unwrap();
        let refuse = |_: &Path, _: &Path| Err(io::Error::from(io::ErrorKind::PermissionDenied));
        assert!(matches!(
            save(&sample(), [1; 16], &target, &refuse),
            Err(SaveError::Replace(_))
        ));
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        assert_eq!(folder.names(), ["doc.ugu2"]);
    }

    #[test]
    fn a_full_disk_keeps_the_old_file_and_no_temporary() {
        let folder = Folder::new("disk-full");
        let target = folder.0.join("doc.ugu2");
        std::fs::write(&target, b"old").unwrap();
        let replaced = std::cell::Cell::new(false);
        let replace = |from: &Path, to: &Path| {
            replaced.set(true);
            rename(from, to)
        };
        let result = save_through(&sample(), [1; 16], &target, &replace, |file| {
            Ok(FullDisk { file, left: 300 })
        });
        assert!(matches!(result, Err(SaveError::Write(_))), "{result:?}");
        assert!(!replaced.get());
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        assert_eq!(folder.names(), ["doc.ugu2"]);
    }
}
