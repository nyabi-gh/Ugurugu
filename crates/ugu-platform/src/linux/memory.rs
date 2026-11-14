// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 NewfolderGames

//! System physical and commit memory status from `/proc/meminfo`.

use ugu_platform_types::memory::Memory;

/// Reports installed physical memory, reclaimable available memory, and
/// remaining commit headroom on Linux; returns `None` when `/proc/meminfo`
/// cannot be read.
pub fn status() -> Option<Memory> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    let strict_overcommit = std::fs::read_to_string("/proc/sys/vm/overcommit_memory")
        .ok()
        .is_some_and(|mode| mode.trim() == "2");
    parse_meminfo(&meminfo, strict_overcommit)
}

fn parse_meminfo(text: &str, strict_overcommit: bool) -> Option<Memory> {
    let mut total_kib = None;
    let mut available_kib = None;
    let mut free_kib = 0u64;
    let mut buffers_kib = 0u64;
    let mut cached_kib = 0u64;
    let mut commit_limit_kib = None;
    let mut committed_as_kib = None;

    for line in text.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let Some(kib) = rest
            .split_whitespace()
            .next()
            .and_then(|num| num.parse::<u64>().ok())
        else {
            continue;
        };
        match key {
            "MemTotal" => total_kib = Some(kib),
            "MemAvailable" => available_kib = Some(kib),
            "MemFree" => free_kib = kib,
            "Buffers" => buffers_kib = kib,
            "Cached" => cached_kib = kib,
            "CommitLimit" => commit_limit_kib = Some(kib),
            "Committed_AS" => committed_as_kib = Some(kib),
            _ => {}
        }
    }

    let total = total_kib?.saturating_mul(1024);
    if total == 0 {
        return None;
    }
    let available = available_kib
        .unwrap_or_else(|| {
            free_kib
                .saturating_add(buffers_kib)
                .saturating_add(cached_kib)
        })
        .saturating_mul(1024)
        .min(total);
    let commit_available = match (strict_overcommit, commit_limit_kib, committed_as_kib) {
        (true, Some(limit), Some(used)) => limit.saturating_sub(used).saturating_mul(1024),
        _ => u64::MAX,
    };

    Some(Memory {
        total,
        available,
        commit_available,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_is_reported() {
        let memory = status().unwrap();
        assert!(memory.total > 0 && memory.available > 0 && memory.available <= memory.total);
    }

    #[test]
    fn meminfo_parses_default_and_strict_overcommit_modes() {
        let sample = "\
MemTotal:       16384000 kB
MemFree:         2048000 kB
MemAvailable:   10240000 kB
Buffers:          512000 kB
Cached:          4096000 kB
CommitLimit:    12288000 kB
Committed_AS:    9216000 kB
";
        let heuristic = parse_meminfo(sample, false).unwrap();
        assert_eq!(heuristic.total, 16_384_000 * 1024);
        assert_eq!(heuristic.available, 10_240_000 * 1024);
        assert_eq!(heuristic.commit_available, u64::MAX);

        let strict = parse_meminfo(sample, true).unwrap();
        assert_eq!(strict.commit_available, 3_072_000 * 1024);
    }
}
