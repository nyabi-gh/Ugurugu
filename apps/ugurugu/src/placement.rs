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
    let outer = window.outer_position().ok().map(|at| [at.x, at.y]);
    let size = window.inner_size();
    compose_placement(
        outer,
        [size.width, size.height],
        window.is_maximized(),
        before,
    )
}

#[cfg(any(not(windows), test))]
fn compose_placement(
    outer: Option<[i32; 2]>,
    inner: [u32; 2],
    maximized: bool,
    before: Option<Placement>,
) -> Option<Placement> {
    if maximized && let Some(before) = before {
        return Some(Placement {
            maximized,
            ..before
        });
    }
    // Wayland gives no outer position (`NotSupported`); keeping `before`'s
    // corner (or the origin) still saves the size and maximized state.
    let [x, y] = outer.unwrap_or_else(|| before.map_or([0, 0], |at| [at.normal[0], at.normal[1]]));
    let [width, height] = inner.map(|edge| i32::try_from(edge).ok());
    Some(Placement {
        normal: [x, y, x.checked_add(width?)?, y.checked_add(height?)?],
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
    if window.outer_position().is_ok() {
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
    }
    let _ = window.request_inner_size(PhysicalSize::new(
        (right - left).unsigned_abs(),
        (bottom - top).unsigned_abs(),
    ));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_an_outer_position_size_and_maximized_state_still_keep() {
        let first = compose_placement(None, [1440, 900], false, None).unwrap();
        assert_eq!(first.normal, [0, 0, 1440, 900]);
        assert!(!first.maximized);
        assert!(first.valid());

        let with_corner = Placement {
            normal: [240, 120, 1520, 920],
            maximized: false,
        };
        let resized = compose_placement(None, [1600, 1000], false, Some(with_corner)).unwrap();
        assert_eq!(resized.normal, [240, 120, 1840, 1120]);

        let maximized = compose_placement(None, [2560, 1400], true, Some(resized)).unwrap();
        assert_eq!(maximized.normal, resized.normal);
        assert!(maximized.maximized);
    }
}
