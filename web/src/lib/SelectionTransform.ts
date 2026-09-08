// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import type { SelectionContour } from "./EngineClient";

// Affine transform in QTransform's row-vector layout, which is also the order
// the bridge takes its six doubles in: x' = m11·x + m21·y + dx.
export type Matrix = [number, number, number, number, number, number];

export interface Bounds {
    left: number;
    top: number;
    right: number;
    bottom: number;
}

export const identity: Matrix = [1, 0, 0, 1, 0, 0];

// Not a fuzzy compare on purpose: a pending transform is "dirty" the moment a
// gesture writes anything into it, and every gesture that ends where it started
// restores the exact identity values it started from.
export function isIdentity(matrix: Matrix): boolean {
    return (
        matrix[0] === 1 &&
        matrix[1] === 0 &&
        matrix[2] === 0 &&
        matrix[3] === 1 &&
        matrix[4] === 0 &&
        matrix[5] === 0
    );
}

// Applies a first, then b — the order QTransform's operator* composes in.
export function compose(a: Matrix, b: Matrix): Matrix {
    const [a11, a12, a21, a22, adx, ady] = a;
    const [b11, b12, b21, b22, bdx, bdy] = b;
    return [
        a11 * b11 + a12 * b21,
        a11 * b12 + a12 * b22,
        a21 * b11 + a22 * b21,
        a21 * b12 + a22 * b22,
        adx * b11 + ady * b21 + bdx,
        adx * b12 + ady * b22 + bdy,
    ];
}

export function mapPoint(
    matrix: Matrix,
    x: number,
    y: number,
): { x: number; y: number } {
    return {
        x: matrix[0] * x + matrix[2] * y + matrix[4],
        y: matrix[1] * x + matrix[3] * y + matrix[5],
    };
}

export function translation(dx: number, dy: number): Matrix {
    return [1, 0, 0, 1, dx, dy];
}

export function scaleAbout(
    centerX: number,
    centerY: number,
    factor: number,
): Matrix {
    return [
        factor,
        0,
        0,
        factor,
        centerX - factor * centerX,
        centerY - factor * centerY,
    ];
}

// Positive degrees turn the content clockwise on screen, matching
// QTransform::rotate and the desktop's Rotate selection dialog.
export function rotationAbout(
    centerX: number,
    centerY: number,
    degrees: number,
): Matrix {
    const radians = (degrees * Math.PI) / 180;
    const cos = Math.cos(radians);
    const sin = Math.sin(radians);
    return [
        cos,
        sin,
        -sin,
        cos,
        centerX - centerX * cos + centerY * sin,
        centerY - centerX * sin - centerY * cos,
    ];
}

export function flipAbout(
    centerX: number,
    centerY: number,
    horizontal: boolean,
): Matrix {
    return horizontal
        ? [-1, 0, 0, 1, 2 * centerX, 0]
        : [1, 0, 0, -1, 0, 2 * centerY];
}

// Bounding box of the outline once the pending transform is applied, in
// document coordinates. Every gesture anchors on the centre of this box, so a
// second scale grows what the artist currently sees rather than the original.
export function transformedBounds(
    contours: SelectionContour[],
    matrix: Matrix,
): Bounds | null {
    let left = Number.POSITIVE_INFINITY;
    let top = Number.POSITIVE_INFINITY;
    let right = Number.NEGATIVE_INFINITY;
    let bottom = Number.NEGATIVE_INFINITY;
    for (const contour of contours) {
        for (let index = 0; index + 1 < contour.length; index += 2) {
            const point = mapPoint(
                matrix,
                contour[index] ?? 0,
                contour[index + 1] ?? 0,
            );
            left = Math.min(left, point.x);
            top = Math.min(top, point.y);
            right = Math.max(right, point.x);
            bottom = Math.max(bottom, point.y);
        }
    }
    if (!Number.isFinite(left) || !Number.isFinite(top)) {
        return null;
    }
    return { left, top, right, bottom };
}

export function boundsCenter(bounds: Bounds): { x: number; y: number } {
    return {
        x: (bounds.left + bounds.right) / 2,
        y: (bounds.top + bounds.bottom) / 2,
    };
}
