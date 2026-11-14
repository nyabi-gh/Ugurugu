// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 NewfolderGames

//! Linux file and folder dialogs via `org.freedesktop.portal.FileChooser`.
//!
//! Callers invoke [`pick`] on a dedicated `"dialog"` background thread, so
//! blocking on `zbus::block_on` never holds the UI or render thread.

use std::collections::HashMap;
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use futures_lite::StreamExt;
use ugu_platform_types::dialog::{Dialog, FileType, with_extension_of};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Structure, Value};
use zbus::{Connection, MatchRule, MessageStream};

static TOKEN_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Shows `dialog` through `org.freedesktop.portal.FileChooser` parented to
/// `owner` and returns the chosen path, or `None` when cancelled.
///
/// Mirrors `crates/ugu-win/src/dialog.rs`: when the portal returns a save path
/// without an extension and appending the chosen filter's default extension
/// lands on an existing file, the save dialog re-opens pre-filled with that
/// full filename so the desktop portal prompts before replacing it.
pub fn pick(owner: isize, dialog: &Dialog) -> Result<Option<PathBuf>, String> {
    let parent = crate::linux::window::portal_parent_window(owner);
    let mut dialog = dialog.clone();
    let mut chosen_index = None;

    loop {
        let Some((returned, file_type)) = zbus::block_on(show_once(&parent, &dialog, chosen_index))
            .map_err(|error| error.to_string())?
        else {
            return Ok(None);
        };
        let Some((index, file_type)) = file_type else {
            return Ok(Some(returned));
        };
        let path = with_extension_of(returned.clone(), file_type);
        if path == returned || !path.exists() {
            return Ok(Some(path));
        }
        if let Dialog::Save { name, folder, .. } = &mut dialog {
            *name = path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            *folder = path.parent().map(Path::to_path_buf);
        }
        chosen_index = Some(index);
    }
}

type PortalFilter = (String, Vec<(u32, String)>);

async fn show_once(
    parent_window: &str,
    dialog: &Dialog,
    chosen_index: Option<usize>,
) -> zbus::Result<Option<(PathBuf, Option<(usize, FileType)>)>> {
    let conn = Connection::session().await?;
    let unique = conn
        .unique_name()
        .ok_or_else(|| zbus::Error::Failure("D-Bus session connection has no unique name".into()))?
        .as_str()
        .to_owned();

    let token = next_handle_token();
    let predicted_path = predicted_request_path(&unique, &token);

    // Subscribe BEFORE invoking `OpenFile` / `SaveFile` so fast portal
    // responses cannot arrive before the signal match rule is active.
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.portal.Request")?
        .member("Response")?
        .path(predicted_path.as_str())?
        .build();
    let mut stream = MessageStream::for_match_rule(rule, &conn, Some(4)).await?;

    let mut options: HashMap<&str, Value<'_>> = HashMap::new();
    options.insert("handle_token", Value::from(token.as_str()));
    options.insert("modal", Value::from(true));

    let (method, title) = match dialog {
        Dialog::Open(file_type) => {
            let filters = vec![portal_filter(*file_type)];
            options.insert("current_filter", Value::from(filters[0].clone()));
            options.insert("filters", Value::from(filters));
            ("OpenFile", "")
        }
        Dialog::Save {
            file_types,
            name,
            folder,
        } => {
            let filters: Vec<PortalFilter> =
                file_types.iter().copied().map(portal_filter).collect();
            let active = chosen_index
                .unwrap_or(0)
                .min(filters.len().saturating_sub(1));
            if let Some(current) = filters.get(active) {
                options.insert("current_filter", Value::from(current.clone()));
            }
            options.insert("filters", Value::from(filters));
            options.insert("current_name", Value::from(name.as_str()));
            if let Some(dir) = folder
                && dir.is_dir()
                && let Some(bytes) = folder_bytes_with_nul(dir)
            {
                options.insert("current_folder", Value::from(bytes));
            }
            ("SaveFile", "")
        }
        Dialog::Folder { title, start } => {
            options.insert("directory", Value::from(true));
            if let Some(dir) = start
                && dir.is_dir()
                && let Some(bytes) = folder_bytes_with_nul(dir)
            {
                options.insert("current_folder", Value::from(bytes));
            }
            ("OpenFile", title.as_str())
        }
    };

    let reply = conn
        .call_method(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            Some("org.freedesktop.portal.FileChooser"),
            method,
            &(parent_window, title, options),
        )
        .await?;

    let returned_path: OwnedObjectPath = reply.body().deserialize()?;
    if returned_path.as_str() != predicted_path {
        let fallback_rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.freedesktop.portal.Request")?
            .member("Response")?
            .path(returned_path.as_str())?
            .build();
        stream = MessageStream::for_match_rule(fallback_rule, &conn, Some(4)).await?;
    }

    if let Some(msg) = stream.next().await {
        let msg = msg?;
        let (code, results): (u32, HashMap<String, OwnedValue>) = msg.body().deserialize()?;
        if code != 0 {
            // 1 = cancelled by user, 2 = closed otherwise.
            return Ok(None);
        }
        let Some(uris_val) = results.get("uris") else {
            return Ok(None);
        };
        let uris: Vec<String> = Vec::<String>::try_from(uris_val.try_clone()?)?;
        let Some(first_uri) = uris.first() else {
            return Ok(None);
        };
        let path = crate::linux::folder::file_uri_to_path(first_uri).ok_or_else(|| {
            zbus::Error::Failure(format!("portal returned a non-local URI: {first_uri}"))
        })?;

        let chosen_type = match dialog {
            Dialog::Save { file_types, .. } if !file_types.is_empty() => {
                let idx = results
                    .get("current_filter")
                    .and_then(extract_filter_name)
                    .and_then(|filter_name| file_types.iter().position(|ft| ft.name == filter_name))
                    .or(chosen_index)
                    .unwrap_or(0)
                    .min(file_types.len() - 1);
                Some((idx, file_types[idx]))
            }
            _ => None,
        };
        return Ok(Some((path, chosen_type)));
    }

    Err(zbus::Error::Failure(
        "portal closed without sending a Response signal".into(),
    ))
}

