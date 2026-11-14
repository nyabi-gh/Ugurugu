// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 NewfolderGames

//! XDG Base Directory and user directory resolution on Linux, plus opening
//! folders in the desktop's file manager.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};

/// RFC 3986 path segment characters that must be percent-encoded in a
/// `file://` URI (everything outside unreserved + `/` + `:`).
const URI_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

/// `$XDG_CONFIG_HOME` when set to an absolute path, else `$HOME/.config`.
pub fn roaming_app_data() -> Option<PathBuf> {
    xdg_dir(
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        home_dir().as_deref(),
        ".config",
    )
}

/// `$XDG_STATE_HOME` when set to an absolute path, else `$HOME/.local/state`.
pub fn local_app_data() -> Option<PathBuf> {
    xdg_dir(
        std::env::var_os("XDG_STATE_HOME").as_deref(),
        home_dir().as_deref(),
        ".local/state",
    )
}

/// `XDG_DOCUMENTS_DIR` from `$XDG_CONFIG_HOME/user-dirs.dirs` when configured
/// to an absolute path, else `$HOME/Documents`.
pub fn documents() -> Option<PathBuf> {
    let home = home_dir()?;
    let config = roaming_app_data()?;
    if let Ok(contents) = std::fs::read_to_string(config.join("user-dirs.dirs"))
        && let Some(dir) = parse_user_documents_dir(&contents, &home)
    {
        return Some(dir);
    }
    Some(home.join("Documents"))
}

fn home_dir() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    home.is_absolute().then_some(home)
}

fn xdg_dir(explicit: Option<&OsStr>, home: Option<&Path>, fallback_rel: &str) -> Option<PathBuf> {
    // The XDG Base Directory Specification requires ignoring relative paths.
    if let Some(raw) = explicit
        && !raw.is_empty()
    {
        let candidate = PathBuf::from(raw);
        if candidate.is_absolute() {
            return Some(candidate);
        }
    }
    Some(home?.join(fallback_rel))
}

fn parse_user_documents_dir(contents: &str, home: &Path) -> Option<PathBuf> {
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(rest) = line.strip_prefix("XDG_DOCUMENTS_DIR=") else {
            continue;
        };
        let unquoted = rest
            .trim()
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(rest.trim());
        let resolved = if unquoted == "$HOME" {
            home.to_path_buf()
        } else if let Some(suffix) = unquoted.strip_prefix("$HOME/") {
            home.join(suffix)
        } else {
            PathBuf::from(unquoted)
        };
        if resolved.is_absolute() {
            return Some(resolved);
        }
    }
    None
}

/// Encodes an absolute filesystem `path` as a `file://` URI.
pub(crate) fn path_to_file_uri(path: &Path) -> Result<String, String> {
    let absolute = std::path::absolute(path).map_err(|error| error.to_string())?;
    let mut uri = String::from("file://");
    for &byte in absolute.as_os_str().as_bytes() {
        if byte.is_ascii() {
            let ch = [byte];
            let s = std::str::from_utf8(&ch).expect("ASCII byte");
            uri.push_str(&utf8_percent_encode(s, URI_ENCODE_SET).to_string());
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    Ok(uri)
}

/// Decodes a `file://` URI into a local [`PathBuf`].
pub(crate) fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.trim().strip_prefix("file://")?;
    let raw_path = rest
        .strip_prefix("localhost/")
        .map(|tail| format!("/{tail}"))
        .unwrap_or_else(|| rest.to_owned());
    if !raw_path.starts_with('/') {
        return None;
    }
    let bytes: Vec<u8> = percent_decode_str(&raw_path).collect();
    if bytes.contains(&0) {
        return None;
    }
    Some(PathBuf::from(OsString::from_vec(bytes)))
}

/// Opens `folder` in the desktop's file manager via
/// `org.freedesktop.FileManager1.ShowFolders`, falling back to `xdg-open` when
/// no D-Bus file manager service is available.
pub fn show(folder: &Path) -> Result<(), String> {
    let uri = path_to_file_uri(folder)?;
    let folder_buf = folder.to_path_buf();
    std::thread::Builder::new()
        .name("show-folder".to_owned())
        .spawn(move || {
            if show_via_dbus(&uri).is_ok() {
                return;
            }
            if let Err(error) = std::process::Command::new("xdg-open")
                .arg(&folder_buf)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                tracing::warn!(%error, folder = %folder_buf.display(), "cannot open the folder");
            }
        })
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn show_via_dbus(uri: &str) -> Result<(), zbus::Error> {
    zbus::block_on(async {
        let conn = zbus::Connection::session().await?;
        let uris = vec![uri];
        conn.call_method(
            Some("org.freedesktop.FileManager1"),
            "/org/freedesktop/FileManager1",
            Some("org.freedesktop.FileManager1"),
            "ShowFolders",
            &(uris, ""),
        )
        .await?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xdg_directories_ignore_relative_paths_and_fall_back_to_home() {
        let home = Path::new("/home/painter");
        assert_eq!(
            xdg_dir(None, Some(home), ".config"),
            Some(PathBuf::from("/home/painter/.config"))
        );
        assert_eq!(
            xdg_dir(Some(OsStr::new("")), Some(home), ".config"),
            Some(PathBuf::from("/home/painter/.config"))
        );
        assert_eq!(
            xdg_dir(Some(OsStr::new("relative/cfg")), Some(home), ".config"),
            Some(PathBuf::from("/home/painter/.config"))
        );
        assert_eq!(
            xdg_dir(
                Some(OsStr::new("/var/home/painter/.config")),
                Some(home),
                ".config"
            ),
            Some(PathBuf::from("/var/home/painter/.config"))
        );
    }

    #[test]
    fn user_dirs_documents_entry_expands_home_and_absolute_paths() {
        let home = Path::new("/home/painter");
        let file = "\
# Written by xdg-user-dirs-update
XDG_DESKTOP_DIR=\"$HOME/Desktop\"
XDG_DOCUMENTS_DIR=\"$HOME/문서\"
";
        assert_eq!(
            parse_user_documents_dir(file, home),
            Some(PathBuf::from("/home/painter/문서"))
        );
        assert_eq!(
            parse_user_documents_dir("XDG_DOCUMENTS_DIR=\"/mnt/work/docs\"\n", home),
            Some(PathBuf::from("/mnt/work/docs"))
        );
        assert_eq!(
            parse_user_documents_dir("XDG_DOCUMENTS_DIR=\"relative/docs\"\n", home),
            None
        );
    }

    #[test]
    fn file_uris_round_trip_spaces_percent_signs_and_unicode() {
        let original = Path::new("/tmp/ugurugu test 100%/그림 #1.ugurugu");
        let uri = path_to_file_uri(original).unwrap();
        assert_eq!(
            uri,
            "file:///tmp/ugurugu%20test%20100%25/%EA%B7%B8%EB%A6%BC%20%231.ugurugu"
        );
        assert_eq!(file_uri_to_path(&uri).as_deref(), Some(original));
        assert_eq!(
            file_uri_to_path("file://localhost/tmp/a%20b.png"),
            Some(PathBuf::from("/tmp/a b.png"))
        );
        assert_eq!(file_uri_to_path("https://example.com/a.png"), None);
    }

    #[test]
    fn resolved_folders_are_absolute_when_home_is_set() {
        if std::env::var_os("HOME").is_some_and(|h| Path::new(&h).is_absolute()) {
            for folder in [roaming_app_data(), local_app_data(), documents()] {
                let folder = folder.expect("an XDG folder");
                assert!(folder.is_absolute(), "{folder:?}");
            }
        }
    }
}
