// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import {
    check,
    waitForDocumentLoaded,
} from "../harness.mjs";

// the licence notice. The browser build links Qt statically and
// ships nothing beside the page, so this panel is the only route from a running
// app to its licence and its source.
export default async function run({ browser, origin }) {
    const context = await browser.newContext();
    const page = await context.newPage();
    await page.goto(`${origin}/`);
    await waitForDocumentLoaded(page);

    await page.locator("#notices").click();
    await page.locator("#notices-close").waitFor({ timeout: 20000 });
    const panel = await page.locator("#notices-title").textContent();
    check(Boolean(panel), "the About button opens the notices panel");

    const text = await page.locator("[role=dialog]").textContent();
    for (const required of [
        "GNU General Public License",
        "Lesser General Public License",
        "statically",
        "Qt 6.11.1",
        "Pretendard JP",
        "Emscripten",
        "zlib 1.3.2",
    ]) {
        check(
            text?.includes(required),
            `the notice names ${required}`,
        );
    }
    check(
        (await page.locator("#notices-source").getAttribute("href")) ===
            "https://github.com/nyabi-gh/Ugurugu",
        "the notice links to the complete source",
    );

    // The licence texts have to be in the upload, not merely linked to: every
    // one the panel points at is fetched from the server the package is served
    // by.
    const hrefs = await page
        .locator("[role=dialog] a[href*='licenses/']")
        .evaluateAll((links) => links.map((link) => link.href));
    check(hrefs.length >= 7, `the notice links ${hrefs.length} licence texts`);
    for (const href of hrefs) {
        const response = await page.request.get(href);
        check(
            response.ok() && (await response.text()).length > 200,
            `${href.split("/").pop()} is served with the package`,
        );
    }

    await page.keyboard.press("Escape");
    check(
        (await page.locator("#notices-close").count()) === 0,
        "Escape closes the notices panel",
    );
    await context.close();
}
