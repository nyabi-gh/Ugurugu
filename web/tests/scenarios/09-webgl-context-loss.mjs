// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import {
    check,
    installPixelCounter,
    opaquePixelCount,
    screenPixel,
    setCanvasRotation,
    waitForDocumentLoaded,
} from "../harness.mjs";

// losing the WebGL context must fall back to a canvas that can
// actually take a 2D context, not to the same element.
export default async function run({ browser, origin }) {
    const context = await browser.newContext();
    const page = await context.newPage();
    await installPixelCounter(page);
    await page.goto(origin);
    await waitForDocumentLoaded(page);
    const presenterStatus = await page
        .locator("#presenter-status")
        .textContent();
    if (presenterStatus?.includes("WebGL 2")) {
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
        await page.evaluate(() => {
            const gl = document
                .querySelector("#display-canvas")
                .getContext("webgl2");
            gl?.getExtension("WEBGL_lose_context")?.loseContext();
        });
        await page.waitForSelector("#display-software", { timeout: 20000 });
        check(true, "a context loss creates the software display canvas");
        await page.waitForFunction(
            () =>
                document
                    .querySelector("#presenter-status")
                    ?.textContent.includes("Canvas 2D"),
            undefined,
            { timeout: 20000 },
        );
        check(true, "the status bar stops claiming WebGL after the loss");
        check(
            (await opaquePixelCount(page, "#display-software")) > 0,
            "the software display still shows the document",
        );
        const box = await page.locator("#display-canvas").boundingBox();
        const center = { x: box.x + box.width / 2, y: box.y + box.height / 2 };
        const centerPixel = await screenPixel(page, center.x, center.y);
        const outsidePixel = await screenPixel(page, center.x + 150, center.y);
        check(
            centerPixel[0] > 235 &&
                centerPixel[1] > 235 &&
                centerPixel[2] > 235,
            "the software fallback keeps the rotated document at its centre",
        );
        check(
            outsidePixel[0] < 120 &&
                outsidePixel[1] < 120 &&
                outsidePixel[2] < 120,
            "the software fallback clips outside the rotated document",
        );
    } else {
        console.log("skip: this browser has no WebGL 2 to lose");
    }
    await context.close();
}
