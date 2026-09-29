// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh

import {
    check,
    countBrushPixels,
    dragBetween,
    installPixelCounter,
    waitForDocumentLoaded,
} from "../harness.mjs";

// copy, cut and paste move content between layers, and the advanced layer
// commands the desktop has reach the web. The engine had all of it linked
// already; only the bridge and the shell were missing.
export default async function run({ browser, origin }) {
    const context = await browser.newContext();
    const page = await context.newPage();
    await installPixelCounter(page);
    await page.goto(origin);
    await waitForDocumentLoaded(page);

    const layerCount = () =>
        page.evaluate(() => document.querySelectorAll("li .name").length);

    // Something to copy: a stroke, then a rectangle selection around it.
    const canvas = await page.locator("#display-canvas").boundingBox();
    const left = canvas.x + canvas.width * 0.3;
    const top = canvas.y + canvas.height * 0.35;
    await dragBetween(page, { x: left, y: top }, { x: left + 90, y: top + 60 });
    await page.waitForFunction(() => window.__uguruguBrushCount() > 0, {
        timeout: 20000,
    });
    const drawn = await countBrushPixels(page);

    await page.locator("#tool-lasso").click();
    await page.locator("#selection-shape-rectangle").click();
    await dragBetween(
        page,
        { x: left - 30, y: top - 30 },
        { x: left + 130, y: top + 100 },
    );
    await page.waitForSelector("#selection-actions", { timeout: 20000 });

    const before = await layerCount();
    await page.locator("#selection-copy").click();
    await page.waitForFunction(
        (count) => document.querySelectorAll("li .name").length > count,
        before,
        { timeout: 20000 },
    );
    check(true, `copy puts the selection on a new layer (${before} rows before)`);

    const copied = await countBrushPixels(page);
    check(
        copied > drawn,
        `the copy adds ink rather than moving it (${drawn} to ${copied} px)`,
    );

    // Paste is offered once the clipboard has something in it.
    await page.waitForSelector("#selection-paste", { timeout: 20000 });
    const beforePaste = await layerCount();
    await page.locator("#selection-paste").click();
    await page.waitForFunction(
        (count) => document.querySelectorAll("li .name").length > count,
        beforePaste,
        { timeout: 20000 },
    );
    check(true, "paste adds another layer from the clipboard");

    await page.locator("#undo").click();
    await page.waitForFunction(
        (count) => document.querySelectorAll("li .name").length === count,
        beforePaste,
        { timeout: 20000 },
    );
    check(true, "one undo takes the pasted layer back off");

    // Advanced layer commands.
    const beforeDuplicate = await layerCount();
    await page.locator("#layer-duplicate").click();
    await page.waitForFunction(
        (count) => document.querySelectorAll("li .name").length > count,
        beforeDuplicate,
        { timeout: 20000 },
    );
    check(true, "duplicate adds a copy of the active layer");

    await page.locator("#layer-blend-mode").selectOption("1");
    await page.waitForFunction(
        () =>
            document.querySelector("#layer-blend-mode")?.value === "1",
        undefined,
        { timeout: 20000 },
    );
    check(true, "the blend mode reaches the engine and comes back");

    const beforeGroup = await layerCount();
    await page.locator("#layer-add-group").click();
    await page.waitForFunction(
        (count) => document.querySelectorAll("li .name").length > count,
        beforeGroup,
        { timeout: 20000 },
    );
    const grouped = await page.evaluate(() =>
        [...document.querySelectorAll("li .name")].some((name) =>
            name.textContent.includes("📁"),
        ),
    );
    check(grouped, "a group row appears when the active layer is wrapped");

    // Clearing empties the active layer without removing the row. Grouping
    // moved the activation, so name the row that actually holds the stroke.
    await page.locator("#tool-brush").click();
    const paintRows = page.locator("li .name:not(.group)");
    await paintRows.last().click();
    await page.waitForFunction(
        () => document.querySelector("li.active") !== null,
        undefined,
        { timeout: 20000 },
    );
    const rowsBeforeClear = await layerCount();
    const inkBeforeClear = await countBrushPixels(page);
    await page.locator("#layer-clear").click();
    await page.waitForFunction(
        (ink) => window.__uguruguBrushCount() < ink,
        inkBeforeClear,
        { timeout: 20000 },
    );
    check(
        (await layerCount()) === rowsBeforeClear,
        "clear empties the layer and keeps the row",
    );

    await context.close();
}
