// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import {
    check,
    drainWorker,
    installWorkerDelay,
    waitForDocumentLoaded,
} from "../harness.mjs";

// layer commands name the layer, not the row. Two deletes issued
// before the first is answered used to remove the row index twice, taking a
// second, unrelated layer with it.
export default async function run({ browser, origin }) {
    const context = await browser.newContext();
    const page = await context.newPage();
    await installWorkerDelay(page);
    await page.goto(origin);
    await waitForDocumentLoaded(page);

    await page.locator("#layer-add").click();
    await page.locator("#layer-add").click();
    await page.waitForFunction(
        () => document.querySelectorAll("aside ul li").length === 3,
        undefined,
        { timeout: 30000 },
    );

    await page.evaluate(() => {
        window.__slowEngine = 400;
    });
    await page.locator("#layer-remove").click();
    await page.locator("#layer-remove").click();
    await drainWorker(page);
    await page.waitForTimeout(250);

    const remaining = await page.evaluate(
        () => document.querySelectorAll("aside ul li").length,
    );
    check(
        remaining === 2,
        `a repeated delete removes one layer, not two (${remaining} left)`,
    );
    await context.close();
}
