// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import {
    brushColor,
    check,
    countBrushPixels,
    hasBrushPixel,
    installPixelCounter,
    screenPixel,
    setCanvasRotation,
    waitForDocumentLoaded,
} from "../harness.mjs";

// canvas rotation is a view-only transform. Controls, shortcuts,
// free rotation, wheel input, presentation, and inverse pointer mapping must
// agree without changing the document pixels.
export default async function run({ browser, origin }) {
    const context = await browser.newContext({
        viewport: { width: 1180, height: 820 },
    });
    const page = await context.newPage();
    await installPixelCounter(page);
    await page.goto(origin);
    await waitForDocumentLoaded(page);

    check(
        (await page.locator("#rotation-angle").inputValue()) === "0",
        "canvas rotation starts at 0 degrees",
    );
    await page.locator("#rotate-right").click();
    check(
        (await page.locator("#rotation-angle").inputValue()) === "5",
        "the rotate-right control adds 5 degrees",
    );
    await page.keyboard.press("-");
    check(
        (await page.locator("#rotation-angle").inputValue()) === "0",
        "the minus shortcut rotates left by 5 degrees",
    );
    await page.keyboard.press("Shift+6");
    check(
        (await page.locator("#rotation-angle").inputValue()) === "5",
        "the caret shortcut rotates right by 5 degrees",
    );
    await page.locator("#rotation-reset").click();

    const zoomBeforeShortcut = await page.locator("#zoom-fit").textContent();
    await page.keyboard.press("Control+-");
    check(
        (await page.locator("#rotation-angle").inputValue()) === "0",
        "Ctrl-minus zooms without rotating the canvas",
    );
    check(
        (await page.locator("#zoom-fit").textContent()) !== zoomBeforeShortcut,
        "Ctrl-minus still changes the zoom",
    );
    await page.keyboard.press("Control+0");

    const canvasBox = await page.locator("#display-canvas").boundingBox();
    const center = {
        x: canvasBox.x + canvasBox.width / 2,
        y: canvasBox.y + canvasBox.height / 2,
    };
    await page.keyboard.down("Shift");
    await page.mouse.move(center.x, center.y);
    await page.mouse.wheel(0, -100);
    await page.keyboard.up("Shift");
    check(
        (await page.locator("#rotation-angle").inputValue()) === "5",
        "Shift-wheel rotates the canvas by 5 degrees",
    );
    await page.locator("#rotation-reset").click();

    await page.keyboard.down("Shift");
    await page.keyboard.down("Space");
    await page.mouse.move(center.x, center.y);
    await page.mouse.down();
    await page.mouse.move(center.x + 40, center.y);
    await page.mouse.up();
    await page.keyboard.up("Space");
    await page.keyboard.up("Shift");
    check(
        Number(await page.locator("#rotation-angle").inputValue()) === 20,
        "Shift-Space drag rotates at 0.5 degrees per pixel",
    );
    check(
        (await countBrushPixels(page)) === 0,
        "rotating the canvas does not paint into the document",
    );

    await page.locator("#new-document").click();
    await page.locator("#new-document-width").fill("320");
    await page.locator("#new-document-height").fill("160");
    await page.locator("#new-document-confirm").click();
    await page.waitForFunction(
        () => document.querySelector("#document-surface")?.width === 320,
        undefined,
        { timeout: 30000 },
    );
    await setCanvasRotation(page, 45);
    await page.locator("#zoom-fit").click();
    check(
        (await page.locator("#rotation-angle").inputValue()) === "45",
        "fit-to-window preserves the rotation",
    );

    const smallCanvasBox = await page.locator("#display-canvas").boundingBox();
    const smallCenter = {
        x: smallCanvasBox.x + smallCanvasBox.width / 2,
        y: smallCanvasBox.y + smallCanvasBox.height / 2,
    };
    const centerPixel = await screenPixel(page, smallCenter.x, smallCenter.y);
    const outsideRotatedQuad = await screenPixel(
        page,
        smallCenter.x + 150,
        smallCenter.y,
    );
    check(
        centerPixel[0] > 235 && centerPixel[1] > 235 && centerPixel[2] > 235,
        "the rotated WebGL quad shows the document at its centre",
    );
    check(
        outsideRotatedQuad[0] < 120 &&
            outsideRotatedQuad[1] < 120 &&
            outsideRotatedQuad[2] < 120,
        "the WebGL presenter clips outside the rotated document quad",
    );

    await setCanvasRotation(page, 37);
    const radians = (37 * Math.PI) / 180;
    const target = {
        documentX: 240,
        documentY: 80,
        screenX: smallCenter.x + Math.cos(radians) * 80,
        screenY: smallCenter.y + Math.sin(radians) * 80,
    };
    await page.mouse.click(target.screenX, target.screenY);
    await page.waitForFunction(hasBrushPixel, undefined, { timeout: 15000 });
    const inkCenter = await page.evaluate((color) => {
        const canvas = document.querySelector("#document-surface");
        const data = canvas
            .getContext("2d")
            .getImageData(0, 0, canvas.width, canvas.height).data;
        let totalX = 0;
        let totalY = 0;
        let count = 0;
        for (let y = 0; y < canvas.height; y += 1) {
            for (let x = 0; x < canvas.width; x += 1) {
                const index = (y * canvas.width + x) * 4;
                if (
                    data[index] === color.red &&
                    data[index + 1] === color.green &&
                    data[index + 2] === color.blue
                ) {
                    totalX += x;
                    totalY += y;
                    count += 1;
                }
            }
        }
        return { x: totalX / count, y: totalY / count, count };
    }, brushColor);
    check(
        inkCenter.count > 0 &&
            Math.abs(inkCenter.x - target.documentX) < 8 &&
            Math.abs(inkCenter.y - target.documentY) < 8,
        `rotated pointer mapping lands at document ${inkCenter.x.toFixed(1)},` +
            `${inkCenter.y.toFixed(1)}`,
    );
    await context.close();
}
