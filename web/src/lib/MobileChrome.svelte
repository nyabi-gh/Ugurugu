<!--
SPDX-License-Identifier: GPL-3.0-or-later
Copyright (C) 2026 Nyabi (nyabi-gh)
-->
<script lang="ts">
    // The desktop layout gives every panel a column, which on a phone left the
    // canvas 55% of a screen that already carries a browser's own chrome. Here
    // the panels ride in sheets a dock raises on demand, the arrangement phone
    // painting apps settle on. It floats over the canvas rather than wrapping
    // it: the viewport keeps its place in App.svelte's tree, so crossing the
    // breakpoint never tears down the presenter's canvas.
    import type { Snippet } from "svelte";
    import Sheet from "./Sheet.svelte";
    import ToolIcon from "./ToolIcon.svelte";
    import { toolDefinition } from "./tools";
    import type { ToolId } from "./tools";

    interface Props {
        tool: ToolId;
        color: string;
        playing: boolean;
        hasDocument: boolean;
        embedded: boolean;
        ontoggleplay: () => void;
        toolRail: Snippet;
        toolOptions: Snippet;
        wobblePanel: Snippet;
        colorPanel: Snippet;
        layerPanel: Snippet;
        fileActions: Snippet;
        aboutAction: Snippet;
        historyActions: Snippet;
        timelineControls: Snippet;
        statusBar: Snippet;
    }

    const {
        tool,
        color,
        playing,
        hasDocument,
        embedded,
        ontoggleplay,
        toolRail,
        toolOptions,
        wobblePanel,
        colorPanel,
        layerPanel,
        fileActions,
        aboutAction,
        historyActions,
        timelineControls,
        statusBar,
    }: Props = $props();

    type SheetId = "tool" | "color" | "layers" | "wobble" | "frames" | "file";

    let openSheet: SheetId | null = $state(null);

    const activeTool = $derived(toolDefinition(tool));

    const titles: Record<SheetId, string> = {
        tool: "Tool",
        color: "Color",
        layers: "Layers",
        wobble: "Wobble",
        frames: "Frames",
        file: "File",
    };

    function toggle(id: SheetId) {
        openSheet = openSheet === id ? null : id;
    }
</script>

<div class="floating top-start">
    {@render historyActions()}
</div>

<div class="floating top-end">
    <button
        id="file-menu"
        class="pill"
        aria-label="File menu"
        aria-expanded={openSheet === "file"}
        onclick={() => toggle("file")}
    >
        ⋯
    </button>
</div>

<div class="toast">
    {@render statusBar()}
</div>

<nav class="dock" class:embedded aria-label="Panels">
    <button
        id="dock-tool"
        class:active={openSheet === "tool"}
        onclick={() => toggle("tool")}
    >
        <ToolIcon name={activeTool.id} size={20} />
        <span>{activeTool.label}</span>
    </button>
    <button
        id="dock-color"
        class:active={openSheet === "color"}
        onclick={() => toggle("color")}
    >
        <span class="swatch" style="background: {color}"></span>
        <span>Color</span>
    </button>
    <button
        id="dock-layers"
        class:active={openSheet === "layers"}
        onclick={() => toggle("layers")}
    >
        <span class="glyph" aria-hidden="true">▤</span>
        <span>Layers</span>
    </button>
    <button
        id="dock-wobble"
        class:active={openSheet === "wobble"}
        disabled={!hasDocument}
        onclick={() => toggle("wobble")}
    >
        <span class="glyph" aria-hidden="true">∿</span>
        <span>Wobble</span>
    </button>
    <button
        id="dock-frames"
        class:active={openSheet === "frames"}
        disabled={!hasDocument}
        onclick={() => toggle("frames")}
    >
        <span class="glyph" aria-hidden="true">⧉</span>
        <span>Frames</span>
    </button>
    <button id="dock-play" disabled={!hasDocument} onclick={ontoggleplay}>
        <ToolIcon name={playing ? "pause" : "play"} size={20} />
        <span>{playing ? "Stop" : "Play"}</span>
    </button>
</nav>

