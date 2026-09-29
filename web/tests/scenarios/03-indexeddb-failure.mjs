// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import {
    check,
    drawStroke,
    waitForDocumentLoaded,
} from "../harness.mjs";

// IndexedDB failure must land in the status bar, not crash.
export default async function run({ browser, origin }) {
    const context = await browser.newContext();
    const page = await context.newPage();
    await page.addInitScript(() => {
        IDBFactory.prototype.open = () => {
            throw new DOMException(
                "QuotaExceededError (simulated)",
                "QuotaExceededError",
            );
        };
    });
    await page.goto(`${origin}/?autosave=1`);
    await waitForDocumentLoaded(page);
    await page.waitForFunction(
        () =>
            document
                .querySelector("#autosave-status")
                ?.textContent.includes("Could not read the recovery slot"),
        undefined,
        { timeout: 15000 },
    );
    check(true, "recovery slot read failure surfaced");

    await drawStroke(page);
    await page.waitForFunction(
        () =>
            document
                .querySelector("#autosave-status")
                ?.textContent.includes("Recovery save failed"),
        undefined,
        { timeout: 20000 },
    );
    check(true, "snapshot write failure surfaced in status bar");
    await context.close();
}
