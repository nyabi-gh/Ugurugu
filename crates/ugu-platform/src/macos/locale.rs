// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

use objc2_foundation::NSLocale;

/// The user's languages in order of preference, such as "ko-KR" or
/// "zh-Hans-CN", from System Settings.
pub fn preferred_ui_languages() -> Vec<String> {
    NSLocale::preferredLanguages()
        .iter()
        .map(|name| name.to_string())
        .collect()
}

/// The macOS version and build, such as `macOS 26.2 (25C56)`.
pub fn os_version() -> String {
    let version = crate::macos::sysctl_text(c"kern.osproductversion");
    let build = crate::macos::sysctl_text(c"kern.osversion");
    format!(
        "macOS {} ({})",
        version.as_deref().unwrap_or("?"),
        build.as_deref().unwrap_or("?")
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_version_names_a_release_and_a_build() {
        let version = super::os_version();
        assert!(
            version.starts_with("macOS 1") || version.starts_with("macOS 2"),
            "{version}"
        );
        assert!(!version.contains('?'), "{version}");
    }

    #[test]
    fn some_language_is_preferred() {
        assert!(!super::preferred_ui_languages().is_empty());
    }
}
