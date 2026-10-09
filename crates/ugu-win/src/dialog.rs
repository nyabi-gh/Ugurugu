// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The Windows file dialogs. Each call blocks until the dialog closes and
//! sets up COM on the calling thread, so it belongs on a thread of its own
//! that does nothing else.

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{E_FAIL, ERROR_CANCELLED, HWND};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
    CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog,
    FileSaveDialog, IFileDialog, IShellItem, SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
};
use windows::core::{HRESULT, HSTRING, PCWSTR};

/// A file type the dialog offers: a name such as "PNG image" and its
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
        file_types: &'static [FileType],
        name: String,
        folder: Option<PathBuf>,
    },
    /// Picks a folder, starting in `start` if given.
    Folder {
        title: String,
        start: Option<PathBuf>,
    },
}

/// Shows the dialog over `owner` (a window handle) and returns the chosen
/// path, or `None` when cancelled.
pub fn pick(owner: isize, dialog: &Dialog) -> windows::core::Result<Option<PathBuf>> {
    // SAFETY: the thread is set up for COM once here and torn down below;
    // every COM object made in between is released before that.
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).ok()?;
        let result = show(owner, dialog);
        CoUninitialize();
        result
    }
}

unsafe fn set_file_types(
    picker: &IFileDialog,
    file_types: &[FileType],
) -> windows::core::Result<()> {
    let strings: Vec<(HSTRING, HSTRING)> = file_types
        .iter()
        .map(|file_type| {
            let pattern = file_type
                .extensions
                .iter()
                .map(|extension| format!("*.{extension}"))
                .collect::<Vec<_>>()
                .join(";");
            (HSTRING::from(file_type.name), HSTRING::from(pattern))
        })
        .collect();
    let specs: Vec<COMDLG_FILTERSPEC> = strings
        .iter()
        .map(|(name, pattern)| COMDLG_FILTERSPEC {
            pszName: PCWSTR(name.as_ptr()),
            pszSpec: PCWSTR(pattern.as_ptr()),
        })
        .collect();
    // SAFETY: COM is set up on this thread; the strings outlive the calls.
    unsafe {
        picker.SetFileTypes(&specs)?;
        picker.SetDefaultExtension(&HSTRING::from(file_types[0].extensions[0]))
    }
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

/// Opens the dialog in `folder`. A folder that cannot be found is left to
/// Windows, which then starts where it last did.
unsafe fn set_folder(picker: &IFileDialog, folder: &Path) {
    // SAFETY: COM is set up on this thread; the string outlives the call.
    unsafe {
        match SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(folder), None) {
            Ok(item) => {
                if let Err(error) = picker.SetFolder(&item) {
                    tracing::warn!(%error, ?folder, "the dialog cannot start in the folder");
                }
            }
            Err(error) => tracing::debug!(%error, ?folder, "no folder to start the dialog in"),
        }
    }
}

unsafe fn show(owner: isize, dialog: &Dialog) -> windows::core::Result<Option<PathBuf>> {
    // SAFETY: COM is set up on this thread; the strings outlive the calls
    // that read them.
    unsafe {
        let class = match dialog {
            Dialog::Open(_) | Dialog::Folder { .. } => &FileOpenDialog,
            Dialog::Save { .. } => &FileSaveDialog,
        };
        let picker: IFileDialog = CoCreateInstance(class, None, CLSCTX_INPROC_SERVER)?;
        let mut options = picker.GetOptions()? | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST;
        match dialog {
            Dialog::Open(file_type) => set_file_types(&picker, &[*file_type])?,
            Dialog::Save {
                file_types,
                name,
                folder,
            } => {
                set_file_types(&picker, file_types)?;
                options |= FOS_OVERWRITEPROMPT;
                picker.SetFileName(&HSTRING::from(name.as_str()))?;
                if let Some(folder) = folder {
                    set_folder(&picker, folder);
                }
            }
            Dialog::Folder { title, start } => {
                options |= FOS_PICKFOLDERS;
                picker.SetTitle(&HSTRING::from(title.as_str()))?;
                if let Some(start) = start {
                    set_folder(&picker, start);
                }
            }
        }
        picker.SetOptions(options)?;
        match picker.Show(Some(HWND(owner as *mut _))) {
            Ok(()) => {}
            Err(error) if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        }
        let item = picker.GetResult()?;
        let text = item.GetDisplayName(SIGDN_FILESYSPATH)?;
        let path = text.to_string();
        CoTaskMemFree(Some(text.0 as *const _));
        let path = PathBuf::from(path.map_err(|_| windows::core::Error::from(E_FAIL))?);
        if let Dialog::Save { file_types, .. } = dialog {
            // One-based.
            let chosen = picker.GetFileTypeIndex()? as usize;
            if let Some(file_type) = file_types.get(chosen.wrapping_sub(1)) {
                return Ok(Some(with_extension_of(path, *file_type)));
            }
        }
        Ok(Some(path))
    }
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
