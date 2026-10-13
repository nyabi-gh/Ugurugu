// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Hears that Windows is ending the session: signing out, restarting or
//! shutting down. The process may be ended as soon as `WM_ENDSESSION` is
//! answered, without the window being closed, so what must be kept is kept
//! before answering.

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{WM_ENDSESSION, WM_NCDESTROY};

const SUBCLASS_ID: usize = 0x5547_4F45;

/// Calls back as the session ends, until dropped.
pub struct SessionEnd {
    hwnd: HWND,
    /// Read by the subclass until `Drop` removes it.
    _ending: Box<Ending>,
}

type Ending = std::cell::RefCell<Box<dyn FnMut()>>;

impl SessionEnd {
    /// `ending` is called on the window's thread when the session ends,
    /// and the process may end once it returns.
    ///
    /// # Safety
    /// `hwnd` must be a live window owned by the calling thread, which must
    /// also be the thread that drops the returned value.
    pub unsafe fn install(
        hwnd: isize,
        ending: impl FnMut() + 'static,
    ) -> Result<Self, &'static str> {
        let hwnd = hwnd as HWND;
        let ending: Box<Ending> = Box::new(std::cell::RefCell::new(Box::new(ending)));
        let data = &*ending as *const Ending as usize;
        // SAFETY: `ending` is boxed and outlives the subclass, which `Drop` removes.
        if unsafe { SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, data) } == 0 {
            return Err("SetWindowSubclass failed");
        }
        Ok(Self {
            hwnd,
            _ending: ending,
        })
    }
}

impl Drop for SessionEnd {
    fn drop(&mut self) {
        // SAFETY: removing a subclass from a destroyed window fails harmlessly.
        unsafe { RemoveWindowSubclass(self.hwnd, Some(subclass_proc), SUBCLASS_ID) };
    }
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    match message {
        // `wparam` is zero when the session goes on after all.
        WM_ENDSESSION if wparam != 0 => {
            // SAFETY: `data` is the `Ending` installed with this subclass.
            let ending = unsafe { &*(data as *const Ending) };
            if let Ok(mut ending) = ending.try_borrow_mut() {
                ending();
            }
        }
        WM_NCDESTROY => {
            // SAFETY: removal during WM_NCDESTROY is the documented clean-up.
            unsafe { RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID) };
        }
        _ => {}
    }
    // SAFETY: forwards the original arguments down the subclass chain.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}
