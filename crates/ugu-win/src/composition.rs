// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Window structure that decides how DWM composes a swap chain.
//!
//! DWM can show a swap chain on a hardware overlay plane (independent flip)
//! only when it does not have to compose the swap chain's area itself.

use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DwmSetWindowAttribute,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, HTTRANSPARENT, RegisterClassExW,
    SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos, WM_NCHITTEST, WNDCLASSEXW,
    WS_CHILD, WS_CLIPSIBLINGS, WS_DISABLED, WS_VISIBLE,
};

#[derive(Debug)]
pub struct CompositionError(&'static str);

impl std::fmt::Display for CompositionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for CompositionError {}

/// Turns off Windows 11 rounded corners, which DWM clips by composing.
pub fn set_square_corners(hwnd: isize) -> Result<(), CompositionError> {
    let preference = DWMWCP_DONOTROUND;
    // SAFETY: valid attribute pointer and size; a stale window fails harmlessly.
    let result = unsafe {
        DwmSetWindowAttribute(
            hwnd as HWND,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            (&raw const preference).cast(),
            size_of_val(&preference) as u32,
        )
    };
    if result < 0 {
        return Err(CompositionError("DwmSetWindowAttribute failed"));
    }
    Ok(())
}

/// A child window that holds its own swap chain inside a top-level window.
///
/// It never takes input: hit tests fall through to the parent, so pointer
/// input keeps arriving at the parent in parent client coordinates.
pub struct SurfaceChild {
    hwnd: HWND,
}

const CLASS_NAME: windows_sys::core::PCWSTR = windows_sys::core::w!("UguruguSurfaceChild");

unsafe extern "system" fn child_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if message == WM_NCHITTEST {
        return HTTRANSPARENT as LRESULT;
    }
    // SAFETY: forwards the arguments the system passed in.
    unsafe { DefWindowProcW(hwnd, message, w, l) }
}

fn register_class() -> Result<(), CompositionError> {
    static REGISTERED: OnceLock<bool> = OnceLock::new();
    let registered = *REGISTERED.get_or_init(|| {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(child_proc),
            // SAFETY: the module handle of this executable.
            hInstance: unsafe { GetModuleHandleW(std::ptr::null()) },
            lpszClassName: CLASS_NAME,
            // No background brush: the swap chain covers the whole window.
            ..unsafe { std::mem::zeroed() }
        };
        // SAFETY: the class structure is complete and its strings are static.
        unsafe { RegisterClassExW(&class) != 0 }
    });
    if registered {
        Ok(())
    } else {
        Err(CompositionError("RegisterClassExW failed"))
    }
}

impl SurfaceChild {
    /// # Safety
    /// `parent` must be a live window owned by the calling thread, which must
    /// also drop the returned value, after every swap chain of it is gone.
    pub unsafe fn create(parent: isize) -> Result<Self, CompositionError> {
        register_class()?;
        // SAFETY: registered class, live parent on this thread.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                CLASS_NAME,
                std::ptr::null(),
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_DISABLED,
                0,
                0,
                1,
                1,
                parent as HWND,
                std::ptr::null_mut(),
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            return Err(CompositionError("CreateWindowExW failed"));
        }
        Ok(Self { hwnd })
    }

    pub fn hwnd(&self) -> isize {
        self.hwnd as isize
    }
}

impl Drop for SurfaceChild {
    fn drop(&mut self) {
        // SAFETY: the window belongs to this thread; see `create`.
        unsafe { DestroyWindow(self.hwnd) };
    }
}

/// Places a `SurfaceChild` in parent client physical pixels. It may be called
/// from any thread and does not wait for the window's thread.
pub fn place_child(hwnd: isize, [left, top, right, bottom]: [i32; 4]) {
    // SAFETY: a stale window handle fails harmlessly.
    unsafe {
        SetWindowPos(
            hwnd as HWND,
            std::ptr::null_mut(),
            left,
            top,
            right - left,
            bottom - top,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
        )
    };
}
