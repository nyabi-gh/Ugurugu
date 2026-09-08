// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

export interface ViewState {
    // CSS pixels per document pixel.
    scale: number;
    // Clockwise degrees, normalized to [-180, 180).
    rotation: number;
    // Document point pinned to the centre of the viewport.
    centerX: number;
    centerY: number;
}

export const minimumScale = 0.05;
export const maximumScale = 32;

export interface Viewport {
    width: number;
    height: number;
}

// Never magnifies. Nothing renders above native resolution — the desktop
// clamps PreviewRenderPolicy::renderSize to scale 1.0 and this shell renders
// at document size — so a fit that enlarged a small document would only put
// its own pixel grid on screen. Shrinking to fit a large document is the case
// this is for; enlarging stays an explicit zoom.
export function fitToViewport(
    document: { width: number; height: number },
    viewport: Viewport,
    rotation = 0,
): ViewState {
    const normalizedRotation = normalizeRotation(rotation);
    const radians = degreesToRadians(normalizedRotation);
    const cosine = Math.abs(Math.cos(radians));
    const sine = Math.abs(Math.sin(radians));
    const rotatedWidth = document.width * cosine + document.height * sine;
    const rotatedHeight = document.width * sine + document.height * cosine;
    const scale =
        document.width <= 0 || document.height <= 0
            ? 1
            : Math.min(
                  viewport.width / rotatedWidth,
                  viewport.height / rotatedHeight,
                  1,
              );
    return {
        scale: Math.max(minimumScale, scale),
        rotation: normalizedRotation,
        centerX: document.width / 2,
        centerY: document.height / 2,
    };
}

export function clampScale(scale: number): number {
    return Math.min(maximumScale, Math.max(minimumScale, scale));
}

export function normalizeRotation(rotation: number): number {
    if (!Number.isFinite(rotation)) {
        return 0;
    }
    return ((((rotation + 180) % 360) + 360) % 360) - 180;
}

export function toDocument(
    view: ViewState,
    viewport: Viewport,
    viewportX: number,
    viewportY: number,
): { x: number; y: number } {
    const radians = degreesToRadians(view.rotation);
    const cosine = Math.cos(radians);
    const sine = Math.sin(radians);
    const offsetX = (viewportX - viewport.width / 2) / view.scale;
    const offsetY = (viewportY - viewport.height / 2) / view.scale;
    return {
        x: view.centerX + cosine * offsetX + sine * offsetY,
        y: view.centerY - sine * offsetX + cosine * offsetY,
    };
}

export function toViewport(
    view: ViewState,
    viewport: Viewport,
    documentX: number,
    documentY: number,
): { x: number; y: number } {
    const radians = degreesToRadians(view.rotation);
    const cosine = Math.cos(radians);
    const sine = Math.sin(radians);
    const offsetX = documentX - view.centerX;
    const offsetY = documentY - view.centerY;
    return {
        x:
            viewport.width / 2 +
            (cosine * offsetX - sine * offsetY) * view.scale,
        y:
            viewport.height / 2 +
            (sine * offsetX + cosine * offsetY) * view.scale,
    };
}

// Keeps the document point at one viewport position pinned to another while
// the scale and rotation change. Pinch gestures use the previous and current
// midpoint respectively, so translation, zoom, and rotation stay in one
// continuous transform rather than accumulating independent rounding errors.
export function transformAround(
    view: ViewState,
    viewport: Viewport,
    anchorViewportX: number,
    anchorViewportY: number,
    nextViewportX: number,
    nextViewportY: number,
    nextScale: number,
    nextRotation: number,
): ViewState {
    const anchor = toDocument(
        view,
        viewport,
        anchorViewportX,
        anchorViewportY,
    );
    const scale = clampScale(nextScale);
    const rotation = normalizeRotation(nextRotation);
    const radians = degreesToRadians(rotation);
    const cosine = Math.cos(radians);
    const sine = Math.sin(radians);
    const offsetX = (nextViewportX - viewport.width / 2) / scale;
    const offsetY = (nextViewportY - viewport.height / 2) / scale;
    return {
        scale,
        rotation,
        centerX: anchor.x - cosine * offsetX - sine * offsetY,
        centerY: anchor.y + sine * offsetX - cosine * offsetY,
    };
}

// Keeps the document point under the cursor pinned while the scale changes,
// which is what makes wheel and pinch zoom feel anchored rather than jumpy.
export function zoomAround(
    view: ViewState,
    viewport: Viewport,
    viewportX: number,
    viewportY: number,
    nextScale: number,
): ViewState {
    return transformAround(
        view,
        viewport,
        viewportX,
        viewportY,
        viewportX,
        viewportY,
        nextScale,
        view.rotation,
    );
}

export function pan(
    view: ViewState,
    deltaViewportX: number,
    deltaViewportY: number,
): ViewState {
    const radians = degreesToRadians(view.rotation);
    const cosine = Math.cos(radians);
    const sine = Math.sin(radians);
    return {
        scale: view.scale,
        rotation: normalizeRotation(view.rotation),
        centerX:
            view.centerX -
            (cosine * deltaViewportX + sine * deltaViewportY) / view.scale,
        centerY:
            view.centerY +
            (sine * deltaViewportX - cosine * deltaViewportY) / view.scale,
    };
}

function degreesToRadians(degrees: number): number {
    return (degrees * Math.PI) / 180;
}

export interface PinchMeasurement {
    centerX: number;
    centerY: number;
    distance: number;
    angle: number;
}

// The midpoint, span and tilt of exactly two touches, which is what a pinch
// compares between two moves to produce one transform.
export function pinchMeasurement(
    touches: Iterable<{ x: number; y: number }>,
): PinchMeasurement | null {
    const [first, second, third] = [...touches];
    if (!first || !second || third) {
        return null;
    }
    const deltaX = second.x - first.x;
    const deltaY = second.y - first.y;
    return {
        centerX: (first.x + second.x) / 2,
        centerY: (first.y + second.y) / 2,
        distance: Math.hypot(deltaX, deltaY),
        angle: (Math.atan2(deltaY, deltaX) * 180) / Math.PI,
    };
}
