// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import {
    check,
    dragBetween,
    installPixelCounter,
    waitForDocumentLoaded,
} from "../harness.mjs";

// the engine refuses to paint where the artist cannot see, the way
// CanvasWidget::beginStroke does.
export default async function run({ browser, origin }) {
    const context = await browser.newContext();
    const page = await context.newPage();
    await installPixelCounter(page);
    await page.goto(origin);
    await waitForDocumentLoaded(page);

    await page.locator('aside li input[type="checkbox"]').first().uncheck();
    const box = await page.locator("#display-canvas").boundingBox();
    const middle = { x: box.x + box.width / 2, y: box.y + box.height / 2 };
    await dragBetween(
        page,
        { x: middle.x - 60, y: middle.y },
        { x: middle.x + 60, y: middle.y },
    );
    await page.waitForFunction(
        () =>
            document.querySelector("#status")?.textContent.includes("hidden"),
        undefined,
        { timeout: 20000 },
    );
    check(true, "drawing on a hidden layer is refused with a reason");

    await page.locator('aside li input[type="checkbox"]').first().check();
    await page.waitForFunction(
        () => window.__uguruguBrushCount?.() === 0,
        undefined,
        { timeout: 20000 },
    );
    check(true, "the refused stroke was never committed");

    // The same keypress that deletes a selection must stay quiet when there is
    // nothing selected, so the status bar has to read the same afterwards.
    const statusBefore = await page.locator("#status").textContent();
    await page.keyboard.press("Backspace");
    await page.waitForTimeout(500);
    check(
        (await page.locator("#status").textContent()) === statusBefore,
        "Delete with no selection reports nothing",
    );
    await context.close();
}
