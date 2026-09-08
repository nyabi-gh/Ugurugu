import { copyFile, mkdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const here = (path) => fileURLToPath(new URL(path, import.meta.url));

await mkdir(here("public/engine"), { recursive: true });
await copyFile(here("../examples/Wave.ugu"), here("public/engine/Wave.ugu"));

// An itch.io upload is the whole distribution, so the licence texts the
// Notices panel points at have to travel inside it. They come from the
// repository rather than from a toolchain directory: CI builds the shell
// without Emscripten installed, and the texts must ship either way.
await mkdir(here("public/licenses"), { recursive: true });
for (const [source, name] of [
    ["../LICENSE", "GPL-3.0.txt"],
    ["../resources/licenses/LGPL-3.0.txt", "LGPL-3.0.txt"],
    ["../resources/licenses/Svelte-LICENSE.txt", "Svelte-LICENSE.txt"],
    ["../resources/licenses/OpenType-LICENSE.txt", "OpenType-LICENSE.txt"],
    ["../resources/licenses/Emscripten-LICENSE.txt", "Emscripten-LICENSE.txt"],
    ["../resources/fonts/OFL.txt", "Pretendard-OFL.txt"],
    ["../THIRD_PARTY_NOTICES.md", "THIRD_PARTY_NOTICES.txt"],
]) {
    await copyFile(here(source), here(`public/licenses/${name}`));
}

// The wasm engine comes from the wasm-release CMake preset. It is absent in CI,
// which builds the shell only to type-check and bundle it.
try {
    for (const artifact of [
        "ugurugu_engine_spike.js",
        "ugurugu_engine_spike.wasm",
    ]) {
        await copyFile(
            here(`../out/build/wasm-release/${artifact}`),
            here(`public/engine/${artifact}`),
        );
    }
    console.log("engine artifacts synced");
} catch (error) {
    if (error.code !== "ENOENT") {
        throw error;
    }
    console.warn("wasm engine not built; the shell will not load a document");
}
