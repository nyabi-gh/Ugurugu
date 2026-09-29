// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import { toViewport } from "./ViewTransform";
import type { ViewState } from "./ViewTransform";

export interface DirtyRegion {
    x: number;
    y: number;
    width: number;
    height: number;
}

// Two surfaces on purpose. The document surface is a document-resolution 2D
// canvas that stays the authority for pixels: the eyedropper samples it and
// PNG export encodes it, so both are independent of the current zoom. The
// display surface only ever shows that image under the view transform, which
// is what WebGL 2 accelerates; Canvas 2D is the fallback when no context is
// available, exactly as the feasibility report scoped it.
export class CanvasPresenter {
    #surface: HTMLCanvasElement;
    #surfaceContext: CanvasRenderingContext2D;
    #display: HTMLCanvasElement;
    // Second element, created only when the software path is in use. A canvas
    // keeps the first context type it is ever given, so once WebGL has touched
    // the display canvas, getContext("2d") on it returns null forever; the
    // fallback has to be a different element.
    #softwareCanvas: HTMLCanvasElement | null = null;
    #gl: WebGL2RenderingContext | null = null;
    #displayContext: CanvasRenderingContext2D | null = null;
    #program: WebGLProgram | null = null;
    #texture: WebGLTexture | null = null;
    #originLocation: WebGLUniformLocation | null = null;
    #xAxisLocation: WebGLUniformLocation | null = null;
    #yAxisLocation: WebGLUniformLocation | null = null;
    #documentWidth = 0;
    #documentHeight = 0;
    #textureDirty = true;
    #lastView: ViewState | null = null;
    #onDisplayModeChange: (usingWebGL: boolean) => void;

    constructor(
        surface: HTMLCanvasElement,
        display: HTMLCanvasElement,
        onDisplayModeChange: (usingWebGL: boolean) => void = () => {},
    ) {
        this.#surface = surface;
        const context = surface.getContext("2d", { willReadFrequently: true });
        if (!context) {
            throw new Error("2D canvas context is unavailable");
        }
        this.#surfaceContext = context;
        this.#display = display;
        this.#onDisplayModeChange = onDisplayModeChange;
        this.#initializeDisplay();
    }

    get usingWebGL(): boolean {
        return this.#gl !== null;
    }

    get documentSurface(): HTMLCanvasElement {
        return this.#surface;
    }

