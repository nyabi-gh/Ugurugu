// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

// Checks that every workspace crate depends only on the workspace crates
// allowed by docs/RUST_PORT_PLAN.md section 3.1. Crates missing from the table
// are rejected so a new crate has to declare its place in the layering.

import { execFileSync } from "node:child_process";
import process from "node:process";

const allowed = {
    "ugu-base": [],
    "ugu-model": ["ugu-base"],
    "ugu-motion": ["ugu-model", "ugu-base"],
    "ugu-raster": ["ugu-base"],
    "ugu-render": ["ugu-motion", "ugu-raster", "ugu-model", "ugu-base"],
    "ugu-format": ["ugu-model", "ugu-base"],
    "ugu-edit": ["ugu-render", "ugu-motion", "ugu-model", "ugu-base"],
    "ugu-export": ["ugu-render", "ugu-model", "ugu-base"],
    "ugu-session": ["ugu-edit", "ugu-export", "ugu-format", "ugu-render", "ugu-motion", "ugu-model", "ugu-base"],
    "ugu-gpu": ["ugu-base"],
    "ugu-platform": ["ugu-base"],
    "ugu-ui": ["ugu-session", "ugu-gpu", "ugu-platform", "ugu-base"],
    "ugurugu": ["ugu-ui", "ugu-session", "ugu-gpu", "ugu-platform"],
    "ugu-wasm": ["ugu-session"],
    "ugu-reference": "any",
};

const metadata = JSON.parse(
    execFileSync("cargo", ["metadata", "--format-version", "1", "--no-deps", "--locked"], {
        encoding: "utf8",
        maxBuffer: 64 * 1024 * 1024,
    }),
);

const workspaceCrates = new Set(metadata.packages.map((crate) => crate.name));
const problems = [];

for (const crate of metadata.packages) {
    const rule = allowed[crate.name];
    if (rule === undefined) {
        problems.push(`${crate.name} is not in the dependency table`);
        continue;
    }
    for (const dependency of crate.dependencies) {
        if (!workspaceCrates.has(dependency.name) || rule === "any") {
            continue;
        }
        if (!rule.includes(dependency.name)) {
            problems.push(`${crate.name} may not depend on ${dependency.name}`);
        }
    }
}

if (problems.length !== 0) {
    console.error("Crate dependency rules violated:");
    for (const problem of problems) {
        console.error(`  ${problem}`);
    }
    process.exit(1);
}
console.log(`${metadata.packages.length} workspace crates follow the dependency table`);
