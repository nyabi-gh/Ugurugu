// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The Windows file dialogs. Each call blocks until the dialog closes and
//! sets up COM on the calling thread, so it belongs on a thread of its own
//! that does nothing else.

use std::path::PathBuf;

use windows::Win32::Foundation::{E_FAIL, ERROR_CANCELLED, HWND};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
    CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, FileOpenDialog, FileSaveDialog,
    IFileDialog, SIGDN_FILESYSPATH,
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

pub enum Dialog<'a> {
    Open,
    /// Suggests `name` for the file.
    Save {
        name: &'a str,
    },
}

/// Shows the dialog over `owner` (a window handle) and returns the chosen
/// path, or `None` when cancelled.
pub fn pick(
    owner: isize,
    dialog: Dialog<'_>,
    file_type: FileType,
) -> windows::core::Result<Option<PathBuf>> {
    // SAFETY: the thread is set up for COM once here and torn down below;
    // every COM object made in between is released before that.
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).ok()?;
        let result = show(owner, dialog, file_type);
        CoUninitialize();
        result
    }
}

unsafe fn show(
    owner: isize,
    dialog: Dialog<'_>,
    file_type: FileType,
) -> windows::core::Result<Option<PathBuf>> {
    // SAFETY: COM is set up on this thread; the strings outlive the calls
    // that read them.
    unsafe {
        let class = match dialog {
            Dialog::Open => &FileOpenDialog,
            Dialog::Save { .. } => &FileSaveDialog,
        };
        let picker: IFileDialog = CoCreateInstance(class, None, CLSCTX_INPROC_SERVER)?;
        let name = HSTRING::from(file_type.name);
        let pattern = file_type
            .extensions
            .iter()
            .map(|extension| format!("*.{extension}"))
            .collect::<Vec<_>>()
            .join(";");
        let pattern = HSTRING::from(pattern);
        picker.SetFileTypes(&[COMDLG_FILTERSPEC {
            pszName: PCWSTR(name.as_ptr()),
            pszSpec: PCWSTR(pattern.as_ptr()),
        }])?;
        let extension = HSTRING::from(file_type.extensions[0]);
        picker.SetDefaultExtension(&extension)?;
        let mut options = picker.GetOptions()? | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST;
        if let Dialog::Save { name } = dialog {
            options |= FOS_OVERWRITEPROMPT;
            picker.SetFileName(&HSTRING::from(name))?;
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
