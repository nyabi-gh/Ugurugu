// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Puts the icon and the version into the executable, as 2.2.13's
//! `resources/icons/Ugurugu.rc.in`. The icon is resource 1, which the window
//! and the file association use.

use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("set by Cargo"));
    let icon = manifest.join("../../resources/icons/Ugurugu.ico");
    let icon = icon.canonicalize().expect("the icon is in resources/icons");
    println!("cargo:rerun-if-changed={}", icon.display());
    let version = std::env::var("CARGO_PKG_VERSION").expect("set by Cargo");
    let numbers: Vec<u16> = version
        .split(['.', '-'])
        .take(3)
        .map(|part| part.parse().unwrap_or(0))
        .collect();
    let [major, minor, patch] = [0, 1, 2].map(|index| numbers.get(index).copied().unwrap_or(0));
    // The resource compiler takes no `\\?\` prefix, and forward slashes need
    // no escaping in its strings.
    let icon = icon
        .display()
        .to_string()
        .trim_start_matches(r"\\?\")
        .replace('\\', "/");
    let script = format!(
        r#"1 ICON "{icon}"

1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName", "Nyabi (nyabi-gh)"
            VALUE "FileDescription", "Ugurugu"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "ugurugu"
            VALUE "LegalCopyright", "Copyright (C) 2026 Nyabi (nyabi-gh). Licensed under GPL-3.0-or-later."
            VALUE "OriginalFilename", "ugurugu.exe"
            VALUE "ProductName", "Ugurugu"
            VALUE "ProductVersion", "{version}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#
    );
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("set by Cargo")).join("ugurugu.rc");
    std::fs::write(&out, script).expect("OUT_DIR is writable");
    embed_resource::compile(&out, embed_resource::NONE)
        .manifest_required()
        .expect("the resource compiler runs");
}
