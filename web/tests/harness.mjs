// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

// What every scenario under scenarios/ shares: the static server that serves
// web/dist, the browser lookup, the check counter and the page helpers.
//
// Pixels are always read from #document-surface, the document-resolution
// surface the engine writes to. The visible canvas is the presenter's output
// and is sized in CSS pixels under the current pan and zoom, so it is only
// used for geometry and input.

import { createServer } from "node:http";
import { existsSync } from "node:fs";
import { readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

export const distRoot = fileURLToPath(new URL("../dist", import.meta.url));
export const brushColor = { red: 29, green: 33, blue: 41 };

export const contentTypes = {
    ".html": "text/html; charset=utf-8",
    ".js": "text/javascript",
    ".css": "text/css",
    ".wasm": "application/wasm",
    ".ugu": "application/octet-stream",
    ".svg": "image/svg+xml",
};

// withoutEngine serves everything but the engine loader, which is how a
// blocked or missing wasm artifact looks to the browser.
export function startServer({ withoutEngine = false } = {}) {
    const server = createServer(async (request, response) => {
        try {
            const url = new URL(request.url, "http://localhost");
            // The directory index is decided on the URL, before normalize()
            // turns the separators native: on Windows "/" comes back as "\"
            // and would never match a trailing slash.
            let path = decodeURIComponent(url.pathname);
            if (path.endsWith("/")) {
                path += "index.html";
            }
            path = normalize(path);
            if (withoutEngine && path.includes("ugurugu_engine_spike.js")) {
                response.writeHead(404);
                response.end();
                return;
            }
            const file = await readFile(join(distRoot, path));
            response.writeHead(200, {
                "Content-Type":
                    contentTypes[extname(path)] ?? "application/octet-stream",
            });
            response.end(file);
        } catch {
            response.writeHead(404);
            response.end();
        }
    });
    return new Promise((resolve) => {
        server.listen(0, "127.0.0.1", () => {
            resolve({
                server,
                origin: `http://127.0.0.1:${server.address().port}`,
            });
        });
    });
}

export function countBrushPixels(page) {
    return page.evaluate((color) => {
        const canvas = document.querySelector("#document-surface");
        const context = canvas.getContext("2d");
        const pixels = context.getImageData(
            0,
            0,
            canvas.width,
            canvas.height,
        ).data;
        let count = 0;
        for (let index = 0; index < pixels.length; index += 4) {
            if (
                pixels[index] === color.red &&
                pixels[index + 1] === color.green &&
                pixels[index + 2] === color.blue
            ) {
                count += 1;
            }
        }
        return count;
    }, brushColor);
}

export async function installPixelCounter(page) {
    await page.addInitScript(() => {
        // One scan serves both the count and the bounding box: the selection
        // transform checks need to know where the ink went, not just how much
        // of it there is.
        const scan = () => {
            const canvas = document.querySelector("#document-surface");
            if (!canvas) {
                return null;
            }
            const data = canvas
                .getContext("2d")
                .getImageData(0, 0, canvas.width, canvas.height).data;
            let count = 0;
            let left = canvas.width;
            let top = canvas.height;
            let right = -1;
            let bottom = -1;
            for (let y = 0; y < canvas.height; y += 1) {
                for (let x = 0; x < canvas.width; x += 1) {
                    const index = (y * canvas.width + x) * 4;
                    if (
                        data[index] === 29 &&
                        data[index + 1] === 33 &&
                        data[index + 2] === 41
                    ) {
                        count += 1;
                        left = Math.min(left, x);
                        top = Math.min(top, y);
                        right = Math.max(right, x);
                        bottom = Math.max(bottom, y);
                    }
                }
            }
            return count === 0
                ? { count: 0, left: -1, top: -1, right: -1, bottom: -1 }
                : { count, left, top, right, bottom };
        };
        window.__uguruguBrushCount = () => scan()?.count ?? -1;
        window.__uguruguInkBounds = () => scan();
    });
}

export async function waitForDocumentLoaded(page) {
    await page.waitForFunction(
        () =>
            document
                .querySelector("#status")
                ?.textContent.includes("schema v"),
        undefined,
        { timeout: 30000 },
    );
}

export async function drawStroke(page) {
    const box = await page.locator("#display-canvas").boundingBox();
    const centerX = box.x + box.width / 2;
    const centerY = box.y + box.height / 2;
    const path = [{ x: centerX - 80, y: centerY - 20 }];
    for (let step = 1; step <= 12; step += 1) {
        path.push({
            x: centerX - 80 + step * 7,
            y: centerY - 20 + step * 2.5,
        });
    }
    await page.mouse.move(path[0].x, path[0].y);
    await page.mouse.down();
    for (const point of path.slice(1)) {
        await page.mouse.move(point.x, point.y);
    }
    // The live preview must appear while the pointer is still down; it once
    // silently regressed to commit-only rendering.
    await page.waitForFunction(hasBrushPixel, undefined, { timeout: 15000 });
    await page.mouse.up();
    await page.waitForFunction(hasBrushPixel, undefined, { timeout: 15000 });
    return path;
}

// Serialised into the page, so it may not close over anything.
export function hasBrushPixel() {
    const canvas = document.querySelector("#document-surface");
    const data = canvas
        .getContext("2d")
        .getImageData(0, 0, canvas.width, canvas.height).data;
    for (let index = 0; index < data.length; index += 4) {
        if (
            data[index] === 29 &&
            data[index + 1] === 33 &&
            data[index + 2] === 41
        ) {
            return true;
        }
    }
    return false;
}

export async function dragBetween(page, from, to) {
    await page.mouse.move(from.x, from.y);
    await page.mouse.down();
    const steps = 10;
    for (let step = 1; step <= steps; step += 1) {
        await page.mouse.move(
            from.x + ((to.x - from.x) * step) / steps,
            from.y + ((to.y - from.y) * step) / steps,
        );
    }
    await page.mouse.up();
}

// A new document is opaque white, so ink has to be found by color: alpha is
// 255 everywhere.
export function rightmostInk(page) {
    return page.evaluate(() => {
        const canvas = document.querySelector("#document-surface");
        const data = canvas
            .getContext("2d")
            .getImageData(0, 0, canvas.width, canvas.height).data;
        let rightmost = -1;
        for (let y = 0; y < canvas.height; y += 1) {
            for (let x = 0; x < canvas.width; x += 1) {
                const index = (y * canvas.width + x) * 4;
                if (
                    data[index] === 29 &&
                    data[index + 1] === 33 &&
                    data[index + 2] === 41
                ) {
                    rightmost = Math.max(rightmost, x);
                }
            }
        }
        return rightmost;
    });
}

// Serialised into the page, so it may not close over anything.
export function overlayHasInk() {
    const canvas = document.querySelector("#selection-overlay");
    if (!canvas || canvas.width < 2) {
        return false;
    }
    const data = canvas
        .getContext("2d")
        .getImageData(0, 0, canvas.width, canvas.height).data;
    for (let index = 3; index < data.length; index += 4) {
        if (data[index] > 0) {
            return true;
        }
    }
    return false;
}

// The play control is an icon button, so its state lives in the title rather
// than in visible text.
export async function isPlaying(page) {
    const title = await page.locator(".play").getAttribute("title");
    return title?.startsWith("Stop") === true;
}

export function firstThumbnailDataUrl(page) {
    return page.evaluate(() => {
        const thumb = document.querySelector("aside .thumb-box canvas");
        if (!thumb || thumb.width <= 1) {
            return null;
        }
        return thumb.toDataURL();
    });
}

export async function waitForThumbnails(page) {
    await page.waitForFunction(
        () => {
            const thumb = document.querySelector("aside .thumb-box canvas");
            return thumb !== null && thumb.width > 1;
        },
        undefined,
        { timeout: 20000 },
    );
}

export function scratchFile(name) {
    return join(tmpdir(), `ugurugu-verify-${process.pid}-${name}`);
}

// Reads what the compositor actually put on screen at one point. The display
// canvas is WebGL without preserveDrawingBuffer, so it cannot be read back
// directly; the screenshot goes to the page to be decoded instead.
export async function screenPixel(page, x, y) {
    const shot = await page.screenshot({
        clip: { x, y, width: 1, height: 1 },
    });
    return page.evaluate(async (encoded) => {
        const image = new Image();
        image.src = `data:image/png;base64,${encoded}`;
        await image.decode();
        const canvas = document.createElement("canvas");
        canvas.width = 1;
        canvas.height = 1;
        const context = canvas.getContext("2d");
        context.drawImage(image, 0, 0);
        return [...context.getImageData(0, 0, 1, 1).data];
    }, shot.toString("base64"));
}

export function opaquePixelCount(page, selector) {
    return page.evaluate((id) => {
        const canvas = document.querySelector(id);
        if (!canvas) {
            return -1;
        }
        const data = canvas
            .getContext("2d")
            .getImageData(0, 0, canvas.width, canvas.height).data;
        let opaque = 0;
        for (let index = 3; index < data.length; index += 4) {
            if (data[index] > 0) {
                opaque += 1;
            }
        }
        return opaque;
    }, selector);
}

// Columns of the document surface that carry brush ink, collapsed into runs.
// Two separate strokes have to stay two runs: one run means something bridged
// them, and a run that is only a few columns wide means a stroke lost the
// points that came after its first.
export function inkColumnRuns(page) {
    return page.evaluate((color) => {
        const canvas = document.querySelector("#document-surface");
        const data = canvas
            .getContext("2d")
            .getImageData(0, 0, canvas.width, canvas.height).data;
        const runs = [];
        let current = null;
        for (let x = 0; x < canvas.width; x += 1) {
            let inked = false;
            for (let y = 0; y < canvas.height && !inked; y += 1) {
                const index = (y * canvas.width + x) * 4;
                inked =
                    data[index] === color.red &&
                    data[index + 1] === color.green &&
                    data[index + 2] === color.blue;
            }
            if (inked) {
                current ??= { start: x, end: x };
                current.end = x;
            } else if (current) {
                runs.push(current);
                current = null;
            }
        }
        if (current) {
            runs.push(current);
        }
        return runs;
    }, brushColor);
}

// Delays every message on its way to the worker, which is how a document whose
// renders cost more than the gap between two clicks behaves. Scenarios opt in
// by setting window.__slowEngine to a number of milliseconds.
export function installWorkerDelay(page) {
    return page.addInitScript(() => {
        window.__sent = 0;
        window.__received = 0;
        const BaseWorker = window.Worker;
        window.Worker = class extends BaseWorker {
            constructor(...args) {
                super(...args);
                const post = this.postMessage.bind(this);
                this.postMessage = (...rest) => {
                    window.__sent += 1;
                    if (window.__slowEngine) {
                        setTimeout(() => post(...rest), window.__slowEngine);
                        return undefined;
                    }
                    return post(...rest);
                };
                this.addEventListener("message", () => {
                    window.__received += 1;
                });
            }
        };
    });
}

export async function drainWorker(page) {
    await page.evaluate(() => {
        window.__slowEngine = 0;
    });
    await page.waitForFunction(
        () => window.__sent === window.__received,
        undefined,
        { timeout: 120000 },
    );
}

// A straight horizontal drag with no waiting in between, so the whole stroke is
// handed to the shell before the engine has answered anything.
export async function quickStroke(page, box, fromFraction, toFraction, yFraction) {
    const y = box.y + box.height * yFraction;
    const from = box.x + box.width * fromFraction;
    const to = box.x + box.width * toFraction;
    await page.mouse.move(from, y);
    await page.mouse.down();
    for (let step = 1; step <= 6; step += 1) {
        await page.mouse.move(from + ((to - from) * step) / 6, y);
    }
    await page.mouse.up();
}

export async function setCanvasRotation(page, degrees) {
    const input = page.locator("#rotation-angle");
    await input.fill(String(degrees));
    await input.press("Enter");
    await input.blur();
    await page.waitForFunction(
        (expected) =>
            Number(document.querySelector("#rotation-angle")?.value) === expected,
        degrees,
    );
}

export const failures = [];

export function check(condition, label) {
    if (condition) {
        console.log(`ok: ${label}`);
    } else {
        console.error(`FAIL: ${label}`);
        failures.push(label);
    }
}

export const { chromium } = await import("playwright-core");
// playwright-core never downloads a browser, so the executable has to come from
// the environment, a Playwright install, or a local Chromium app bundle.
export function resolveChromiumPath() {
    const candidates = [
        process.env.UGURUGU_CHROMIUM_PATH,
        safeExecutablePath(),
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    ];
    for (const candidate of candidates) {
        if (candidate && existsSync(candidate)) {
            return candidate;
        }
    }
    throw new Error(
        "No Chromium found. Run `npx playwright install chromium` or set " +
            "UGURUGU_CHROMIUM_PATH.",
    );
}

export function safeExecutablePath() {
    try {
        return chromium.executablePath();
    } catch {
        return null;
    }
}
