// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Where the window was, to open it there again: its restored rectangle in
//! workspace coordinates, as Windows keeps it, and whether it was maximized.

/// The smallest restored window kept, each way, in pixels.
const SMALLEST: i32 = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    /// Left, top, right, bottom.
    pub normal: [i32; 4],
    pub maximized: bool,
}

impl Placement {
    /// Whether the restored rectangle is one a window can have.
    pub fn valid(&self) -> bool {
        let [left, top, right, bottom] = self.normal;
        let edge = |from: i32, to: i32| to.checked_sub(from).is_some_and(|edge| edge >= SMALLEST);
        edge(left, right) && edge(top, bottom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_rectangle_a_window_can_have_is_valid() {
        let at = |normal| Placement {
            normal,
            maximized: false,
        };
        assert!(at([0, 0, 1600, 1000]).valid());
        assert!(at([-3000, -200, -1400, 800]).valid());
        assert!(!at([0, 0, 199, 1000]).valid());
        assert!(!at([100, 100, 0, 0]).valid());
        assert!(!at([i32::MIN, 0, i32::MAX, 1000]).valid());
    }
}
