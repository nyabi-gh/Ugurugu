// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Where the window was, to open it there again. Windows keeps a window's
//! restored rectangle itself. Elsewhere it comes from winit: the outer
//! position and the inner size, since winit sets a size only from inside,
//! and AppKit lets only the main thread read a window, which the render
//! thread that saves this is not.

use ugu_platform::placement::Placement;
use winit::window::Window;

/// Where `window` (whose handle is `hwnd`) is. Outside Windows a maximized
/// window gives no restored rectangle, so it keeps the one in `before`.
#[cfg(windows)]
pub fn get(_window: &Window, hwnd: isize, _before: Option<Placement>) -> Option<Placement> {
    ugu_platform::placement::get(hwnd)
}

#[cfg(not(windows))]
pub fn get(window: &Window, _hwnd: isize, before: Option<Placement>) -> Option<Placement> {
    let maximized = window.is_maximized();
    if maximized && let Some(before) = before {
        return Some(Placement {
            maximized,
            ..before
        });
    }
    let position = window.outer_position().ok()?;
    let size = window.inner_size();
    let [width, height] = [size.width, size.height].map(|edge| i32::try_from(edge).ok());
    Some(Placement {
        normal: [
            position.x,
            position.y,
            position.x.checked_add(width?)?,
            position.y.checked_add(height?)?,
        ],
        maximized,
    })
}

/// Puts the hidden `window` where `placement` says, maximized or not.
/// Outside Windows, a window whose title bar would be on no screen keeps
/// the position winit gave it and takes only the size.
pub fn restore(window: &Window, placement: &Placement) {
    if restore_rectangle(window, placement) {
        window.set_maximized(placement.maximized);
    }
}

#[cfg(windows)]
fn restore_rectangle(window: &Window, placement: &Placement) -> bool {
    let Ok(hwnd) = crate::app::hwnd_of(window) else {
        return false;
    };
    // SAFETY: the window is alive and owned by this thread.
    unsafe { ugu_platform::placement::restore_to(hwnd, placement) }
}

#[cfg(not(windows))]
fn restore_rectangle(window: &Window, placement: &Placement) -> bool {
    use winit::dpi::{PhysicalPosition, PhysicalSize};

    let [left, top, right, bottom] = placement.normal;
    let title = (left + (right - left) / 2, top);
    let on_screen = window.available_monitors().any(|monitor| {
        let (at, size) = (monitor.position(), monitor.size());
        let inside = |point: i32, from: i32, edge: u32| {
            point >= from && i64::from(point) < i64::from(from) + i64::from(edge)
        };
        inside(title.0, at.x, size.width) && inside(title.1, at.y, size.height)
    });
    if on_screen {
        window.set_outer_position(PhysicalPosition::new(left, top));
    }
    let _ = window.request_inner_size(PhysicalSize::new(
        (right - left).unsigned_abs(),
        (bottom - top).unsigned_abs(),
    ));
    true
}
