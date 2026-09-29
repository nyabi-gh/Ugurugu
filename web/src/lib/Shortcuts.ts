// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import type { ToolId } from "./tools";

export interface ShortcutActions {
    undo: () => void;
    redo: () => void;
    save: () => void;
    open: () => void;
    newDocument: () => void;
    selectTool: (tool: ToolId) => void;
    adjustBrushSize: (delta: number) => void;
    zoomBy: (factor: number) => void;
    zoomToFit: () => void;
    zoomToActualSize: () => void;
    rotateBy: (delta: number) => void;
    resetRotation: () => void;
    stepFrame: (delta: number) => void;
    togglePlayback: () => void;
    selectAll: () => void;
    toggleSelectionMove: () => void;
    invertSelection: () => void;
    deselect: () => void;
    fillSelection: () => void;
    deleteSelection: () => void;
    copySelection: () => void;
    cutSelection: () => void;
    paste: () => void;
}

// Tool letters match the desktop rail.
const toolKeys: Record<string, ToolId> = {
    b: "brush",
    t: "text",
    e: "eraser",
    l: "lasso",
    w: "wand",
    g: "bucket",
    i: "eyedropper",
};

// A slider or a checkbox holds no text to type into, so leaving one focused
// must not swallow the shortcuts: moving the wobble slider and pressing
// Ctrl+Z has to undo the change it just made.
function isTextEntry(target: HTMLElement): boolean {
    if (target.isContentEditable) {
        return true;
    }
    if (target instanceof HTMLInputElement) {
        return !["range", "checkbox", "radio", "button"].includes(target.type);
    }
    return ["TEXTAREA", "SELECT"].includes(target.tagName);
}

// Browser- and OS-owned chords (Cmd/Ctrl+W, +L, +T, reload, native zoom) are
// deliberately left alone: inside the itch.io iframe the app cannot assume it
// owns them, so every shortcut here also has a visible control.
export function handleShortcut(
    event: KeyboardEvent,
    actions: ShortcutActions,
): boolean {
    const target = event.target as HTMLElement | null;
    if (target && isTextEntry(target)) {
        return false;
    }
    const accelerator = event.metaKey || event.ctrlKey;
    const key = event.key.toLowerCase();

    if (accelerator) {
        switch (key) {
            case "z":
                if (event.shiftKey) {
                    actions.redo();
                } else {
                    actions.undo();
                }
                return true;
            case "y":
                actions.redo();
                return true;
            case "s":
                actions.save();
                return true;
            case "o":
                actions.open();
                return true;
            case "n":
                actions.newDocument();
                return true;
            case "a":
                actions.selectAll();
                return true;
            case "d":
                actions.deselect();
                return true;
            case "c":
                actions.copySelection();
                return true;
            case "x":
                actions.cutSelection();
                return true;
            case "v":
                actions.paste();
                return true;
            case "i":
                if (event.shiftKey) {
                    actions.invertSelection();
                    return true;
                }
                return false;
            case "0":
                actions.zoomToFit();
                return true;
            case "1":
                actions.zoomToActualSize();
                return true;
            case "=":
            case "+":
                actions.zoomBy(1.25);
                return true;
            case "-":
                actions.zoomBy(1 / 1.25);
                return true;
            default:
                return false;
        }
    }

    // Alt+Delete fills the selection the way the desktop's Fill action does;
    // plain Delete removes what is inside it.
    if (key === "delete" || key === "backspace") {
        if (event.altKey) {
            actions.fillSelection();
        } else {
            actions.deleteSelection();
        }
        return true;
    }

    if (event.altKey) {
        return false;
    }

    if (key === "escape") {
        actions.deselect();
        return true;
    }

    const tool = toolKeys[key];
    if (tool) {
        actions.selectTool(tool);
        return true;
    }

    switch (key) {
        // Move mode is a mode rather than a tool, so it sits outside the tool
        // letters even though it answers to the same kind of key.
        case "m":
            actions.toggleSelectionMove();
            return true;
        case "[":
            actions.adjustBrushSize(-1);
            return true;
        case "]":
            actions.adjustBrushSize(1);
            return true;
        case "-":
            actions.rotateBy(-5);
            return true;
        case "^":
            actions.rotateBy(5);
            return true;
        case "6":
            if (event.shiftKey && event.code === "Digit6") {
                actions.rotateBy(5);
                return true;
            }
            return false;
        case "arrowleft":
            actions.stepFrame(-1);
            return true;
        case "arrowright":
            actions.stepFrame(1);
            return true;
        case "enter":
            actions.togglePlayback();
            return true;
        default:
            return false;
    }
}
