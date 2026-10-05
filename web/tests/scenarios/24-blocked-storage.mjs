// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import {
    check,
    countBrushPixels,
    drawStroke,
    eventually,
    installPixelCounter,
} from "../harness.mjs";

// Browsers refuse storage in different places: with site data blocked the
// localStorage getter itself throws, a full or private-mode store throws from
// setItem, and some embeds throw from every call. Preferences are a
// convenience, so none of these may cost the artist the canvas or a save.
const blockers = {
    getter: () => {
        Object.defineProperty(window, "localStorage", {
            configurable: true,
            get() {
                throw new DOMException("storage blocked (simulated)", "SecurityError");
            },
        });
    },
    getItem: () => {
        Storage.prototype.getItem = () => {
            throw new DOMException("storage blocked (simulated)", "SecurityError");
        };
    },
    setItem: () => {
        Storage.prototype.setItem = () => {
            throw new DOMException("storage full (simulated)", "QuotaExceededError");
        };
    },
};

export default async function run({ browser, origin }) {
    for (const [name, blocker] of Object.entries(blockers)) {
        const context = await browser.newContext({ acceptDownloads: true });
        const page = await context.newPage();
        const errors = [];
        page.on("pageerror", (error) => errors.push(error.message));
        await installPixelCounter(page);
        await page.addInitScript(blocker);
        await page.addInitScript(() => {
            IDBFactory.prototype.open = () => {
                throw new DOMException("IndexedDB blocked (simulated)", "SecurityError");
            };
        });
        await page.goto(`${origin}/`);

        check(
            await eventually(
                page,
                () => document.querySelector("#document-surface")?.width > 0,
            ),
            `${name}: the shell opens a document with storage blocked`,
        );
        if (errors.length > 0) {
            check(false, `${name}: uncaught ${errors[0]}`);
            await context.close();
            continue;
        }

        await page.locator("#new-document").click();
        await page.locator("#new-document-width").fill("320");
        await page.locator("#new-document-height").fill("240");
        await page.locator("#new-document-confirm").click();
        check(
            await eventually(
                page,
                () => document.querySelector("#document-surface")?.height === 240,
            ),
            `${name}: a new document can be created`,
        );
        await drawStroke(page);
        check(
            (await countBrushPixels(page)) > 0,
            `${name}: the new document can be drawn on`,
        );

        const download = page.waitForEvent("download", { timeout: 20000 });
        await page.locator("#save-document").click();
        const saved = await download.catch(() => null);
        check(
            saved?.suggestedFilename().endsWith(".ugu") ?? false,
            `${name}: the document can still be downloaded`,
        );
        check(errors.length === 0, `${name}: no uncaught errors (${errors.join("; ")})`);
        await context.close();
    }
}
