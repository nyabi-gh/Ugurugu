// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

// Regenerates the reference scene matrix and this platform's C++ reference
// output under tests/reference: scenes/ from ugurugu_reference_scenes, then
// cpp/<platform>/<scene>/ from ugurugu_reference_export for frames 0, 3, 6
// and 9. Both tools come from a Release build of this checkout.
//
//   node tools/reference_matrix.mjs <tool directory> <platform>

import { execFileSync } from "node:child_process";
import { readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import process from "node:process";

const frames = "0,3,6,9";

const [toolDirectory, platform] = process.argv.slice(2);
if (!toolDirectory || !/^[a-z0-9-]+$/.test(platform ?? "")) {
    console.error("usage: node tools/reference_matrix.mjs <tool directory> <platform>");
    process.exit(2);
}

const executable = (name) =>
    join(toolDirectory, process.platform === "win32" ? `${name}.exe` : name);
const root = join("tests", "reference");
const scenes = join(root, "scenes");
const output = join(root, "cpp", platform);

rmSync(scenes, { recursive: true, force: true });
execFileSync(executable("ugurugu_reference_scenes"), [scenes], { stdio: "inherit" });

const matrix = JSON.parse(readFileSync(join(scenes, "matrix.json"), "utf8"));
rmSync(output, { recursive: true, force: true });
for (const scene of matrix.scenes) {
    execFileSync(
        executable("ugurugu_reference_export"),
        ["all", join(scenes, `${scene.name}.ugu`), "--out", join(output, scene.name), "--frames", frames],
        { stdio: "inherit" },
    );
}
console.log(`${matrix.scenes.length} scenes exported to ${output}`);
