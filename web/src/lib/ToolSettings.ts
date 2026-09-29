// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import type { LassoMode, SelectionShapeName } from "./tools";

// Everything the tool options column edits, in one object so the shell owns a
// single source of truth and the panel can mutate it directly.
// The desktop's brush popover slider runs to 128 and every built-in preset
// default fits inside that; the web used to stop at 64, so picking a preset
// above it left the slider pinned and the value unreachable again.
export const minimumBrushSize = 1;
export const maximumBrushSize = 128;

export interface ToolSettings {
    presetIndex: number;
    eraserPresetIndex: number;
    // Kept apart the way the desktop keeps m_brushWidth and m_eraserWidth, so
    // reaching for the eraser does not resize the brush.
    brushSize: number;
    eraserSize: number;
    stabilization: number;
    brushAntialiasing: boolean;
    lassoMode: LassoMode;
    selectionShape: SelectionShapeName;
    fillReference: number;
    fillComparison: number;
    fillTolerance: number;
    bucketAntialiasing: boolean;
}

const storageKey = "ugurugu-web-tool-settings";

// Defaults match the desktop: antialiased fills, aliased brushes, alpha
// boundary flooding with a 32 tolerance ready for the color comparison.
export function defaultToolSettings(): ToolSettings {
    return {
        presetIndex: 0,
        eraserPresetIndex: 0,
        brushSize: 6,
        eraserSize: 6,
        stabilization: 0,
        brushAntialiasing: false,
        lassoMode: "select",
        selectionShape: "freehand",
        fillReference: 0,
        fillComparison: 0,
        fillTolerance: 32,
        bucketAntialiasing: true,
    };
}

export function loadToolSettings(): ToolSettings {
    const settings = defaultToolSettings();
    try {
        const stored = window.localStorage.getItem(storageKey);
        if (!stored) {
            return settings;
        }
        const parsed: unknown = JSON.parse(stored);
        if (!parsed || typeof parsed !== "object") {
            return settings;
        }
        // Field by field, so a stored file written by an older or newer shell
        // contributes what it can instead of being taken or dropped whole.
        const incoming = parsed as Record<string, unknown>;
        const target = settings as unknown as Record<string, unknown>;
        for (const [key, fallback] of Object.entries(target)) {
            const value = incoming[key];
            if (typeof value === typeof fallback) {
                target[key] = value;
            }
        }
    } catch {
        return defaultToolSettings();
    }
    return settings;
}

export function saveToolSettings(settings: ToolSettings) {
    try {
        window.localStorage.setItem(storageKey, JSON.stringify(settings));
    } catch {
        // Preferences that cannot be stored still apply for this session.
    }
}
