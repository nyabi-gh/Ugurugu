// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import {
    check,
    countBrushPixels,
    drawStroke,
    waitForDocumentLoaded,
} from "../harness.mjs";

// a new document honours the web memory policy and can be drawn on.
export default async function run({ browser, origin }) {
    const context = await browser.newContext();
    const page = await context.newPage();
    await page.goto(`${origin}/`);
    await waitForDocumentLoaded(page);

    await page.locator("#new-document").click();
    await page.locator("#new-document-width").fill("640");
    await page.locator("#new-document-height").fill("480");
    await page.locator("#new-document-confirm").click();
    // "Creating a … document" is also a status, so wait for the surface.
    await page.waitForFunction(
        () => document.querySelector("#document-surface")?.height === 480,
        undefined,
        { timeout: 30000 },
    );
    const size = await page.evaluate(() => {
        const canvas = document.querySelector("#document-surface");
        return { width: canvas.width, height: canvas.height };
    });
    check(
        size.width === 640 && size.height === 480,
        `new document surface is ${size.width}x${size.height}`,
    );
    check(
        (await countBrushPixels(page)) === 0,
        "new document starts empty",
    );

    await drawStroke(page);
    const newDocumentPixels = await countBrushPixels(page);
    check(
        newDocumentPixels > 0,
        `stroke committed on the new document (${newDocumentPixels} px)`,
    );

    // The clamp is the policy gate the report asked for; the engine itself
    // would accept up to 4096.
    await page.locator("#new-document").click();
    const maximum = await page
        .locator("#new-document-width")
        .getAttribute("max");
    check(
        Number(maximum) === 2048,
        `new document dialog caps the edge at ${maximum}`,
    );
    await page.locator("#new-document-width").fill("9000");
    await page.locator("#new-document-confirm").click();
    await page.waitForFunction(
        () => document.querySelector("#document-surface")?.width !== 640,
        undefined,
        { timeout: 60000 },
    );
    const clamped = await page.evaluate(
        () => document.querySelector("#document-surface").width,
    );
    check(clamped === 2048, `oversized request clamped to ${clamped}`);
    await context.close();
}
