// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import {
    check,
    countBrushPixels,
    dragBetween,
    installPixelCounter,
    overlayHasInk,
    rightmostInk,
    waitForDocumentLoaded,
} from "../harness.mjs";

// the selection and fill tools — lasso, wand and bucket — plus the
// marching-ants overlay that carries the engine's selection back to the shell.
// A 640x480 document keeps every coordinate below in document space.
export default async function run({ browser, origin }) {
    const width = 640;
    const height = 480;
    const context = await browser.newContext();
    const page = await context.newPage();
    await installPixelCounter(page);
    await page.goto(`${origin}/`);
    await waitForDocumentLoaded(page);

    await page.locator("#new-document").click();
    await page.locator("#new-document-width").fill(String(width));
    await page.locator("#new-document-height").fill(String(height));
    await page.locator("#new-document-confirm").click();
    await page.waitForFunction(
        (expected) =>
            document.querySelector("#document-surface")?.height === expected,
        height,
        { timeout: 30000 },
    );

    const box = await page.locator("#display-canvas").boundingBox();
    // The document is centred in a viewport that is usually larger than it, so
    // screen coordinates have to be derived rather than guessed. Fit never
    // magnifies, and nothing here pans, so the centre stays the document's.
    const zoom =
        Number(
            (await page.locator("#zoom-fit").textContent())
                ?.trim()
                .replace("%", ""),
        ) / 100;
    const at = (documentX, documentY) => ({
        x: box.x + box.width / 2 + (documentX - width / 2) * zoom,
        y: box.y + box.height / 2 + (documentY - height / 2) * zoom,
    });

    await page.locator("#tool-lasso").click();
    await page.locator("#selection-shape-rectangle").click();
    check(
        (await page.locator("#selection-actions").count()) === 0,
        "no selection actions before anything is selected",
    );

    await dragBetween(page, at(250, 190), at(390, 290));
    await page.locator("#selection-actions").waitFor({ timeout: 20000 });
    check(true, "a rectangle lasso drag produced a selection");

    // The ants are drawn from contours the engine traced, so ink on the
    // overlay proves the outline survived the round trip.
    await page.waitForFunction(overlayHasInk, undefined, { timeout: 20000 });
    check(true, "the selection overlay drew marching ants");

    await page.locator("#selection-fill").click();
    await page.waitForFunction(
        () => window.__uguruguBrushCount?.() > 0,
        undefined,
        { timeout: 20000 },
    );
    const filled = await countBrushPixels(page);
    check(
        filled > 13000 && filled <= 140 * 100,
        `filling the 140x100 selection painted ${filled} px inside it`,
    );

    await page.keyboard.press("Control+d");
    await page.waitForFunction(
        () => document.querySelector("#selection-actions") === null,
        undefined,
        { timeout: 20000 },
    );
    check(true, "Ctrl+D deselected");

    // A stroke that starts inside a selection must stop at its edge, which is
    // the clipMask the bridge attaches to every stroke.
    await dragBetween(page, at(200, 140), at(440, 340));
    await page.locator("#selection-actions").waitFor({ timeout: 20000 });
    await page.locator("#tool-brush").click();
    await page.mouse.move(at(320, 240).x, at(320, 240).y);
    await page.mouse.down();
    for (let step = 1; step <= 12; step += 1) {
        const point = at(320 + step * 25, 240);
        await page.mouse.move(point.x, point.y);
    }
    await page.mouse.up();
    await page.waitForFunction(
        (before) => window.__uguruguBrushCount?.() > before,
        filled,
        { timeout: 20000 },
    );
    const clippedRight = await rightmostInk(page);
    check(
        clippedRight > 400 && clippedRight < 460,
        `the stroke stopped at the selection edge (rightmost ink x ` +
            `${clippedRight}, edge 439)`,
    );

    await page.locator("#selection-delete").click();
    await page.waitForFunction(
        () => window.__uguruguBrushCount?.() === 0,
        undefined,
        { timeout: 30000 },
    );
    check(true, "deleting the selection removed everything inside it");
    check(
        (await page.locator("#selection-actions").count()) === 0,
        "deleting the selection also clears it",
    );

    // Nothing is drawn now, so an alpha-boundary flood has no walls to stop
    // at and covers the whole canvas.
    await page.locator("#tool-bucket").click();
    const corner = at(20, 20);
    await page.mouse.click(corner.x, corner.y);
    await page.waitForFunction(
        (expected) => window.__uguruguBrushCount?.() === expected,
        width * height,
        { timeout: 30000 },
    );
    check(true, `the bucket flooded the empty layer (${width * height} px)`);
    check(
        !(await page.locator("#status").textContent())?.includes("code"),
        "the bucket reported no error",
    );

    await page.keyboard.press("Control+z");
    await page.waitForFunction(
        () => window.__uguruguBrushCount?.() === 0,
        undefined,
        { timeout: 30000 },
    );
    check(true, "undo took the bucket fill back");

    // Paint mode commits the lassoed area as a fill instead of selecting it.
    await page.locator("#tool-lasso").click();
    await page.locator("#lasso-mode-paint").click();
    await page.locator("#selection-shape-ellipse").click();
    await dragBetween(page, at(60, 60), at(200, 180));
    await page.waitForFunction(
        () => window.__uguruguBrushCount?.() > 0,
        undefined,
        { timeout: 20000 },
    );
    check(
        (await page.locator("#selection-actions").count()) === 0,
        "lasso paint mode fills without leaving a selection behind",
    );

    // The wand needs a seed the flood can start from, so it goes where no
    // paint has reached.
    await page.locator("#tool-wand").click();
    const empty = at(600, 440);
    await page.mouse.click(empty.x, empty.y);
    await page.locator("#selection-actions").waitFor({ timeout: 20000 });
    check(true, "the wand selected the area around the painted shape");

    // A selection change is an undo entry of its own, the way
    // pushSelectionStateCommand records it on the desktop.
    const paintedBeforeUndo = await countBrushPixels(page);
    await page.keyboard.press("Control+z");
    await page.waitForFunction(
        () => document.querySelector("#selection-actions") === null,
        undefined,
        { timeout: 20000 },
    );
    check(true, "undo takes back the wand's selection");
    check(
        (await countBrushPixels(page)) === paintedBeforeUndo,
        "undoing a selection change leaves the artwork alone",
    );

    await page.keyboard.press("Control+Shift+z");
    await page.locator("#selection-actions").waitFor({ timeout: 20000 });
    check(true, "redo brings the selection back");

    // "Reference layers" is only answerable once a layer is marked as one,
    // which is what ugu_layer_set_reference exists for.
    await page.keyboard.press("Control+d");
    await page.locator("#fill-reference-1").click();
    await page.mouse.click(empty.x, empty.y);
    await page.waitForFunction(
        () =>
            document
                .querySelector("#status")
                ?.textContent.includes("reference"),
        undefined,
        { timeout: 20000 },
    );
    check(
        (await page.locator("#selection-actions").count()) === 0,
        "the wand refuses to read reference layers while none is marked",
    );

    const referenceToggle = page.locator("aside li button.reference").first();
    await referenceToggle.click();
    await page.waitForFunction(
        () =>
            document
                .querySelector("aside li button.reference")
                ?.getAttribute("aria-pressed") === "true",
        undefined,
        { timeout: 20000 },
    );
    check(true, "a layer can be marked as a reference layer");

    await page.mouse.click(empty.x, empty.y);
    await page.locator("#selection-actions").waitFor({ timeout: 20000 });
    check(true, "the wand reads the layer once it is marked");

    // The rail letters are the desktop's, and every one of them has a button.
    for (const [key, id] of [
        ["l", "tool-lasso"],
        ["w", "tool-wand"],
        ["g", "tool-bucket"],
        ["b", "tool-brush"],
    ]) {
        await page.keyboard.press(key);
        check(
            (await page.locator(`#${id}`).getAttribute("aria-pressed")) ===
                "true",
            `the ${key.toUpperCase()} shortcut selects ${id}`,
        );
    }

    await context.close();
}
