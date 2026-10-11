// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The `.ugurugu` file association, for the current user only so that no
//! administrator is needed. The installer's hooks register it and the
//! uninstaller's remove it; 2.x's `.ugu` is left alone.

use std::path::Path;

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteTreeW, RegGetValueW, RegSetKeyValueW,
};
use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};

/// Where per-user file types live.
pub const CLASSES: &str = r"Software\Classes";
const EXTENSION: &str = ".ugurugu";
const PROG_ID: &str = "Ugurugu.Document.3";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// Sets `key`'s value `name` (the default value when `None`) to `data`.
fn set(key: &str, name: Option<&str>, data: &str) -> Result<(), String> {
    let (key_wide, data_wide) = (wide(key), wide(data));
    let name_wide = name.map(wide);
    // SAFETY: every string is null-terminated and the data's size is given
    // in bytes, terminator included.
    let status = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key_wide.as_ptr(),
            name_wide
                .as_ref()
                .map_or(std::ptr::null(), |name| name.as_ptr()),
            REG_SZ,
            data_wide.as_ptr().cast(),
            (data_wide.len() * 2) as u32,
        )
    };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("cannot write HKCU\\{key}: error {status}"))
    }
}

/// `key`'s default value, if it has one.
fn default_value(key: &str) -> Option<String> {
    let key_wide = wide(key);
    let mut buffer = [0u16; 256];
    let mut bytes = (buffer.len() * 2) as u32;
    // SAFETY: the key is null-terminated and the buffer's size is given in
    // bytes.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key_wide.as_ptr(),
            std::ptr::null(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    let length = (bytes as usize / 2).saturating_sub(1).min(buffer.len());
    (status == ERROR_SUCCESS).then(|| String::from_utf16_lossy(&buffer[..length]))
}

fn delete_tree(key: &str) {
    let key_wide = wide(key);
    // SAFETY: the key is null-terminated. A missing key is not an error here.
    unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, key_wide.as_ptr()) };
}

fn changed() {
    // SAFETY: SHCNE_ASSOCCHANGED takes no items.
    unsafe {
        SHChangeNotify(
            SHCNE_ASSOCCHANGED as i32,
            SHCNF_IDLIST,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
}

/// Opens `.ugurugu` files with `app`, under `classes` in HKEY_CURRENT_USER
/// (`CLASSES` but in tests). `name` is what Explorer calls the file type.
pub fn register(app: &Path, name: &str, classes: &str) -> Result<(), String> {
    let app = app.display();
    let prog = format!(r"{classes}\{PROG_ID}");
    set(&prog, None, name)?;
    set(
        &format!(r"{prog}\DefaultIcon"),
        None,
        &format!("\"{app}\",0"),
    )?;
    set(
        &format!(r"{prog}\shell\open\command"),
        None,
        &format!("\"{app}\" \"%1\""),
    )?;
    let extension = format!(r"{classes}\{EXTENSION}");
    set(&extension, None, PROG_ID)?;
    set(&format!(r"{extension}\OpenWithProgids"), Some(PROG_ID), "")?;
    changed();
    Ok(())
}

/// Takes the association away again, leaving `.ugurugu` alone if something
/// else has taken it over since.
pub fn unregister(classes: &str) {
    let extension = format!(r"{classes}\{EXTENSION}");
    if default_value(&extension).as_deref() == Some(PROG_ID) {
        delete_tree(&extension);
    } else {
        delete_tree(&format!(r"{extension}\OpenWithProgids\{PROG_ID}"));
    }
    delete_tree(&format!(r"{classes}\{PROG_ID}"));
    changed();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registering_and_unregistering_leave_nothing_behind() {
        let root = format!(r"Software\UguruguTest-{}", std::process::id());
        let classes = format!(r"{root}\Classes");
        let app = Path::new(r"C:\Users\Someone\AppData\Local\Ugurugu3\current\ugurugu.exe");
        register(app, "Ugurugu document", &classes).unwrap();
        assert_eq!(
            default_value(&format!(r"{classes}\.ugurugu")).as_deref(),
            Some(PROG_ID)
        );
        assert_eq!(
            default_value(&format!(r"{classes}\{PROG_ID}\shell\open\command")).unwrap(),
            format!("\"{}\" \"%1\"", app.display())
        );
        unregister(&classes);
        assert_eq!(default_value(&format!(r"{classes}\.ugurugu")), None);
        assert_eq!(default_value(&format!(r"{classes}\{PROG_ID}")), None);

        // Another app's association stays; only ours goes from it.
        set(&format!(r"{classes}\.ugurugu"), None, "Other.App").unwrap();
        register(app, "Ugurugu document", &classes).unwrap();
        set(&format!(r"{classes}\.ugurugu"), None, "Other.App").unwrap();
        unregister(&classes);
        assert_eq!(
            default_value(&format!(r"{classes}\.ugurugu")).as_deref(),
            Some("Other.App")
        );
        delete_tree(&root);
        assert_eq!(default_value(&format!(r"{classes}\.ugurugu")), None);
    }
}
