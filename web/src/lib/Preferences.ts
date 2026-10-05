// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

// localStorage can throw from its getter (site data blocked), from getItem and
// from setItem (quota, private mode). Preferences only shape the next visit,
// so every access goes through here and a refusal means the default applies.

export function readPreference(key: string): string | null {
    try {
        return window.localStorage.getItem(key);
    } catch {
        return null;
    }
}

export function writePreference(key: string, value: string) {
    try {
        window.localStorage.setItem(key, value);
    } catch {
        // Still applies for this session.
    }
}
