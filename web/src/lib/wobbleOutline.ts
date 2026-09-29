// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

// The active tool is ringed by a hand-drawn outline that redraws itself a few
// times a second — the same trick the engine plays on every stroke, applied to
// the chrome. Offsets come from a hash of the sample index and the frame, so a
// given frame always looks the same and the ring never drifts.

const samples = 44;
const exponent = 4.2;

function jitter(index: number, frame: number, amplitude: number): number {
    let hash = Math.imul(index * 2654435761 + frame * 40503, 2246822519);
    hash ^= hash >>> 15;
    hash = Math.imul(hash, 2654435761);
    hash ^= hash >>> 13;
    return ((hash >>> 0) / 4294967295 - 0.5) * amplitude;
}

// A superellipse rather than a rounded rectangle: its corners stay soft under
// the offsets, where a rounded rect's straight runs would show the wobble as
// dents in a ruled line.
export function wobbleOutline(
    size: number,
    frame: number,
    amplitude = 1.5,
): string {
    const centre = size / 2;
    const radius = centre - amplitude - 1;
    const segments: string[] = [];
    for (let index = 0; index < samples; index += 1) {
        const angle = (index / samples) * Math.PI * 2;
        const cos = Math.cos(angle);
        const sin = Math.sin(angle);
        const shape = 2 / exponent;
        const x =
            centre +
            Math.sign(cos) * Math.abs(cos) ** shape * radius +
            jitter(index, frame, amplitude);
        const y =
            centre +
            Math.sign(sin) * Math.abs(sin) ** shape * radius +
            jitter(index + samples, frame, amplitude);
        segments.push(
            `${index === 0 ? "M" : "L"}${x.toFixed(2)} ${y.toFixed(2)}`,
        );
    }
    return `${segments.join(" ")}Z`;
}

export const wobbleFrameCount = 3;
export const wobbleIntervalMs = 140;
