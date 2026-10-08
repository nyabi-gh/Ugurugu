// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Memory for the canvas caches: the layers' own surfaces and the playback
//! frames rendered ahead. A share of the installed memory, so a larger PC
//! keeps more, and no more than half of what is free when it is worked out,
//! so other programs are not paged out and allocations are not refused.

use ugu_win::memory::Memory;

const MIB: u64 = 1024 * 1024;
/// The share of installed memory the caches may take.
const SHARE: u64 = 4;
/// What the caches get however short memory is: a 2048² document's layers
/// and a few frames. Beyond it, frames are rendered when they come up and
/// layers are put together one at a time.
const FLOOR: u64 = 256 * MIB;
/// When Windows does not report memory: the budget of a 3 GiB PC.
const UNKNOWN: u64 = 768 * MIB;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    /// The surfaces and the frames together.
    pub render: usize,
    /// The surfaces alone.
    pub surfaces: usize,
}

impl Budget {
    /// `held` is what the caches hold now, which they would give back.
    pub fn new(memory: Option<Memory>, held: u64) -> Self {
        let render = match memory {
            Some(memory) => {
                let free = memory.available.min(memory.commit_available) + held;
                (memory.total / SHARE).min(free / 2).max(FLOOR)
            }
            None => UNKNOWN,
        };
        let render = usize::try_from(render).unwrap_or(usize::MAX);
        Self {
            render,
            surfaces: render / 2,
        }
    }

    /// The budget for memory as it is now.
    pub fn now(held: u64) -> Self {
        Self::new(ugu_win::memory::status(), held)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * MIB;

    fn memory(total: u64, available: u64, commit_available: u64) -> Option<Memory> {
        Some(Memory {
            total,
            available,
            commit_available,
        })
    }

    #[test]
    fn a_quarter_of_installed_memory_when_enough_is_free() {
        assert_eq!(
            Budget::new(memory(16 * GIB, 10 * GIB, 20 * GIB), 0).render as u64,
            4 * GIB
        );
        let budget = Budget::new(memory(32 * GIB, 20 * GIB, 30 * GIB), 0);
        assert_eq!(budget.render as u64, 8 * GIB);
        assert_eq!(budget.surfaces as u64, 4 * GIB);
    }

    #[test]
    fn half_of_what_is_free_counting_what_the_caches_hold() {
        // Other programs use most of a 16 GiB PC.
        assert_eq!(
            Budget::new(memory(16 * GIB, 3 * GIB, 9 * GIB), 0).render as u64,
            1536 * MIB
        );
        assert_eq!(
            Budget::new(memory(16 * GIB, 3 * GIB, 9 * GIB), GIB).render as u64,
            2 * GIB
        );
        // Little left to commit: a small page file.
        assert_eq!(
            Budget::new(memory(32 * GIB, 14 * GIB, 2 * GIB), 0).render as u64,
            GIB
        );
    }

    #[test]
    fn a_floor_when_almost_nothing_is_free_and_a_default_when_unknown() {
        assert_eq!(
            Budget::new(memory(8 * GIB, 100 * MIB, GIB), 0).render as u64,
            FLOOR
        );
        assert_eq!(Budget::new(None, 0).render as u64, UNKNOWN);
    }
}
