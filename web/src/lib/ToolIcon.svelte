<script lang="ts">
    // Traced from the desktop rail's vector glyphs in src/ui/Icons.cpp, so the
    // web and the app show the same marks for the same tools. Coordinates are
    // the same 24x24 box, lines stroked and fills filled exactly as there.
    let { name, size = 22 }: { name: string; size?: number } = $props();
</script>

<svg
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.6"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
>
    {#if name === "brush"}
        <path d="M18.6 3.6 11.4 10.8" />
        <path
            d="M11.4 10.8 13 12.8 10.6 16.4 6.6 18.8 4.8 17.2 7.6 13.4Z"
            fill="currentColor"
            stroke="none"
        />
    {:else if name === "text"}
        <path d="M5 5h14M12 5v15M8 20h8M5 5v3M19 5v3" />
    {:else if name === "eraser"}
        <path d="M14.6 4.4 19.8 9.6 11.6 17.6 6.4 12.4Z" />
        <path d="M9.2 9.7 14.4 14.9" />
        <path d="M5.2 20.4 15.4 20.4" />
    {:else if name === "lasso"}
        <ellipse cx="12" cy="9.8" rx="7.2" ry="5.4" />
        <path d="M13.6 15 Q10 17.4 6.4 19.6" />
        <path d="M6.4 19.6 9.2 20.8" />
    {:else if name === "wand"}
        <path d="M15.8 8.2 6.6 17.4" />
        <path d="M18.4 2.8v2.6" />
        <path d="M18.4 7.4v2.6" />
        <path d="M15 6.4h2.4" />
        <path d="M19.4 6.4h2.4" />
        <circle cx="20.6" cy="11.6" r="1" fill="currentColor" stroke="none" />
    {:else if name === "bucket"}
        <path d="M7.2 9.6 Q12 3.6 16.8 9.6" />
        <path d="M5.8 9.6 18.2 9.6" />
        <path d="M6.8 9.6 8.4 19.2 15.6 19.2 17.2 9.6" />
        <circle cx="20.2" cy="14.4" r="1.4" fill="currentColor" stroke="none" />
    {:else if name === "eyedropper"}
        <path
            d="M4.6 19.4 5.7 15.7 10 11.4 9 10.4 15.4 4 Q20.3 3.7 20 8.6
               L13.6 15 12.7 14 8.3 18.3Z"
        />
        <path d="M8.4 11.9 12.1 15.6" />
    {:else if name === "deselect"}
        <path d="M4.6 4.6 19.4 19.4" />
        <path d="M4.6 8.4V6a1.4 1.4 0 0 1 1.4-1.4h2.4" />
        <path d="M15.6 4.6H18A1.4 1.4 0 0 1 19.4 6v2.4" />
        <path d="M19.4 15.6V18a1.4 1.4 0 0 1-1.4 1.4h-2.4" />
        <path d="M8.4 19.4H6A1.4 1.4 0 0 1 4.6 18v-2.4" />
    {:else if name === "selectAll"}
        <path d="M4.6 8.4V6a1.4 1.4 0 0 1 1.4-1.4h2.4" />
        <path d="M15.6 4.6H18A1.4 1.4 0 0 1 19.4 6v2.4" />
        <path d="M19.4 15.6V18a1.4 1.4 0 0 1-1.4 1.4h-2.4" />
        <path d="M8.4 19.4H6A1.4 1.4 0 0 1 4.6 18v-2.4" />
        <path d="M11 4.6h2" />
        <path d="M11 19.4h2" />
        <path d="M4.6 11v2" />
        <path d="M19.4 11v2" />
    {:else if name === "invert"}
        <path d="M4.6 4.6h14.8v14.8H4.6Z" />
        <path d="M4.6 12 12 4.6v14.8Z" fill="currentColor" stroke="none" />
    {:else if name === "delete"}
        <path d="M5.6 7.4h12.8" />
        <path d="M9.4 7.4V5.6h5.2v1.8" />
        <path d="M7.2 7.4 8 19.4h8L16.8 7.4" />
    {:else if name === "copy"}
        <path d="M9 4.6h8.4A1.4 1.4 0 0 1 18.8 6v8.4" />
        <path d="M6.6 9.2h8.8a1.4 1.4 0 0 1 1.4 1.4v7.4a1.4 1.4 0 0 1-1.4 1.4H6.6a1.4 1.4 0 0 1-1.4-1.4v-7.4a1.4 1.4 0 0 1 1.4-1.4Z" />
    {:else if name === "cut"}
        <path d="M6.4 4.8 15.6 17.2" />
        <path d="M17.6 4.8 8.4 17.2" />
        <circle cx="6.6" cy="18.6" r="2.2" />
        <circle cx="17.4" cy="18.6" r="2.2" />
    {:else if name === "paste"}
        <path d="M8 5.8H6.6a1.4 1.4 0 0 0-1.4 1.4v11.2a1.4 1.4 0 0 0 1.4 1.4h10.8a1.4 1.4 0 0 0 1.4-1.4V7.2a1.4 1.4 0 0 0-1.4-1.4H16" />
        <path d="M9.2 4.2h5.6v3.2H9.2Z" />
    {:else if name === "undo"}
        <path d="M19 15.6 Q17.4 5.6 6.4 9.6" />
        <path d="M10.5 6.6 6.4 9.6 9.7 13.2" />
    {:else if name === "redo"}
        <path d="M5 15.6 Q6.6 5.6 17.6 9.6" />
        <path d="M13.5 6.6 17.6 9.6 14.3 13.2" />
    {:else if name === "play"}
        <path d="M8.6 5.4 19.2 12 8.6 18.6Z" fill="currentColor" stroke="none" />
    {:else if name === "pause"}
        <path
            d="M7.2 5.6h3.2v12.8H7.2Zm6.4 0h3.2v12.8h-3.2Z"
            fill="currentColor"
            stroke="none"
        />
    {/if}
</svg>

<style>
    svg {
        display: block;
        flex-shrink: 0;
    }
</style>
