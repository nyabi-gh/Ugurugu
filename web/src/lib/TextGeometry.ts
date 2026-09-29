// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import type { Font, PathCommand } from "opentype.js";
import fontUrl from "../../../resources/fonts/PretendardJP-Medium.otf?url";

export interface TextDraft {
    text: string;
    size: number;
    width: number;
    filled: boolean;
    x: number;
    y: number;
}

export interface TextGeometry {
    commands: number[];
    path: Path2D;
}

let fontPromise: Promise<Font> | null = null;

function loadFont(): Promise<Font> {
    fontPromise ??= Promise.all([import("opentype.js"), fetch(fontUrl)])
        .then(async ([opentype, response]) => {
            if (!response.ok)
                throw new Error("The text font could not be loaded.");
            return opentype.parse(await response.arrayBuffer());
        })
        .catch((error: unknown) => {
            fontPromise = null;
            throw error;
        });
    return fontPromise;
}

export async function textGeometry(
    text: string,
    size: number,
): Promise<TextGeometry> {
    const normalized = text.normalize("NFC");
    if (!normalized.trim()) throw new Error("Type some text to place it.");
    if (normalized.length > 256)
        throw new Error("Use up to 256 characters at a time.");
    if (!Number.isFinite(size) || size < 8 || size > 512) {
        throw new Error("Choose a text size from 8 to 512 pixels.");
    }
    const font = await loadFont();
    for (const character of normalized) {
        if (character !== "\n" && !font.hasChar(character)) {
            throw new Error(`Pretendard JP does not contain “${character}”.`);
        }
    }
    const path = new Path2D();
    const commands: number[] = [];
    const pack = (c: PathCommand) => {
        switch (c.type) {
            case "M":
                path.moveTo(c.x, c.y);
                commands.push(0, c.x, c.y, 0, 0, 0, 0);
                break;
            case "L":
                path.lineTo(c.x, c.y);
                commands.push(1, c.x, c.y, 0, 0, 0, 0);
                break;
            case "C":
                path.bezierCurveTo(c.x1, c.y1, c.x2, c.y2, c.x, c.y);
                commands.push(2, c.x, c.y, c.x1, c.y1, c.x2, c.y2);
                break;
            case "Q":
                path.quadraticCurveTo(c.x1, c.y1, c.x, c.y);
                commands.push(3, c.x, c.y, c.x1, c.y1, 0, 0);
                break;
            case "Z":
                path.closePath();
                commands.push(4, 0, 0, 0, 0, 0, 0);
                break;
        }
    };
    const scale = size / font.unitsPerEm;
    let baseline = font.ascender * scale;
    for (const line of normalized.split("\n")) {
        for (const command of font.getPath(line, 0, baseline, size).commands) {
            pack(command);
            if (commands.length > 32768 * 7)
                throw new Error("Use fewer characters for this text.");
        }
        baseline += (font.ascender - font.descender) * scale;
    }
    if (!commands.length) throw new Error("The text has no drawable outlines.");
    return { commands, path };
}
