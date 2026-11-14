// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 NewfolderGames

//! User interface language preferences and Linux distribution/kernel version.

use std::ffi::CStr;

/// Returns the user's preferred UI language tags (such as `"ko-KR"` or
/// `"zh-CN"`) from POSIX locale environment variables (`LC_ALL`,
/// `LC_MESSAGES`, `LANG`, and `LANGUAGE`).
pub fn preferred_ui_languages() -> Vec<String> {
    preferred_from_env(
        std::env::var("LC_ALL").ok().as_deref(),
        std::env::var("LC_MESSAGES").ok().as_deref(),
        std::env::var("LANG").ok().as_deref(),
        std::env::var("LANGUAGE").ok().as_deref(),
    )
}

fn preferred_from_env(
    lc_all: Option<&str>,
    lc_messages: Option<&str>,
    lang: Option<&str>,
    language: Option<&str>,
) -> Vec<String> {
    let primary_raw = [lc_all, lc_messages, lang]
        .into_iter()
        .flatten()
        .find(|value| !value.trim().is_empty());

    let Some(primary_raw) = primary_raw else {
        return Vec::new();
    };
    let Some(primary) = normalize_locale_tag(primary_raw) else {
        // GNU gettext ignores `LANGUAGE` when the active locale is `C` or `POSIX`.
        return Vec::new();
    };

    let mut out = Vec::new();
    if let Some(list) = language {
        for entry in list.split(':') {
            if let Some(tag) = normalize_locale_tag(entry)
                && !out.contains(&tag)
            {
                out.push(tag);
            }
        }
    }
    if !out.contains(&primary) {
        out.push(primary);
    }
    out
}

fn normalize_locale_tag(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Strip `@modifier` first, then `.encoding` (handles both `sr_RS.UTF-8@latin`
    // and `de_DE@euro`).
    let without_mod = trimmed.split('@').next().unwrap_or(trimmed);
    let base = without_mod.split('.').next().unwrap_or(without_mod).trim();
    if base.is_empty() || base.eq_ignore_ascii_case("c") || base.eq_ignore_ascii_case("posix") {
        return None;
    }
    Some(base.replace('_', "-"))
}

/// Returns the distribution name and kernel release, such as
/// `Ubuntu 24.04.2 LTS (Linux 6.8.0-55-generic)`.
pub fn os_version() -> String {
    let kernel = kernel_release();
    let os_release = std::fs::read_to_string("/etc/os-release")
        .or_else(|_| std::fs::read_to_string("/usr/lib/os-release"))
        .ok();
    format_os_version(os_release.as_deref(), kernel.as_deref())
}

fn kernel_release() -> Option<String> {
    // SAFETY: all-zero is a valid `utsname`, and `uname` writes NUL-terminated
    // fields into it.
    let mut uts: libc::utsname = unsafe { std::mem::zeroed() };
    if unsafe { libc::uname(&mut uts) } != 0 {
        return None;
    }
    // SAFETY: `uname` guarantees `release` is NUL-terminated within the array.
    let release = unsafe { CStr::from_ptr(uts.release.as_ptr()) };
    let text = release.to_string_lossy().trim().to_owned();
    (!text.is_empty()).then_some(text)
}

fn format_os_version(os_release: Option<&str>, kernel: Option<&str>) -> String {
    let distro = os_release.and_then(parse_os_release);
    match (distro, kernel) {
        (Some(distro), Some(kernel)) => format!("{distro} (Linux {kernel})"),
        (Some(distro), None) => distro,
        (None, Some(kernel)) => format!("Linux {kernel}"),
        (None, None) => "Linux".to_owned(),
    }
}

fn parse_os_release(text: &str) -> Option<String> {
    let mut pretty = None;
    let mut name = None;
    let mut version = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, raw_val)) = line.split_once('=') else {
            continue;
        };
        let val = unquote_os_release(raw_val.trim());
        if val.is_empty() {
            continue;
        }
        match key.trim() {
            "PRETTY_NAME" => pretty = Some(val),
            "NAME" => name = Some(val),
            "VERSION_ID" => version = Some(val),
            _ => {}
        }
    }
    if let Some(pretty) = pretty {
        return Some(pretty);
    }
    match (name, version) {
        (Some(name), Some(version)) => Some(format!("{name} {version}")),
        (Some(name), None) => Some(name),
        _ => None,
    }
}

fn unquote_os_release(raw: &str) -> String {
    let inner = raw
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| raw.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
        .unwrap_or(raw);
    inner.replace(r#"\""#, "\"").replace(r"\\", "\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_locales_normalize_to_bcp47_and_respect_precedence() {
        assert_eq!(
            preferred_from_env(None, None, Some("ko_KR.UTF-8"), None),
            vec!["ko-KR"]
        );
        assert_eq!(
            preferred_from_env(Some("ja_JP.utf8"), Some("ko_KR.UTF-8"), Some("en_US"), None),
            vec!["ja-JP"]
        );
        assert_eq!(
            preferred_from_env(
                None,
                None,
                Some("en_US.UTF-8"),
                Some("zh_CN:ko_KR.UTF-8:C:en_US")
            ),
            vec!["zh-CN", "ko-KR", "en-US"]
        );
        // `C` and `POSIX` disable `LANGUAGE` overrides per gettext rules.
        assert!(preferred_from_env(Some("C.UTF-8"), None, None, Some("ko_KR:ja_JP")).is_empty());
        assert!(preferred_from_env(None, None, Some("POSIX"), Some("ko")).is_empty());
    }

    #[test]
    fn os_version_reports_distro_and_kernel() {
        let live = os_version();
        assert!(live.contains("Linux"), "{live}");
        assert!(!live.contains('?'), "{live}");

        let parsed = format_os_version(
            Some("NAME=\"Fedora Linux\"\nPRETTY_NAME=\"Fedora Linux 41 (Workstation)\"\n"),
            Some("6.12.5-200.fc41.x86_64"),
        );
        assert_eq!(
            parsed,
            "Fedora Linux 41 (Workstation) (Linux 6.12.5-200.fc41.x86_64)"
        );
    }
}
