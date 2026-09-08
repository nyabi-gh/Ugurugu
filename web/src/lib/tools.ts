// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyattic)

export type ToolId =
    | "text"
    | "brush"
    | "eraser"
    | "lasso"
    | "wand"
    | "bucket"
    | "eyedropper";

export interface ToolDefinition {
    id: ToolId;
    label: string;
    shortcut: string;
    hint: string;
}

const brushTool: ToolDefinition = {
    id: "brush",
    label: "Brush",
    shortcut: "B",
    hint: "Draw a wobbling line",
};

// Shortcuts match the desktop tool rail so muscle memory carries over.
export const tools: ToolDefinition[] = [
    brushTool,
    { id: "text", label: "Text", shortcut: "T", hint: "Place wobbling text" },
    {
        id: "eraser",
        label: "Eraser",
        shortcut: "E",
        hint: "Rub out what you drew",
    },
    {
        id: "lasso",
        label: "Lasso",
        shortcut: "L",
        hint: "Draw around an area to select it",
    },
    {
        id: "wand",
        label: "Wand",
        shortcut: "W",
        hint: "Select an area enclosed by lines",
    },
    {
        id: "bucket",
        label: "Bucket",
        shortcut: "G",
        hint: "Flood an area with the current color",
    },
    {
        id: "eyedropper",
        label: "Eyedropper",
        shortcut: "I",
        hint: "Take a color from the canvas",
    },
];

// Values are the enumerators the engine bridge expects; changing one changes
// the wire format.
export const selectionShapes = {
    freehand: 0,
    rectangle: 1,
    ellipse: 2,
} as const;

export const selectionCombines = {
    replace: 0,
    add: 1,
    subtract: 2,
} as const;

export const fillReferences = {
    activeLayer: 0,
    markedLayers: 1,
    allVisibleLayers: 2,
} as const;

export const fillComparisons = {
    alphaBoundary: 0,
    colorTolerance: 1,
} as const;

export type SelectionShapeName = keyof typeof selectionShapes;
export type LassoMode = "select" | "paint";
export type CombineValue =
    (typeof selectionCombines)[keyof typeof selectionCombines];

export function toolDefinition(id: ToolId): ToolDefinition {
    return tools.find((entry) => entry.id === id) ?? brushTool;
}

// Shift adds to the selection, Alt subtracts, and asking for both is treated
// as asking for neither — the same reading CanvasWidgetEvents applies.
export function combineForModifiers(event: {
    shiftKey: boolean;
    altKey: boolean;
}): CombineValue {
    if (event.shiftKey === event.altKey) {
        return selectionCombines.replace;
    }
    return event.shiftKey
        ? selectionCombines.add
        : selectionCombines.subtract;
}