{#if openSheet}
    <Sheet
        title={titles[openSheet]}
        selfTitled={openSheet !== "file" && openSheet !== "frames"}
        onclose={() => (openSheet = null)}
    >
        {#if openSheet === "tool"}
            <div class="tool-sheet">
                {@render toolRail()}
                <div class="tool-sheet-options">
                    {@render toolOptions()}
                </div>
            </div>
        {:else if openSheet === "color"}
            {@render colorPanel()}
        {:else if openSheet === "layers"}
            {@render layerPanel()}
        {:else if openSheet === "wobble"}
            {@render wobblePanel()}
        {:else if openSheet === "frames"}
            <div class="frames-sheet">
                {@render timelineControls()}
            </div>
        {:else}
            <!--
              Every control in here either opens a dialog or starts a download,
              so the sheet has done its job the moment one is used. Leaving it
              up would keep the dock covered behind whatever it opened.
            -->
            <div
                class="file-sheet"
                role="presentation"
                onclick={(event) => {
                    if (
                        (event.target as HTMLElement).closest("button, label")
                    ) {
                        openSheet = null;
                    }
                }}
            >
                {@render fileActions()}
                {@render aboutAction()}
            </div>
        {/if}
    </Sheet>
{/if}

<style>
    .floating {
        position: fixed;
        inset-block-start: 0.5rem;
        z-index: 5;
        display: flex;
        gap: 0.3rem;
        padding: 0.2rem;
        border-radius: 0.6rem;
        background: rgb(27 29 33 / 82%);
    }

    .top-start {
        inset-inline-start: 0.5rem;
    }

    .top-end {
        inset-inline-end: 0.5rem;
    }

    .pill {
        min-inline-size: 2.5rem;
        min-block-size: 2.5rem;
        border: 0;
        border-radius: 0.45rem;
        background: none;
        color: var(--paper);
        font-size: 1.1rem;
        line-height: 1;
        cursor: pointer;
    }

    .toast {
        position: fixed;
        inset-inline: 0.5rem;
        inset-block-end: calc(var(--dock-block-size) + 0.4rem);
        z-index: 4;
        pointer-events: none;
    }

    .toast :global(.status-bar) {
        gap: 0.6rem;
        padding: 0.3rem 0.55rem;
        border: 0;
        border-radius: 0.4rem;
        background: rgb(27 29 33 / 82%);
    }

    /* Which tool and which presenter are diagnostics; on a phone they are not
       worth a line of the canvas. Kept in the DOM for the browser suite and
       for anything reading the page. */
    .toast :global(#presenter-status) {
        position: absolute;
        inline-size: 1px;
        block-size: 1px;
        overflow: hidden;
        clip-path: inset(50%);
    }

    /* Above the sheet, not under it: raising a panel should not cost the dock,
       or switching panels would take a dismiss and a tap instead of a tap. */
    .dock {
        position: fixed;
        inset-inline: 0;
        inset-block-end: 0;
        z-index: 10;
        box-sizing: border-box;
        block-size: var(--dock-block-size);
        display: flex;
        border-block-start: 1px solid var(--line);
        background: var(--ink-900);
        padding-block-end: env(safe-area-inset-bottom, 0);
    }

    .dock button {
        flex: 1;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 0.15rem;
        min-inline-size: 0;
        padding: 0.3rem 0.1rem;
        border: 0;
        background: none;
        color: var(--paper-dim);
        font-size: 0.5625rem;
        font-weight: 700;
        letter-spacing: 0.06em;
        text-transform: uppercase;
        cursor: pointer;
    }

    /* itch.io draws its fullscreen button over this corner, and the corner is
       where the dock ends. Only an embedded page pays for the clearance. */
    .dock.embedded {
        padding-inline-end: 3rem;
    }

    .dock button.active {
        color: var(--accent);
        background: rgb(255 201 74 / 12%);
    }

    .dock button:disabled {
        opacity: 0.4;
    }

    .glyph {
        font-size: 1.05rem;
        line-height: 1.15;
    }

    .swatch {
        inline-size: 1.05rem;
        block-size: 1.05rem;
        border: 1px solid var(--line);
        border-radius: 0.2rem;
    }

    .tool-sheet {
        display: flex;
        align-items: flex-start;
        gap: 0.5rem;
    }

    .tool-sheet-options {
        flex: 1;
        min-inline-size: 0;
    }

    .file-sheet {
        display: flex;
        flex-wrap: wrap;
        gap: 0.4rem;
        padding: 0 0.9rem 0.9rem;
    }

    .frames-sheet {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0.7rem;
        padding: 0 0.9rem 0.9rem;
    }

    .frames-sheet :global(input[type="range"]) {
        flex: 1 1 8rem;
        accent-color: var(--accent);
    }
</style>
