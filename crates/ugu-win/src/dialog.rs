// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The Windows file dialogs. Each call blocks until the dialog closes and
//! sets up COM on the calling thread, so it belongs on a thread of its own
//! that does nothing else.

use std::path::{Path, PathBuf};

use ugu_platform_types::dialog::{Dialog, FileType, with_extension_of};
use windows::Win32::Foundation::{E_FAIL, ERROR_CANCELLED, HWND};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
    CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS,
    FOS_STRICTFILETYPES, FileOpenDialog, FileSaveDialog, IFileDialog, IShellItem,
    SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
};
use windows::core::{HRESULT, HSTRING, PCWSTR};

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

/// Shows the dialog until it gives the path to use. A save dialog asks
/// about replacing the file it returns; when the chosen type's extension is
/// added to that and names a file there already, it is shown again with
/// that name, to ask about that file.
unsafe fn show(owner: isize, dialog: &Dialog) -> windows::core::Result<Option<PathBuf>> {
    let mut dialog = dialog.clone();
    let mut chosen = None;
    loop {
        // SAFETY: as `show_once`.
        let Some((returned, file_type)) = (unsafe { show_once(owner, &dialog, chosen)? }) else {
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
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            *folder = path.parent().map(Path::to_path_buf);
        }
        chosen = Some(index);
    }
}

/// Shows the dialog once, starting a save dialog at file type `chosen`
/// (one-based) if given, and returns the path and, for a save dialog, the
/// file type chosen.
#[expect(clippy::type_complexity, reason = "private, read where it is made")]
unsafe fn show_once(
    owner: isize,
    dialog: &Dialog,
    chosen: Option<u32>,
) -> windows::core::Result<Option<(PathBuf, Option<(u32, FileType)>)>> {
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
                if let Some(chosen) = chosen {
                    picker.SetFileTypeIndex(chosen)?;
                }
                // The name gets an extension of the types in the dialog,
                // so the file it asks about replacing is the one written.
                options |= FOS_OVERWRITEPROMPT | FOS_STRICTFILETYPES;
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
            let chosen = picker.GetFileTypeIndex()?;
            if let Some(file_type) = file_types.get((chosen as usize).wrapping_sub(1)) {
                return Ok(Some((path, Some((chosen, *file_type)))));
            }
        }
        Ok(Some((path, None)))
    }
}
