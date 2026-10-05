// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import { fileURLToPath } from "node:url";
import {
    check,
    countBrushPixels,
    drawStroke,
    eventually,
    installPixelCounter,
    installRecoveryDelay,
    waitForDocumentLoaded,
} from "../harness.mjs";

const wave = fileURLToPath(new URL("../../../examples/Wave.ugu", import.meta.url));

async function newDocument(page, width, height) {
    await page.locator("#new-document").click();
    await page.locator("#new-document-width").fill(String(width));
    await page.locator("#new-document-height").fill(String(height));
    await page.locator("#new-document-confirm").click();
}

function surfaceSize(page) {
    return page.evaluate(() => {
        const canvas = document.querySelector("#document-surface");
        return { width: canvas.width, height: canvas.height };
    });
}

// Replacing a document the artist has changed since the last download needs
// their say-so, and a recovery snapshot of the document they left must not
// stand in for the one they are working on now.
export default async function run({ browser, origin }) {
    {
        const context = await browser.newContext();
        const page = await context.newPage();
        await installPixelCounter(page);
        const dialogs = [];
        let answer = false;
        page.on("dialog", (dialog) => {
            dialogs.push(dialog.type());
            void (answer ? dialog.accept() : dialog.dismiss());
        });
        await page.goto(`${origin}/`);
        await waitForDocumentLoaded(page);
        const before = await surfaceSize(page);
        await drawStroke(page);
        const drawn = await countBrushPixels(page);

        await newDocument(page, 320, 240);
        await page.waitForTimeout(1000);
        check(dialogs.includes("confirm"), "a new document over unsaved work asks first");
        const kept = await surfaceSize(page);
        check(
            kept.width === before.width &&
                kept.height === before.height &&
                (await countBrushPixels(page)) === drawn,
            "declining keeps the document",
        );

        dialogs.length = 0;
        await page.locator("#open-document").setInputFiles(wave);
        await page.waitForTimeout(1000);
        check(dialogs.includes("confirm"), "opening a file over unsaved work asks first");
        check(
            (await countBrushPixels(page)) === drawn,
            "declining the open keeps the document",
        );

        answer = true;
        await newDocument(page, 320, 240);
        check(
            await eventually(
                page,
                () => document.querySelector("#document-surface")?.height === 240,
            ),
            "accepting replaces the document",
        );

        dialogs.length = 0;
        await drawStroke(page);
        // The stroke counts once the engine has committed it.
        await page.waitForFunction(() => !document.querySelector("#undo")?.disabled);
        await page.close({ runBeforeUnload: true });
        await new Promise((resolve) => setTimeout(resolve, 1000));
        check(
            dialogs.includes("beforeunload"),
            "leaving the page with unsaved work asks first",
        );
        await context.close();
    }

    {
        const context = await browser.newContext();
        const page = await context.newPage();
        await installPixelCounter(page);
        await installRecoveryDelay(page);
        page.on("dialog", (dialog) => void dialog.accept());
        await page.goto(`${origin}/?autosave=1`);
        await waitForDocumentLoaded(page);
        await drawStroke(page);
        // Catch the first document's snapshot write in flight, then replace
        // the document and draw on the new one before it lands.
        const opens = await page.evaluate(() => {
            window.__slowRecovery = 4000;
            return window.__recoveryOpens;
        });
        await page.waitForFunction((count) => window.__recoveryOpens > count, opens, {
            timeout: 10000,
        });
        await newDocument(page, 320, 240);
        await page.waitForFunction(
            () => document.querySelector("#document-surface")?.height === 240,
            undefined,
            { timeout: 30000 },
        );
        await drawStroke(page);
        await page.evaluate(() => {
            window.__slowRecovery = 0;
        });
        await page.waitForTimeout(7000);

        await page.reload();
        await page.locator("#recovery-restore").waitFor({ timeout: 30000 });
        await page.locator("#recovery-restore").click();
        check(
            await eventually(
                page,
                () => document.querySelector("#document-surface")?.height === 240,
            ),
            "the snapshot offered after a document swap is the new document",
        );
        check(
            await eventually(page, () => window.__uguruguBrushCount?.() > 0),
            "the new document's stroke was saved for recovery",
        );
        await context.close();
    }
}
