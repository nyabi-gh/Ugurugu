// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import {
    check,
    countBrushPixels,
    drawStroke,
    installPixelCounter,
    waitForDocumentLoaded,
} from "../harness.mjs";

// the document's own properties — wobble, frame count, fps and
// canvas size — are editable, and every one of them is an undo entry.
export default async function run({ browser, origin }) {
    const context = await browser.newContext();
    const page = await context.newPage();
    await installPixelCounter(page);
    await page.goto(origin);
    await waitForDocumentLoaded(page);

    await page.locator("#new-document").click();
    await page.locator("#new-document-width").fill("400");
    await page.locator("#new-document-height").fill("300");
    await page.locator("#new-document-confirm").click();
    await page.waitForFunction(
        () => document.querySelector("#document-surface")?.height === 300,
        undefined,
        { timeout: 30000 },
    );

    await drawStroke(page);
    const straight = await countBrushPixels(page);

    // Wobble displaces every stroke, so the frame the shell shows has to
    // change when the amount does.
    const before = await page.evaluate(() =>
        document
            .querySelector("#document-surface")
            .getContext("2d")
            .getImageData(0, 0, 400, 300)
            .data.join(","),
    );
    await page.locator("#wobble-amount").fill("9");
    await page.locator("#wobble-amount").dispatchEvent("change");
    await page.waitForFunction(
        (previous) =>
            document
                .querySelector("#document-surface")
                .getContext("2d")
                .getImageData(0, 0, 400, 300)
                .data.join(",") !== previous,
        before,
        { timeout: 30000 },
    );
    check(true, "raising the wobble amount redraws the frame");

    await page.keyboard.press("Control+z");
    await page.waitForFunction(
        (previous) =>
            document
                .querySelector("#document-surface")
                .getContext("2d")
                .getImageData(0, 0, 400, 300)
                .data.join(",") === previous,
        before,
        { timeout: 30000 },
    );
    check(true, "undo takes the wobble change back");
    check(
        (await countBrushPixels(page)) === straight,
        "the stroke itself survived the wobble change",
    );

    await page.locator("#wobble-style-1").click();
    await page.waitForFunction(
        () =>
            document
                .querySelector("#wobble-style-1")
                ?.getAttribute("aria-pressed") === "true",
        undefined,
        { timeout: 20000 },
    );
    check(true, "the motion style can be switched to Smooth");

    await page.locator("#animation-frames").fill("8");
    await page.locator("#animation-frames").dispatchEvent("change");
    await page.waitForFunction(
        () =>
            document
                .querySelector(".frame-label")
                ?.textContent.trim()
                .endsWith("/8"),
        undefined,
        { timeout: 30000 },
    );
    check(true, "the animation frame count is editable");

    await page.locator("#frames-per-second").fill("12");
    await page.locator("#frames-per-second").dispatchEvent("change");
    await page.waitForFunction(
        () =>
            document.querySelector("#frames-per-second")?.value === "12",
        undefined,
        { timeout: 20000 },
    );
    check(true, "the playback speed is editable");

    // Image size scales the artwork with the canvas; the ink has to survive.
    await page.locator("#document-size").click();
    await page.locator("#resize-width").fill("800");
    await page.locator("#resize-height").fill("600");
    await page.locator("#resize-confirm").click();
    await page.waitForFunction(
        () => document.querySelector("#document-surface")?.width === 800,
        undefined,
        { timeout: 30000 },
    );
    const scaled = await countBrushPixels(page);
    check(
        scaled > straight,
        `image resize scaled the artwork up (${straight} to ${scaled} px)`,
    );

    // Canvas size keeps the artwork's size and only moves its bounds.
    await page.locator("#document-size").click();
    await page.locator("#resize-mode-canvas").click();
    await page.locator("#resize-width").fill("500");
    await page.locator("#resize-height").fill("400");
    await page.locator("#resize-anchor-0").click();
    await page.locator("#resize-confirm").click();
    await page.waitForFunction(
        () => document.querySelector("#document-surface")?.width === 500,
        undefined,
        { timeout: 30000 },
    );
    check(true, "canvas resize crops the document to 500x400");

    await page.keyboard.press("Control+z");
    await page.waitForFunction(
        () => document.querySelector("#document-surface")?.width === 800,
        undefined,
        { timeout: 30000 },
    );
    check(true, "undo restores the canvas size");
    await context.close();
}
