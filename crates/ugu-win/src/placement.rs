// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Where the window was, to open it there again: its restored rectangle in
//! workspace coordinates, as Windows keeps it, and whether it was maximized.

use ugu_platform_types::placement::Placement;
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowPlacement, SW_HIDE, SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED, SetWindowPlacement,
    WINDOWPLACEMENT, WPF_RESTORETOMAXIMIZED,
};

/// Where `hwnd` is; a minimized window gives what it restores to.
pub fn get(hwnd: isize) -> Option<Placement> {
    let mut placement = empty();
    // SAFETY: `placement` has the size it says; a window gone makes it fail.
    if unsafe { GetWindowPlacement(hwnd as HWND, &mut placement) } == 0 {
        return None;
    }
    let RECT {
        left,
        top,
        right,
        bottom,
    } = placement.rcNormalPosition;
    let maximized = match placement.showCmd as i32 {
        SW_SHOWMAXIMIZED => true,
        SW_SHOWMINIMIZED => placement.flags & WPF_RESTORETOMAXIMIZED != 0,
        _ => false,
    };
    Some(Placement {
        normal: [left, top, right, bottom],
        maximized,
    })
}

/// Gives the hidden window `hwnd` the restored rectangle of `placement`,
/// leaving it hidden; Windows moves a rectangle off every screen onto one.
/// Showing it, maximized or not, is left to the caller.
///
/// # Safety
/// `hwnd` must be a window owned by the calling thread.
pub unsafe fn restore_to(hwnd: isize, placement: &Placement) -> bool {
    let [left, top, right, bottom] = placement.normal;
    let mut to = empty();
    to.showCmd = SW_HIDE as u32;
    to.rcNormalPosition = RECT {
        left,
        top,
        right,
        bottom,
    };
    // SAFETY: the caller owns the window; `to` has the size it says.
    unsafe { SetWindowPlacement(hwnd as HWND, &to) != 0 }
}

fn empty() -> WINDOWPLACEMENT {
    WINDOWPLACEMENT {
        length: size_of::<WINDOWPLACEMENT>() as u32,
        flags: 0,
        showCmd: 0,
        ptMinPosition: POINT { x: -1, y: -1 },
        ptMaxPosition: POINT { x: -1, y: -1 },
        rcNormalPosition: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
    }
}
