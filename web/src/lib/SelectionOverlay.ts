// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyattic)

import { toViewport } from "./ViewTransform";
import type { ViewState, Viewport } from "./ViewTransform";
import type { SelectionContour } from "./EngineClient";
import { identity, mapPoint } from "./SelectionTransform";
import type { Matrix } from "./SelectionTransform";

export interface DragShape {
    shape: "freehand" | "rectangle" | "ellipse";
    points: number[];
}

// Draws over the display canvas, never into the document: the committed
// selection as marching ants, plus whatever the pointer is drawing right now.
// Two passes, light under dark, so the outline reads over any artwork —
// canvas_detail::drawSelectionPath does the same on the desktop.
export class SelectionOverlay {
    #canvas: HTMLCanvasElement;
    #context: CanvasRenderingContext2D | null;
    #contours: SelectionContour[] = [];
    #drag: DragShape | null = null;
    #dashOffset = 0;
    #frame = 0;
    #text: { path: Path2D; x: number; y: number; color: string; width: number; filled: boolean } | null = null;

    setText(text: { path: Path2D; x: number; y: number; color: string; width: number; filled: boolean } | null) { this.#text = text; }

    // Pending floating transform. The engine keeps sending the committed
    // outline while a selection is being moved, so the ants are mapped through
    // this on the way to the screen instead of being re-read every frame.
    #transform: Matrix = identity;

    constructor(canvas: HTMLCanvasElement) {
        this.#canvas = canvas;
        this.#context = canvas.getContext("2d");
    }

    setContours(contours: SelectionContour[]) {
        this.#contours = contours;
    }

    setTransform(transform: Matrix) {
        this.#transform = transform;
    }

    // Even-odd so a selection with holes reports the holes as outside, the same
    // reading CanvasWidget::selectionContains gets from its mask.
    containsDocumentPoint(x: number, y: number): boolean {
        const context = this.#context;
        if (!context || this.#contours.length === 0) {
            return false;
        }
        const path = new Path2D();
        for (const contour of this.#contours) {
            for (let index = 0; index + 1 < contour.length; index += 2) {
                const point = mapPoint(
                    this.#transform,
                    contour[index] ?? 0,
                    contour[index + 1] ?? 0,
                );
                if (index === 0) {
                    path.moveTo(point.x, point.y);
                } else {
                    path.lineTo(point.x, point.y);
                }
            }
            path.closePath();
        }
        context.save();
        context.setTransform(1, 0, 0, 1, 0, 0);
        const inside = context.isPointInPath(path, x, y, "evenodd");
        context.restore();
        return inside;
    }

    setDrag(drag: DragShape | null) {
        this.#drag = drag;
    }

    get hasContent(): boolean {
        return this.#contours.length > 0 || this.#drag !== null || this.#text !== null;
    }

    resize(width: number, height: number, devicePixelRatio: number) {
        const pixelWidth = Math.max(1, Math.round(width * devicePixelRatio));
        const pixelHeight = Math.max(1, Math.round(height * devicePixelRatio));
        if (
            this.#canvas.width === pixelWidth &&
            this.#canvas.height === pixelHeight
        ) {
            return;
        }
        this.#canvas.width = pixelWidth;
        this.#canvas.height = pixelHeight;
    }

    // Advances the ants. Returns true when something is on screen, so the
    // caller can stop its animation loop once the selection goes away.
    advance(): boolean {
        this.#frame += 1;
        if (this.#frame % 3 === 0) {
            this.#dashOffset = (this.#dashOffset + 1) % 8;
        }
        return this.hasContent;
    }

    draw(view: ViewState) {
        const context = this.#context;
        if (!context) {
            return;
        }
        const cssWidth = this.#canvas.clientWidth || this.#canvas.width;
        const cssHeight = this.#canvas.clientHeight || this.#canvas.height;
        const ratio = this.#canvas.width / Math.max(1, cssWidth);
        context.setTransform(ratio, 0, 0, ratio, 0, 0);
        context.clearRect(0, 0, cssWidth, cssHeight);
        if (!this.hasContent) {
            return;
        }

        const viewport: Viewport = { width: cssWidth, height: cssHeight };
        if (this.#text) {
            const text = this.#text;
            const origin = toViewport(view, viewport, text.x, text.y);
            context.save();
            context.translate(origin.x, origin.y);
            context.rotate(view.rotation * Math.PI / 180);
            context.scale(view.scale, view.scale);
            context.lineJoin = "round";
            context.lineCap = "round";
            context.lineWidth = text.width;
            context.strokeStyle = text.color;
            context.fillStyle = text.color;
            if (text.filled) context.fill(text.path);
            context.stroke(text.path);
            context.restore();
        }
        const path = new Path2D();
        for (const contour of this.#contours) {
            for (let index = 0; index + 1 < contour.length; index += 2) {
                const moved = mapPoint(
                    this.#transform,
                    contour[index] ?? 0,
                    contour[index + 1] ?? 0,
                );
                const point = toViewport(view, viewport, moved.x, moved.y);
                if (index === 0) {
                    path.moveTo(point.x, point.y);
                } else {
                    path.lineTo(point.x, point.y);
                }
            }
            path.closePath();
        }
        this.#appendDrag(path, view, viewport);

        context.lineJoin = "miter";
        context.setLineDash([]);
        context.lineWidth = 2.4;
        context.strokeStyle = "rgba(255, 255, 255, 0.92)";
        context.stroke(path);

        context.lineWidth = 1.2;
        context.setLineDash([4, 4]);
        context.lineDashOffset = this.#dashOffset;
        context.strokeStyle = "rgba(18, 19, 22, 0.96)";
        context.stroke(path);
        context.setLineDash([]);
    }

    #appendDrag(path: Path2D, view: ViewState, viewport: Viewport) {
        const drag = this.#drag;
        if (!drag || drag.points.length < 4) {
            return;
        }
        const at = (index: number) =>
            toViewport(
                view,
                viewport,
                drag.points[index * 2] ?? 0,
                drag.points[index * 2 + 1] ?? 0,
            );
        if (drag.shape === "freehand") {
            const start = at(0);
            path.moveTo(start.x, start.y);
            for (let index = 1; index < drag.points.length / 2; index += 1) {
                const point = at(index);
                path.lineTo(point.x, point.y);
            }
            path.closePath();
            return;
        }
        const last = drag.points.length - 2;
        const anchorX = drag.points[0] ?? 0;
        const anchorY = drag.points[1] ?? 0;
        const currentX = drag.points[last] ?? 0;
        const currentY = drag.points[last + 1] ?? 0;
        const left = Math.min(anchorX, currentX);
        const top = Math.min(anchorY, currentY);
        const right = Math.max(anchorX, currentX);
        const bottom = Math.max(anchorY, currentY);
        if (drag.shape === "rectangle") {
            const topLeft = toViewport(view, viewport, left, top);
            const topRight = toViewport(view, viewport, right, top);
            const bottomRight = toViewport(view, viewport, right, bottom);
            const bottomLeft = toViewport(view, viewport, left, bottom);
            path.moveTo(topLeft.x, topLeft.y);
            path.lineTo(topRight.x, topRight.y);
            path.lineTo(bottomRight.x, bottomRight.y);
            path.lineTo(bottomLeft.x, bottomLeft.y);
            path.closePath();
            return;
        }
        const centerX = (left + right) / 2;
        const centerY = (top + bottom) / 2;
        const center = toViewport(view, viewport, centerX, centerY);
        const horizontal = toViewport(view, viewport, right, centerY);
        const vertical = toViewport(view, viewport, centerX, bottom);
        path.ellipse(
            center.x,
            center.y,
            Math.hypot(horizontal.x - center.x, horizontal.y - center.y),
            Math.hypot(vertical.x - center.x, vertical.y - center.y),
            Math.atan2(horizontal.y - center.y, horizontal.x - center.x),
            0,
            Math.PI * 2,
        );
    }
}
