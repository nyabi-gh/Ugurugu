// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import {
    check,
    dragBetween,
    installPixelCounter,
    waitForDocumentLoaded,
} from "../harness.mjs";

// the floating selection transform. Move, scale, rotate and flip
// all accumulate into one pending matrix, so applying costs a single undo entry
// and cancelling costs none. The ink is laid down as a filled rectangle that
// coincides with the selection, which makes "the pixels moved" observable as a
// bounding box that moved.
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
    const bounds = () => page.evaluate(() => window.__uguruguInkBounds());
    // field is a bounding-box edge or one of the derived box sizes; the
    // comparison runs in the page so the poll sees fresh pixels each time.
    const waitForBounds = (field, comparison, expected) =>
        page.waitForFunction(
            ([key, operator, value]) => {
                const box = window.__uguruguInkBounds();
                if (!box || box.count === 0) {
                    return false;
                }
                const actual =
                    key === "boxWidth"
                        ? box.right - box.left
                        : key === "boxHeight"
                          ? box.bottom - box.top
                          : box[key];
                if (operator === ">") {
                    return actual > value;
                }
                if (operator === "<") {
                    return actual < value;
                }
                return actual <= value;
            },
            [field, comparison, expected],
            { timeout: 30000 },
        );

    check(
        (await page.locator("#selection-transform").count()) === 0,
        "no transform bar before anything is selected",
    );

    await page.locator("#tool-lasso").click();
    await page.locator("#selection-shape-rectangle").click();
    await dragBetween(page, at(200, 160), at(340, 260));
    await page.locator("#selection-actions").waitFor({ timeout: 20000 });
    await page.locator("#selection-fill").click();
    await page.waitForFunction(
        () => window.__uguruguBrushCount?.() > 0,
        undefined,
        { timeout: 20000 },
    );
    const source = await bounds();
    check(
        source.count > 13000 && source.left >= 199 && source.left <= 201,
        `the source rectangle is ${source.count} px at x ${source.left}`,
    );
    check(
        (await page.locator("#selection-transform").count()) === 1,
        "a selection brings up the transform bar",
    );
    check(
        await page.locator("#selection-transform-apply").isDisabled(),
        "apply stays disabled until something actually moves",
    );

    await page.locator("#selection-move").click();
    check(
        (await page.locator("#selection-move").getAttribute("aria-pressed")) ===
            "true",
        "the move toggle reports itself pressed",
    );

    await page.keyboard.press("m");
    check(
        (await page.locator("#selection-move").getAttribute("aria-pressed")) ===
            "false",
        "the M shortcut leaves move mode",
    );
    await page.keyboard.press("m");
    check(
        (await page.locator("#selection-move").getAttribute("aria-pressed")) ===
            "true",
        "the M shortcut goes back into move mode",
    );

    // A drag that starts outside the selection is a no-op with a hint, the same
    // answer CanvasWidgetEvents gives in move mode.
    await dragBetween(page, at(60, 60), at(120, 60));
    check(
        (await page.locator("#status").textContent())?.includes(
            "Drag inside the selection",
        ),
        "dragging outside the selection explains itself instead of moving it",
    );
    const untouched = await bounds();
    check(
        untouched.left === source.left && untouched.count === source.count,
        "the outside drag moved nothing",
    );

    await dragBetween(page, at(270, 210), at(370, 210));
    await waitForBounds("left", ">", source.left + 90);
    check(true, "dragging inside the selection previews the moved pixels");
    check(
        !(await page.locator("#selection-transform-apply").isDisabled()),
        "a pending move enables apply",
    );

    await page.keyboard.press("Enter");
    await waitForBounds("left", ">", source.left + 90);
    const moved = await bounds();
    check(
        Math.abs(moved.left - (source.left + 100)) <= 2 &&
            Math.abs(moved.count - source.count) < source.count * 0.02,
        `Enter committed the move (x ${source.left} to ${moved.left}, ` +
            `${moved.count} px)`,
    );
    check(
        await page.locator("#selection-transform-apply").isDisabled(),
        "applying ends the session",
    );

    // One undo entry for the whole gesture, not one per pointer move.
    await page.keyboard.press("Control+z");
    await waitForBounds("left", "<=", source.left + 1);
    const undone = await bounds();
    check(
        Math.abs(undone.left - source.left) <= 1 &&
            Math.abs(undone.count - source.count) < source.count * 0.02,
        `a single undo took the whole move back (x ${undone.left})`,
    );
    await page.keyboard.press("Control+Shift+z");
    await waitForBounds("left", ">", source.left + 90);
    check(true, "redo puts the moved pixels back");

    // Escape restores the pixels and leaves no history behind.
    const beforeCancel = await bounds();
    await dragBetween(page, at(370, 210), at(370, 300));
    await waitForBounds("top", ">", beforeCancel.top + 40);
    await page.keyboard.press("Escape");
    await waitForBounds("top", "<=", beforeCancel.top + 1);
    const cancelled = await bounds();
    check(
        Math.abs(cancelled.top - beforeCancel.top) <= 1 &&
            cancelled.count === beforeCancel.count,
        "Escape put the pixels back where they were",
    );
    check(
        await page.locator("#selection-transform-apply").isDisabled(),
        "cancelling ends the session too",
    );
    await page.keyboard.press("Control+z");
    await waitForBounds("left", "<=", source.left + 1);
    check(
        true,
        "the cancelled move pushed nothing: one undo still reaches the " +
            "original position",
    );
    await page.keyboard.press("Control+Shift+z");
    await waitForBounds("left", ">", source.left + 90);

    // A 140x100 rectangle turned a quarter turn is a 100x140 one.
    await page.locator("#selection-rotate").fill("90");
    await page.locator("#selection-rotate-apply").click();
    await waitForBounds("boxWidth", "<", moved.right - moved.left);
    await page.keyboard.press("Enter");
    await page.waitForFunction(
        () =>
            document.querySelector("#selection-transform-apply")?.disabled ===
            true,
        undefined,
        { timeout: 30000 },
    );
    const turned = await bounds();
    check(
        Math.abs(turned.right - turned.left - 99) <= 3 &&
            Math.abs(turned.bottom - turned.top - 139) <= 3,
        `rotating 90 degrees transposed the box to ` +
            `${turned.right - turned.left + 1}x${turned.bottom - turned.top + 1}`,
    );

    // Scale accumulates onto whatever the box is now.
    await page.locator("#selection-scale").fill("150");
    await page.locator("#selection-scale-apply").click();
    await waitForBounds("boxWidth", ">", turned.right - turned.left + 20);
    await page.locator("#selection-transform-apply").click();
    await page.waitForFunction(
        () =>
            document.querySelector("#selection-transform-apply")?.disabled ===
            true,
        undefined,
        { timeout: 30000 },
    );
    const scaled = await bounds();
    check(
        scaled.count > turned.count * 1.8,
        `scaling to 150 percent grew the content from ${turned.count} to ` +
            `${scaled.count} px`,
    );

    // Flip is the third command on the same session; it only has to become
    // pending and undo cleanly.
    await page.locator("#selection-flip-horizontal").click();
    await page.waitForFunction(
        () =>
            document.querySelector("#selection-transform-apply")?.disabled ===
            false,
        undefined,
        { timeout: 30000 },
    );
    check(true, "flipping the selection opens a pending transform");
    await page.locator("#selection-transform-cancel").click();
    await page.waitForFunction(
        () =>
            document.querySelector("#selection-transform-apply")?.disabled ===
            true,
        undefined,
        { timeout: 30000 },
    );
    check(
        (await bounds()).count === scaled.count,
        "cancelling the flip left the committed pixels alone",
    );

    await context.close();
}