    #initializeDisplay() {
        const gl = this.#display.getContext("webgl2", {
            alpha: true,
            antialias: false,
            depth: false,
            stencil: false,
            premultipliedAlpha: false,
        });
        if (!gl) {
            this.#useSoftwareDisplay();
            return;
        }
        const program = createProgram(gl);
        if (!program) {
            this.#useSoftwareDisplay();
            return;
        }
        this.#gl = gl;
        this.#program = program;
        this.#texture = gl.createTexture();
        this.#originLocation = gl.getUniformLocation(program, "uOrigin");
        this.#xAxisLocation = gl.getUniformLocation(program, "uXAxis");
        this.#yAxisLocation = gl.getUniformLocation(program, "uYAxis");
        gl.bindTexture(gl.TEXTURE_2D, this.#texture);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
        // Bilinear both ways, because that is what the desktop shows. Its GPU
        // blit samples Nearest, but only after ImageResampler has already
        // resampled the frame to the display size with bilinear taps, so
        // magnifying with NEAREST here would be a visible regression.
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
        // The engine hands out straight (unpremultiplied) RGBA, and the
        // context above was created the same way, so no unpack conversion.
        gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
        gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
        // A context loss leaves every object above invalid; falling back to the
        // software path keeps the shell usable instead of freezing on a blank
        // canvas. The move is one way: a context that was lost once can be lost
        // again, and the document surface holds every pixel we need anyway.
        this.#display.addEventListener("webglcontextlost", (event) => {
            event.preventDefault();
            this.#gl = null;
            this.#program = null;
            this.#texture = null;
            this.#textureDirty = true;
            this.#useSoftwareDisplay();
        });
    }

    // Lays a 2D canvas over the display canvas and draws there from now on. It
    // is transparent to pointer events so the display canvas keeps receiving
    // them, and it is inserted directly after so the selection overlay stays on
    // top of both.
    #useSoftwareDisplay() {
        if (this.#softwareCanvas) {
            return;
        }
        const software = document.createElement("canvas");
        software.id = "display-software";
        software.setAttribute("aria-hidden", "true");
        software.style.position = "absolute";
        software.style.inset = "0";
        software.style.inlineSize = "100%";
        software.style.blockSize = "100%";
        software.style.pointerEvents = "none";
        software.width = Math.max(1, this.#display.width);
        software.height = Math.max(1, this.#display.height);
        this.#display.after(software);
        this.#softwareCanvas = software;
        this.#displayContext = software.getContext("2d");
        this.#onDisplayModeChange(false);
        if (this.#lastView) {
            this.#drawSoftware(this.#lastView);
        }
    }

    resizeDocument(width: number, height: number) {
        this.#documentWidth = width;
        this.#documentHeight = height;
        this.#surface.width = width;
        this.#surface.height = height;
        this.#surfaceContext.clearRect(0, 0, width, height);
        this.#textureDirty = true;
    }

    resizeDisplay(width: number, height: number, devicePixelRatio: number) {
        const pixelWidth = Math.max(1, Math.round(width * devicePixelRatio));
        const pixelHeight = Math.max(1, Math.round(height * devicePixelRatio));
        for (const canvas of [this.#display, this.#softwareCanvas]) {
            if (
                canvas === null ||
                (canvas.width === pixelWidth && canvas.height === pixelHeight)
            ) {
                continue;
            }
            canvas.width = pixelWidth;
            canvas.height = pixelHeight;
        }
    }

    writeRegion(region: DirtyRegion, pixels: Uint8ClampedArray<ArrayBuffer>) {
        if (region.width <= 0 || region.height <= 0) {
            return;
        }
        this.#surfaceContext.putImageData(
            new ImageData(pixels, region.width, region.height),
            region.x,
            region.y,
        );
        const gl = this.#gl;
        if (!gl || !this.#texture) {
            return;
        }
        gl.bindTexture(gl.TEXTURE_2D, this.#texture);
        if (this.#textureDirty) {
            gl.texImage2D(
                gl.TEXTURE_2D,
                0,
                gl.RGBA,
                this.#documentWidth,
                this.#documentHeight,
                0,
                gl.RGBA,
                gl.UNSIGNED_BYTE,
                null,
            );
            this.#textureDirty = false;
            // A fresh texture has no pixels outside the dirty rectangle, so
            // the whole surface has to be uploaded once.
            gl.texSubImage2D(
                gl.TEXTURE_2D,
                0,
                0,
                0,
                this.#documentWidth,
                this.#documentHeight,
                gl.RGBA,
                gl.UNSIGNED_BYTE,
                this.#surfaceContext.getImageData(
                    0,
                    0,
                    this.#documentWidth,
                    this.#documentHeight,
                ).data,
            );
            return;
        }
        gl.texSubImage2D(
            gl.TEXTURE_2D,
            0,
            region.x,
            region.y,
            region.width,
            region.height,
            gl.RGBA,
            gl.UNSIGNED_BYTE,
            pixels,
        );
    }

    draw(view: ViewState) {
        this.#lastView = view;
        const gl = this.#gl;
        if (!gl || !this.#program) {
            this.#drawSoftware(view);
            return;
        }
        const cssWidth = this.#display.clientWidth || this.#display.width;
        const cssHeight = this.#display.clientHeight || this.#display.height;
        const topLeft = toViewport(
            view,
            { width: cssWidth, height: cssHeight },
            0,
            0,
        );
        const topRight = toViewport(
            view,
            { width: cssWidth, height: cssHeight },
            this.#documentWidth,
            0,
        );
        const bottomLeft = toViewport(
            view,
            { width: cssWidth, height: cssHeight },
            0,
            this.#documentHeight,
        );
        gl.viewport(0, 0, this.#display.width, this.#display.height);
        gl.clearColor(0, 0, 0, 0);
        gl.clear(gl.COLOR_BUFFER_BIT);
        if (this.#documentWidth <= 0 || this.#documentHeight <= 0) {
            return;
        }
        gl.useProgram(this.#program);
        gl.bindTexture(gl.TEXTURE_2D, this.#texture);
        // Clip-space origin and basis vectors for the document quad, with y
        // flipped from the viewport's downward-positive coordinate system.
        const originX = (topLeft.x / cssWidth) * 2 - 1;
        const originY = 1 - (topLeft.y / cssHeight) * 2;
        gl.uniform2f(this.#originLocation, originX, originY);
        gl.uniform2f(
            this.#xAxisLocation,
            ((topRight.x - topLeft.x) / cssWidth) * 2,
            ((topLeft.y - topRight.y) / cssHeight) * 2,
        );
        gl.uniform2f(
            this.#yAxisLocation,
            ((bottomLeft.x - topLeft.x) / cssWidth) * 2,
            ((topLeft.y - bottomLeft.y) / cssHeight) * 2,
        );
        // Blending stays off. The context is premultipliedAlpha: false, so the
        // compositor reads the drawing buffer as straight RGBA — exactly what
        // the texture already holds. SRC_ALPHA blending over the transparent
        // clear would premultiply the colour and square the alpha, which turned
        // a half-opaque layer into something close to black.
        gl.disable(gl.BLEND);
        gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    }

    #drawSoftware(view: ViewState) {
        const context = this.#displayContext;
        if (!context) {
            return;
        }
        const target = this.#softwareCanvas ?? this.#display;
        const cssWidth = target.clientWidth || target.width;
        const cssHeight = target.clientHeight || target.height;
        context.setTransform(1, 0, 0, 1, 0, 0);
        context.clearRect(0, 0, target.width, target.height);
        if (this.#documentWidth <= 0 || this.#documentHeight <= 0) {
            return;
        }
        context.setTransform(
            target.width / Math.max(1, cssWidth),
            0,
            0,
            target.height / Math.max(1, cssHeight),
            0,
            0,
        );
        context.translate(cssWidth / 2, cssHeight / 2);
        context.rotate((view.rotation * Math.PI) / 180);
        context.scale(view.scale, view.scale);
        context.translate(-view.centerX, -view.centerY);
        context.imageSmoothingEnabled = true;
        context.drawImage(this.#surface, 0, 0);
    }

    readPixel(x: number, y: number): [number, number, number] {
        const pixelX = Math.min(
            this.#documentWidth - 1,
            Math.max(0, Math.floor(x)),
        );
        const pixelY = Math.min(
            this.#documentHeight - 1,
            Math.max(0, Math.floor(y)),
        );
        const [red = 0, green = 0, blue = 0] = this.#surfaceContext.getImageData(
            pixelX,
            pixelY,
            1,
            1,
        ).data;
        return [red, green, blue];
    }

    toBlob(type: string): Promise<Blob | null> {
        return new Promise((resolve) => {
            this.#surface.toBlob(resolve, type);
        });
    }
}

const vertexSource = `#version 300 es
uniform vec2 uOrigin;
uniform vec2 uXAxis;
uniform vec2 uYAxis;
out vec2 vTexture;
void main() {
    vec2 corner = vec2(float(gl_VertexID & 1), float(gl_VertexID >> 1));
    vTexture = corner;
    gl_Position = vec4(
        uOrigin + corner.x * uXAxis + corner.y * uYAxis,
        0.0,
        1.0);
}`;

const fragmentSource = `#version 300 es
precision mediump float;
uniform sampler2D uSurface;
in vec2 vTexture;
out vec4 fragment;
void main() {
    fragment = texture(uSurface, vTexture);
}`;

function compile(
    gl: WebGL2RenderingContext,
    type: number,
    source: string,
): WebGLShader | null {
    const shader = gl.createShader(type);
    if (!shader) {
        return null;
    }
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
        gl.deleteShader(shader);
        return null;
    }
    return shader;
}

function createProgram(gl: WebGL2RenderingContext): WebGLProgram | null {
    const vertex = compile(gl, gl.VERTEX_SHADER, vertexSource);
    const fragment = compile(gl, gl.FRAGMENT_SHADER, fragmentSource);
    if (!vertex || !fragment) {
        return null;
    }
    const program = gl.createProgram();
    if (!program) {
        return null;
    }
    gl.attachShader(program, vertex);
    gl.attachShader(program, fragment);
    gl.linkProgram(program);
    gl.deleteShader(vertex);
    gl.deleteShader(fragment);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
        gl.deleteProgram(program);
        return null;
    }
    return program;
}
