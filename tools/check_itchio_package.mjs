// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

// Checks a built web shell against the itch.io HTML5 limits documented in
// docs/web-itchio-feasibility.md section 4: an index.html entry point, no
// absolute asset paths, at most 1000 files, 240-character paths, 500 MB total
// and 200 MB per file after extraction. Run it on the Vite output directory.

import { readdir, readFile, stat } from "node:fs/promises";
import { join, relative, sep } from "node:path";
import process from "node:process";

const limits = {
    fileCount: 1000,
    pathLength: 240,
    totalBytes: 500 * 1000 * 1000,
    fileBytes: 200 * 1000 * 1000,
};

async function collect(root, directory = root, found = []) {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
        const absolute = join(directory, entry.name);
        if (entry.isDirectory()) {
            await collect(root, absolute, found);
            continue;
        }
        const info = await stat(absolute);
        found.push({
            path: relative(root, absolute).split(sep).join("/"),
            absolute,
            bytes: info.size,
        });
    }
    return found;
}

// Matches src/href/url() references that resolve against the CDN root instead
// of the project's own subdirectory, which is the failure itch.io hits first.
const absoluteReference =
    /(?:src|href)\s*=\s*["']\/(?!\/)|url\(\s*["']?\/(?!\/)/g;

async function main() {
    const root = process.argv[2];
    if (!root) {
        console.error("usage: check_itchio_package.mjs <directory>");
        process.exit(2);
    }
    const files = await collect(root);
    const problems = [];

    if (!files.some((file) => file.path === "index.html")) {
        problems.push("index.html is missing from the package root");
    }
    if (files.length > limits.fileCount) {
        problems.push(
            `${files.length} files exceeds the ${limits.fileCount} file limit`,
        );
    }
    const total = files.reduce((sum, file) => sum + file.bytes, 0);
    if (total > limits.totalBytes) {
        problems.push(
            `${total} bytes exceeds the ${limits.totalBytes} byte total limit`,
        );
    }
    for (const file of files) {
        if (file.path.length > limits.pathLength) {
            problems.push(`path longer than ${limits.pathLength}: ${file.path}`);
        }
        if (file.bytes > limits.fileBytes) {
            problems.push(
                `${file.path} is ${file.bytes} bytes, over the ` +
                    `${limits.fileBytes} byte single-file limit`,
            );
        }
        // Case-insensitive duplicates load on macOS and break on itch.io's
        // case-sensitive storage.
        const clashes = files.filter(
            (other) =>
                other !== file &&
                other.path.toLowerCase() === file.path.toLowerCase(),
        );
        if (clashes.length > 0) {
            problems.push(`case-insensitive duplicate path: ${file.path}`);
        }
    }

    for (const file of files) {
        if (!/\.(html|css|js|mjs)$/.test(file.path)) {
            continue;
        }
        const text = await readFile(file.absolute, "utf8");
        const matches = text.match(absoluteReference);
        if (matches) {
            problems.push(
                `${file.path} references ${matches.length} absolute path(s); ` +
                    `itch.io serves the game from a subdirectory`,
            );
        }
    }

    console.log(
        `${files.length} files, ${(total / (1024 * 1024)).toFixed(2)} MiB ` +
            `uncompressed`,
    );
    for (const file of [...files].sort((a, b) => b.bytes - a.bytes).slice(0, 5)) {
        console.log(
            `  ${(file.bytes / (1024 * 1024)).toFixed(2)} MiB  ${file.path}`,
        );
    }
    if (problems.length > 0) {
        console.error("itch.io package check failed:");
        for (const problem of problems) {
            console.error(`  - ${problem}`);
        }
        process.exit(1);
    }
    console.log("itch.io package check passed");
}

await main();
