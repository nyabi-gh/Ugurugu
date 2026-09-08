// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import { readFile } from "node:fs/promises";
import {
    check,
    countBrushPixels,
    drawStroke,
    firstThumbnailDataUrl,
    installPixelCounter,
    isPlaying,
    waitForDocumentLoaded,
    waitForThumbnails,
} from "../harness.mjs";

// draw, autosave, reload, restore, then export the frame as PNG.
export default async function run({ browser, origin }) {
    const context = await browser.newContext({ acceptDownloads: true });
    const page = await context.newPage();
    await installPixelCounter(page);
    await page.goto(`${origin}/?autosave=1`);
    await waitForDocumentLoaded(page);
    check(
        (await page.locator(".recovery-banner").count()) === 0,
        "fresh profile shows no recovery banner",
    );
    check(
        (await countBrushPixels(page)) === 0,
        "canvas starts without brush-colored pixels",
    );
    await waitForThumbnails(page);
    const thumbnailBeforeStroke = await firstThumbnailDataUrl(page);
    check(
        thumbnailBeforeStroke !== null,
        "layer thumbnail rendered after load",
    );

    const strokePath = await drawStroke(page);
    const drawnPixels = await countBrushPixels(page);
    check(drawnPixels > 0, `stroke committed (${drawnPixels} px)`);

    await page.waitForFunction(
        (before) => {
            const thumb = document.querySelector("aside .thumb-box canvas");
            return thumb !== null && thumb.width > 1
                && thumb.toDataURL() !== before;
        },
        thumbnailBeforeStroke,
        { timeout: 20000 },
    );
    check(true, "layer thumbnail refreshed after the stroke");

    await page.waitForFunction(
        () =>
            document
                .querySelector("#autosave-status")
                ?.textContent.includes("Recovery snapshot saved"),
        undefined,
        { timeout: 20000 },
    );
    check(true, "autosave snapshot reported in status bar");

    await page.reload();
    await page.locator("#recovery-restore").waitFor({ timeout: 30000 });
    check(true, "recovery banner offered after reload");
    await page.locator("#recovery-restore").click();
    await page.waitForFunction(
        (expected) => {
            const canvas = document.querySelector("#document-surface");
            if (!canvas) {
                return false;
            }
            const data = canvas
                .getContext("2d")
                .getImageData(0, 0, canvas.width, canvas.height).data;
            let count = 0;
            for (let index = 0; index < data.length; index += 4) {
                if (
                    data[index] === 29 &&
                    data[index + 1] === 33 &&
                    data[index + 2] === 41
                ) {
                    count += 1;
                }
            }
            return count === expected;
        },
        drawnPixels,
        { timeout: 30000 },
    );
    check(true, `restored document renders the stroke (${drawnPixels} px)`);

    const downloadPromise = page.waitForEvent("download", { timeout: 20000 });
    await page.locator("#export-png").click();
    const download = await downloadPromise;
    const pngPath = await download.path();
    const pngBytes = await readFile(pngPath);
    check(
        download.suggestedFilename().endsWith(".png"),
        `export filename: ${download.suggestedFilename()}`,
    );
    check(
        pngBytes.length > 8 &&
            pngBytes[0] === 0x89 &&
            pngBytes[1] === 0x50 &&
            pngBytes[2] === 0x4e &&
            pngBytes[3] === 0x47,
        `export produced a PNG (${pngBytes.length} bytes)`,
    );

    const gifDownloadPromise = page.waitForEvent("download", {
        timeout: 60000,
    });
    await page.locator("#export-gif").click();
    const gifDownload = await gifDownloadPromise;
    const gifBytes = await readFile(await gifDownload.path());
    check(
        gifDownload.suggestedFilename().endsWith(".gif"),
        `gif export filename: ${gifDownload.suggestedFilename()}`,
    );
    check(
        gifBytes.length > 6 &&
            gifBytes.slice(0, 6).toString("latin1") === "GIF89a",
        `animated GIF exported (${gifBytes.length} bytes)`,
    );

    await waitForThumbnails(page);
    check(true, "layer thumbnails rendered for the restored document");

    check(
        (await page
            .locator('#recent-colors .swatch[title="#1d2129"]')
            .count()) === 1,
        "recent color history recorded the committed brush color",
    );

    const wheelBox = await page.locator("#color-wheel").boundingBox();
    const wheelCenterX = wheelBox.x + wheelBox.width / 2;
    const wheelCenterY = wheelBox.y + wheelBox.height / 2;
    await page.mouse.click(
        wheelCenterX + wheelBox.width * 0.42,
        wheelCenterY,
    );
    await page.mouse.click(
        wheelCenterX + wheelBox.width * 0.24,
        wheelCenterY - wheelBox.height * 0.24,
    );
    check(
        (await page.locator('input[type="color"]').inputValue()) === "#ff0000",
        "color wheel ring and field pick pure red",
    );

    // Sampling a point the pointer itself passed through keeps this check
    // independent of the current pan and zoom.
    await page.locator("#tool-eyedropper").click();
    const pickTarget = strokePath[Math.floor(strokePath.length / 2)];
    await page.mouse.click(pickTarget.x, pickTarget.y);
    check(
        (await page.locator('input[type="color"]').inputValue()) === "#1d2129",
        "eyedropper picked the stroke color from the canvas",
    );

    const zoomBefore = await page.locator("#zoom-fit").textContent();
    await page.locator(".zoom-controls button:last-child").click();
    const zoomAfter = await page.locator("#zoom-fit").textContent();
    check(
        zoomBefore !== zoomAfter,
        `zoom in changed the scale (${zoomBefore.trim()} -> ` +
            `${zoomAfter.trim()})`,
    );
    await page.locator("#zoom-fit").click();
    check(
        (await page.locator("#zoom-fit").textContent()) === zoomBefore,
        "zoom to fit restored the initial scale",
    );

    // The stroke has to stay put in document space across a zoom, which is
    // what a wrong presenter transform would break.
    await page.keyboard.press("Control+Equal");
    const zoomedPixels = await countBrushPixels(page);
    check(
        zoomedPixels === drawnPixels,
        `zooming does not change document pixels (${zoomedPixels} px)`,
    );
    await page.keyboard.press("Control+Digit0");

    await page.keyboard.press("e");
    check(
        (await page.locator("#eraser-preset").count()) === 1,
        "the E shortcut selects the eraser and reveals its presets",
    );
    check(
        (await page.locator("#eraser-preset option").count()) > 0,
        "eraser presets come from the engine catalog",
    );
    await page.keyboard.press("b");

    // The restored document carries the stroke as content with no history, so
    // undo needs a stroke drawn in this session to work on.
    const beforeSecondStroke = await countBrushPixels(page);
    await page.mouse.move(pickTarget.x, pickTarget.y + 60);
    await page.mouse.down();
    for (let step = 1; step <= 10; step += 1) {
        await page.mouse.move(pickTarget.x + step * 6, pickTarget.y + 60);
    }
    await page.mouse.up();
    await page.waitForFunction(
        (before) => window.__uguruguBrushCount?.() > before,
        beforeSecondStroke,
        { timeout: 20000 },
    );
    await page.keyboard.press("Control+z");
    await page.waitForFunction(
        (expected) => window.__uguruguBrushCount?.() === expected,
        beforeSecondStroke,
        { timeout: 20000 },
    );
    check(true, "the Ctrl+Z shortcut undid the stroke");

    // BrushSettings::antialiasing defaults off and no preset sets it, so
    // without this toggle the shell could only ever commit aliased strokes.
    // Aliased edges also leave hairline seams where wobbled segments meet.
    const alias = (checked) =>
        page.evaluate((want) => {
            const box = document.querySelector("#brush-antialiasing");
            if (box.checked !== want) {
                box.click();
            }
            return box.checked;
        }, checked);
    check((await alias(true)) === true, "antialiasing can be turned on");
    const smoothEdgesBefore = await page.evaluate(() => {
        const canvas = document.querySelector("#document-surface");
        const data = canvas
            .getContext("2d")
            .getImageData(0, 0, canvas.width, canvas.height).data;
        let partial = 0;
        for (let index = 0; index < data.length; index += 4) {
            const value = data[index];
            if (value > 60 && value < 200) {
                partial += 1;
            }
        }
        return partial;
    });
    await drawStroke(page);
    const smoothEdgesAfter = await page.evaluate(() => {
        const canvas = document.querySelector("#document-surface");
        const data = canvas
            .getContext("2d")
            .getImageData(0, 0, canvas.width, canvas.height).data;
        let partial = 0;
        for (let index = 0; index < data.length; index += 4) {
            const value = data[index];
            if (value > 60 && value < 200) {
                partial += 1;
            }
        }
        return partial;
    });
    check(
        smoothEdgesAfter > smoothEdgesBefore,
        `antialiased stroke added ${smoothEdgesAfter - smoothEdgesBefore} ` +
            `partially covered edge pixels`,
    );

    // Preset selection replaces the engine's brush template wholesale, which
    // used to silently drop the toggle: cycling presets or visiting the
    // eraser left the box checked but committed aliased strokes.
    const smoothEdgeCensus = () => {
        const canvas = document.querySelector("#document-surface");
        const data = canvas
            .getContext("2d")
            .getImageData(0, 0, canvas.width, canvas.height).data;
        let partial = 0;
        for (let index = 0; index < data.length; index += 4) {
            const value = data[index];
            if (value > 60 && value < 200) {
                partial += 1;
            }
        }
        return partial;
    };
    const drawOffsetStroke = async (offsetY) => {
        const box = await page.locator("#display-canvas").boundingBox();
        const startX = box.x + box.width / 2 - 80;
        const startY = box.y + box.height / 2 + offsetY;
        await page.mouse.move(startX, startY);
        await page.mouse.down();
        for (let step = 1; step <= 12; step += 1) {
            await page.mouse.move(startX + step * 7, startY + step * 2.5);
        }
        await page.mouse.up();
    };
    const expectSmootherAfter = async (baseline, label) => {
        await page.waitForFunction(
            (edges) => {
                const canvas = document.querySelector("#document-surface");
                const data = canvas
                    .getContext("2d")
                    .getImageData(0, 0, canvas.width, canvas.height).data;
                let partial = 0;
                for (let index = 0; index < data.length; index += 4) {
                    const value = data[index];
                    if (value > 60 && value < 200) {
                        partial += 1;
                    }
                }
                return partial > edges;
            },
            baseline,
            { timeout: 15000 },
        );
        check(true, label);
    };
    await page.locator("#brush-preset").selectOption("1");
    await page.locator("#brush-preset").selectOption("0");
    const beforePresetCycle = await page.evaluate(smoothEdgeCensus);
    await drawOffsetStroke(40);
    await expectSmootherAfter(
        beforePresetCycle,
        "antialiasing survives a brush preset cycle",
    );
    await page.locator("#tool-eraser").click();
    await page.locator("#tool-brush").click();
    const beforeEraserRoundTrip = await page.evaluate(smoothEdgeCensus);
    await drawOffsetStroke(80);
    await expectSmootherAfter(
        beforeEraserRoundTrip,
        "antialiasing survives an eraser round-trip",
    );
    await page.keyboard.press("Control+z");
    await alias(false);

    // addStroke refuses a stroke when any point falls outside the canvas, so
    // a drag that leaves the viewport used to lose the whole line.
    const beforeExit = await countBrushPixels(page);
    const canvasBox = await page.locator("#display-canvas").boundingBox();
    const startX = canvasBox.x + canvasBox.width / 2;
    const startY = canvasBox.y + canvasBox.height / 2 + 60;
    await page.mouse.move(startX, startY);
    await page.mouse.down();
    for (let step = 1; step <= 8; step += 1) {
        // Runs well past the right edge of the document and back inside.
        await page.mouse.move(startX + step * 90, startY + step * 4);
    }
    await page.mouse.move(startX + 40, startY + 30);
    await page.mouse.up();
    await page.waitForFunction(
        (before) => window.__uguruguBrushCount?.() > before,
        beforeExit,
        { timeout: 20000 },
    );
    check(true, "a stroke that leaves the canvas still commits");
    check(
        !(await page.locator("#status").textContent())?.includes("Error"),
        "leaving the canvas reports no error",
    );
    await page.keyboard.press("Control+z");

    // Drawing pauses the wobble but must not switch playback off, or the user
    // has to restart it after every stroke. CanvasWidget::advanceFrame does
    // the same by skipping frames instead of clearing m_animating.
    await page.locator(".play").click();
    check(await isPlaying(page), "playback started");
    await drawStroke(page);
    check(await isPlaying(page), "playback survives a stroke");

    // Rendering a playback frame mid-stroke used to wipe the live stroke
    // and replace the image the incremental compositor patches into, which
    // flashed garbage until the pointer was released.
    await page.locator("#animate-while-drawing").check();
    const brushCountBeforeWobbleDraw = await page.evaluate(() =>
        window.__uguruguBrushCount?.(),
    );
    // Clear of the stroke above, so the commit adds a whole line rather than
    // the sliver two wobbles of the same path happen to disagree on. Playback
    // shares the request queue, so the commit can land several frames after
    // the pointer went up and has to be waited for rather than sampled.
    await drawOffsetStroke(-40);
    await page.waitForFunction(
        (before) => window.__uguruguBrushCount?.() > before,
        brushCountBeforeWobbleDraw,
        { timeout: 20000 },
    );
    check(true, "a stroke drawn while wobbling commits");
    check(
        !(await page.locator("#status").textContent())?.includes("Error"),
        "wobble-while-drawing reports no error",
    );
    check(await isPlaying(page), "playback survives a wobbling stroke");
    await page.locator("#animate-while-drawing").uncheck();
    await page.locator(".play").click();
    await context.close();
}
