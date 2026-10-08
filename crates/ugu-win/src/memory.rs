// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

/// Memory as Windows reports it now, in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Memory {
    /// Installed physical memory.
    pub total: u64,
    /// Physical memory that can be taken without paging anything out.
    pub available: u64,
    /// What can still be committed before allocations fail.
    pub commit_available: u64,
}

/// `None` when Windows cannot report it.
pub fn status() -> Option<Memory> {
    // SAFETY: all-zero is a valid MEMORYSTATUSEX.
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: the pointer is valid and `dwLength` is set as the call requires.
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return None;
    }
    Some(Memory {
        total: status.ullTotalPhys,
        available: status.ullAvailPhys,
        commit_available: status.ullAvailPageFile,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn memory_is_reported() {
        let memory = super::status().unwrap();
        assert!(memory.total > 0 && memory.available <= memory.total);
    }
}
