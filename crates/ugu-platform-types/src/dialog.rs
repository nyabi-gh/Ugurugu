// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

use std::path::PathBuf;

/// A file type the dialog offers: a name such as "PNG images (*.png)" and its
/// extensions without the dot, the first one added to a saved name that has
/// none.
#[derive(Clone, Copy, Debug)]
pub struct FileType {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
}

#[derive(Clone, Debug)]
pub enum Dialog {
    Open(FileType),
    /// Suggests `name` for the file, in `folder` if given; otherwise the
    /// folder Windows remembers. The first file type is chosen to begin
    /// with; the chosen one's first extension is added to a name without
    /// one of its own.
    Save {
        file_types: Vec<FileType>,
        name: String,
        folder: Option<PathBuf>,
    },
    /// Picks a folder, starting in `start` if given.
    Folder {
        title: String,
        start: Option<PathBuf>,
    },
}

/// `path` with `file_type`'s first extension added unless it already ends
/// in one of the type's.
pub fn with_extension_of(path: PathBuf, file_type: FileType) -> PathBuf {
    let has = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            file_type
                .extensions
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        });
    if has {
        return path;
    }
    let mut name = path.into_os_string();
    name.push(".");
    name.push(file_type.extensions[0]);
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMAGE: FileType = FileType {
        name: "JPEG",
        extensions: &["jpg", "jpeg"],
    };

    #[test]
    fn a_name_gets_the_chosen_type_s_extension_unless_it_has_one() {
        let named = |path: &str| with_extension_of(PathBuf::from(path), IMAGE);
        assert_eq!(named(r"C:\a\cat"), PathBuf::from(r"C:\a\cat.jpg"));
        assert_eq!(named(r"C:\a\cat.JPEG"), PathBuf::from(r"C:\a\cat.JPEG"));
        assert_eq!(named(r"C:\a\cat.png"), PathBuf::from(r"C:\a\cat.png.jpg"));
        assert_eq!(named(r"C:\a\cat.v2"), PathBuf::from(r"C:\a\cat.v2.jpg"));
    }
}
