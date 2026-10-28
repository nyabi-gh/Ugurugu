// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

use std::ffi::c_int;

use ugu_platform_types::memory::Memory;

/// `struct vm_statistics64` of `<mach/vm_statistics.h>`.
#[repr(C, align(8))]
#[derive(Default)]
struct VmStatistics64 {
    free_count: u32,
    active_count: u32,
    inactive_count: u32,
    wire_count: u32,
    zero_fill_count: u64,
    reactivations: u64,
    pageins: u64,
    pageouts: u64,
    faults: u64,
    cow_faults: u64,
    lookups: u64,
    hits: u64,
    purges: u64,
    purgeable_count: u32,
    speculative_count: u32,
    decompressions: u64,
    compressions: u64,
    swapins: u64,
    swapouts: u64,
    compressor_page_count: u32,
    throttled_count: u32,
    external_page_count: u32,
    internal_page_count: u32,
    total_uncompressed_pages_in_compressor: u64,
}

const HOST_VM_INFO64: c_int = 4;
const KERN_SUCCESS: c_int = 0;

unsafe extern "C" {
    static mach_task_self_: u32;
    static vm_kernel_page_size: usize;
    fn mach_host_self() -> u32;
    fn mach_port_deallocate(task: u32, name: u32) -> c_int;
    fn host_statistics64(
        host: u32,
        flavor: c_int,
        info: *mut VmStatistics64,
        count: *mut u32,
    ) -> c_int;
}

/// `None` when macOS cannot report it.
///
/// Available memory is the free pages (the speculative ones among them)
/// and the inactive ones, which macOS reclaims first; purgeable pages are
/// already counted as active or inactive. macOS
/// commits nothing ahead (it compresses memory and grows its swap files
/// instead of refusing an allocation), so there is no commit limit to
/// count against.
pub fn status() -> Option<Memory> {
    let total = crate::macos::sysctl_u64(c"hw.memsize")?;
    let mut statistics = VmStatistics64::default();
    let mut count = (size_of::<VmStatistics64>() / size_of::<u32>()) as u32;
    // SAFETY: the host port is released once read; the statistics and the
    // count, in 32-bit words, describe the same live value.
    let (status, page) = unsafe {
        let host = mach_host_self();
        let status = host_statistics64(host, HOST_VM_INFO64, &mut statistics, &mut count);
        mach_port_deallocate(mach_task_self_, host);
        (status, vm_kernel_page_size as u64)
    };
    if status != KERN_SUCCESS {
        return None;
    }
    let pages = u64::from(statistics.free_count) + u64::from(statistics.inactive_count);
    Some(Memory {
        total,
        available: (pages * page).min(total),
        commit_available: u64::MAX,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn memory_is_reported() {
        let memory = super::status().unwrap();
        assert!(memory.total > 0 && memory.available > 0 && memory.available <= memory.total);
    }
}
