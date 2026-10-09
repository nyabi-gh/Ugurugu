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
    /// folder Windows remembers.
    Save {
        file_type: FileType,
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

unsafe fn set_file_type(picker: &IFileDialog, file_type: FileType) -> windows::core::Result<()> {
    let name = HSTRING::from(file_type.name);
    let pattern = file_type
        .extensions
        .iter()
        .map(|extension| format!("*.{extension}"))
        .collect::<Vec<_>>()
        .join(";");
    let pattern = HSTRING::from(pattern);
    // SAFETY: COM is set up on this thread; the strings outlive the calls.
    unsafe {
        picker.SetFileTypes(&[COMDLG_FILTERSPEC {
            pszName: PCWSTR(name.as_ptr()),
            pszSpec: PCWSTR(pattern.as_ptr()),
        }])?;
        picker.SetDefaultExtension(&HSTRING::from(file_type.extensions[0]))
    }
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
            Dialog::Open(file_type) => set_file_type(&picker, *file_type)?,
            Dialog::Save {
                file_type,
                name,
                folder,
            } => {
                set_file_type(&picker, *file_type)?;
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
        Ok(Some(PathBuf::from(
            path.map_err(|_| windows::core::Error::from(E_FAIL))?,
        )))
    }
}