fn next_handle_token() -> String {
    let seq = TOKEN_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("ugurugu_{}_{seq}", std::process::id())
}

fn predicted_request_path(unique_name: &str, token: &str) -> String {
    let sender = unique_name.trim_start_matches(':').replace('.', "_");
    format!("/org/freedesktop/portal/desktop/request/{sender}/{token}")
}

/// Portal `current_folder` requires a NUL-terminated byte array (`ay`).
fn folder_bytes_with_nul(folder: &Path) -> Option<Vec<u8>> {
    CString::new(folder.as_os_str().as_bytes())
        .ok()
        .map(CString::into_bytes_with_nul)
}

/// Converts a [`FileType`] into a portal `(name, [(0, case_insensitive_glob)])`
/// tuple (`0` = glob pattern in `org.freedesktop.portal.FileChooser`).
fn portal_filter(file_type: FileType) -> PortalFilter {
    let patterns = file_type
        .extensions
        .iter()
        .map(|ext| (0u32, case_insensitive_glob(ext)))
        .collect();
    (file_type.name.to_owned(), patterns)
}

fn case_insensitive_glob(extension: &str) -> String {
    let mut glob = String::from("*.");
    for ch in extension.chars() {
        if ch.is_ascii_alphabetic() {
            glob.push('[');
            glob.push(ch.to_ascii_lowercase());
            glob.push(ch.to_ascii_uppercase());
            glob.push(']');
        } else {
            glob.push(ch);
        }
    }
    glob
}

fn extract_filter_name(value: &OwnedValue) -> Option<String> {
    let structure: &Structure<'_> = value.downcast_ref().ok()?;
    let first = structure.fields().first()?;
    let name: &str = first.downcast_ref().ok()?;
    Some(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_path_and_case_insensitive_globs_follow_portal_spec() {
        assert_eq!(
            predicted_request_path(":1.204", "ugurugu_42_1"),
            "/org/freedesktop/portal/desktop/request/1_204/ugurugu_42_1"
        );
        assert_eq!(case_insensitive_glob("png"), "*.[pP][nN][gG]");
        assert_eq!(
            case_insensitive_glob("ugurugu"),
            "*.[uU][gG][uU][rR][uU][gG][uU]"
        );
        assert_eq!(case_insensitive_glob("mp4"), "*.[mM][pP]4");
    }

    #[test]
    fn portal_option_values_carry_expected_dbus_signatures() {
        let ft = FileType {
            name: "PNG images",
            extensions: &["png"],
        };
        let single = Value::from(portal_filter(ft));
        assert_eq!(single.value_signature().to_string(), "(sa(us))");

        let list = Value::from(vec![portal_filter(ft)]);
        assert_eq!(list.value_signature().to_string(), "a(sa(us))");

        let folder = Value::from(folder_bytes_with_nul(Path::new("/tmp")).unwrap());
        assert_eq!(folder.value_signature().to_string(), "ay");
    }
}
