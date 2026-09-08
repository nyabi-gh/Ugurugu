<script lang="ts">
    import { onMount } from "svelte";
    import { EngineClient } from "./lib/EngineClient";
    import type {
        BrushPresetInfo,
        DocumentMeta,
        LayerInfo,
        LayerThumbnail,
        RegionUpdate,
        SelectionContour,
        WobbleSettings,
    } from "./lib/EngineClient";
    import ColorPanel from "./lib/ColorPanel.svelte";
    import LayerPanel from "./lib/LayerPanel.svelte";
    import NewDocumentDialog from "./lib/NewDocumentDialog.svelte";
    import DocumentSizeDialog from "./lib/DocumentSizeDialog.svelte";
    import WobblePanel from "./lib/WobblePanel.svelte";
    import NoticesDialog from "./lib/NoticesDialog.svelte";
    import MobileChrome from "./lib/MobileChrome.svelte";
    import ToolIcon from "./lib/ToolIcon.svelte";
    import ToolOptions from "./lib/ToolOptions.svelte";
    import TextOptions from "./lib/TextOptions.svelte";
    import { textGeometry } from "./lib/TextGeometry";
    import type { TextDraft, TextGeometry } from "./lib/TextGeometry";
    import { decodeImage } from "./lib/ImageImport";
    import ToolRail from "./lib/ToolRail.svelte";
    import { CanvasPresenter } from "./lib/CanvasPresenter";
    import { SelectionOverlay } from "./lib/SelectionOverlay";
    import type { DragShape } from "./lib/SelectionOverlay";
    import SelectionTransformBar from "./lib/SelectionTransformBar.svelte";
    import {
        boundsCenter,
        compose,
        flipAbout,
        identity,
        isIdentity,
        rotationAbout,
        scaleAbout,
        transformedBounds,
        translation,
    } from "./lib/SelectionTransform";
    import type { Matrix } from "./lib/SelectionTransform";
    import { checkImportSize, detectMemoryProfile } from "./lib/MemoryPolicy";
    import type { MemoryProfile } from "./lib/MemoryPolicy";
    import { handleShortcut } from "./lib/Shortcuts";
    import {
        combineForModifiers,
        selectionCombines,
        selectionShapes,
        toolDefinition,
    } from "./lib/tools";
    import type { CombineValue, ToolId } from "./lib/tools";
    import {
        loadToolSettings,
        maximumBrushSize,
        minimumBrushSize,
        saveToolSettings,
    } from "./lib/ToolSettings";
    import {
        fitToViewport,
        normalizeRotation,
        pan,
        pinchMeasurement,
        toDocument,
        transformAround,
        zoomAround,
    } from "./lib/ViewTransform";
    import type { PinchMeasurement, ViewState } from "./lib/ViewTransform";
    import { AutosaveController } from "./lib/AutosaveController.svelte";
    import { downloadBlob } from "./lib/download";

    const engine = new EngineClient();
    const profile: MemoryProfile = detectMemoryProfile();

    let surfaceCanvas: HTMLCanvasElement;
    let displayCanvas: HTMLCanvasElement;
    let overlayCanvas: HTMLCanvasElement;
    let viewportElement: HTMLElement;
    let fileInput: HTMLInputElement;
    let imageInput: HTMLInputElement;
    let presenter: CanvasPresenter | null = null;
    let overlay: SelectionOverlay | null = null;

    let meta = $state<DocumentMeta | null>(null);
    let layers = $state<LayerInfo[]>([]);
    let presets = $state<BrushPresetInfo[]>([]);
    let eraserPresets = $state<BrushPresetInfo[]>([]);
    let frameIndex = $state(0);
    let playing = $state(false);
    let status = $state("Loading engine…");
    let documentName = $state("Untitled.ugu");
    let canUndo = $state(false);
    let canRedo = $state(false);

    let colorHex = $state("#1d2129");
    let tool = $state<ToolId>("brush");
    const textDraft = $state<TextDraft>({
        text: "Ugurugu",
        size: 64,
        width: 4,
        filled: false,
        x: 20,
        y: 20,
    });
    let textShape = $state.raw<TextGeometry | null>(null);
    let textMessage = $state("");
    let textApplying = $state(false);
    let importingImage = $state(false);
    let documentGeneration = 0;
    const textReady = $derived(
        !!textShape &&
            !textApplying &&
            !!meta &&
            layers.some(
                (layer) =>
                    layer.active &&
                    !layer.group &&
                    layer.visible &&
                    layer.opacity > 0,
            ) &&
            Number.isFinite(textDraft.width) &&
            textDraft.width >= 1 &&
            textDraft.width <= 128 &&
            Number.isFinite(textDraft.x) &&
            Number.isFinite(textDraft.y) &&
            textDraft.x >= 0 &&
            textDraft.y >= 0 &&
            textDraft.x <= (meta?.width ?? 0) &&
            textDraft.y <= (meta?.height ?? 0),
    );

    const settings = $state(loadToolSettings());
    let thumbnails = $state<LayerThumbnail[]>([]);
    let recentColors = $state<string[]>(loadRecentColors());
    let view = $state<ViewState>({
        scale: 1,
        rotation: 0,
        centerX: 0,
        centerY: 0,
    });
    let showNewDocument = $state(false);
    let showDocumentSize = $state(false);
    let showNotices = $state(false);
    let usingWebGL = $state(false);
    let hasSelection = $state(false);
    // Move mode is a mode, not a tool, exactly as on the desktop: while it is
    // on, a drag inside the selection moves the pixels whatever tool is picked.
    let selectionMoveMode = $state(false);
    let transformActive = $state(false);
    // Raw on purpose. A plain $state array is handed out as a proxy, and a
    // proxy cannot be structured-cloned, so posting one to the worker fails
    // with DataCloneError. The matrix is always replaced wholesale, never
    // mutated in place, so it needs no deep reactivity anyway.
    let transformMatrix = $state.raw<Matrix>(identity);
    const transformPending = $derived(
        transformActive && !isIdentity(transformMatrix),
    );
    // Same default as the desktop's canvas/animateWhileDrawing setting.
    let animateWhileDrawing = $state(
        window.localStorage.getItem("ugurugu-web-animate-while-drawing") ===
            "1",
    );

    const recentColorCapacity = 16;
    const zoomPercent = $derived(Math.round(view.scale * 100));
    const rotationDegrees = $derived(Math.round(view.rotation * 10) / 10);
    const activeTool = $derived(toolDefinition(tool));

    const autosave = new AutosaveController({
        ready: () => meta !== null && !drawing,
        revision: () => contentRevision,
        name: () => documentName,
        serialize: () => engine.serialize(),
        open: (bytes, name) => void openDocument(bytes, name),
    });
    let exportingGif = $state(false);

    let playTimer: ReturnType<typeof setInterval> | null = null;
    // Matches the CSS breakpoint. The layouts differ in structure, not only in
    // measurements, so the choice has to reach the markup as well.
    const compactQuery = "(max-width: 48rem)";
    // itch.io runs the upload in an iframe and draws its own fullscreen button
    // over the bottom-right corner. Comparing the frames is allowed across
    // origins; reading through them is not.
    const embedded = typeof window !== "undefined" && window.self !== window.top;
    let compact = $state(
        typeof window === "undefined"
            ? false
            : window.matchMedia(compactQuery).matches,
    );
    // The clipboard lives in the engine and is only ever filled by this shell,
    // so tracking that a copy happened is as good as asking it.
    let canPaste = $state(false);
    let drawing = false;
    let picking = false;
    let panning = $state(false);
    let rotating = $state(false);
    let spaceHeld = $state(false);
    let lastPanX = 0;
    let lastPanY = 0;
    let rotationDragStartX = 0;
    let rotationDragStartAngle = 0;
    const activeTouches = new Map<number, { x: number; y: number }>();
    let touchGesture: PinchMeasurement | null = null;
    // Two fingers hold an angle far less steadily than a hand thinks they do,
    // so a pan or a zoom arrived with a degree or two of twist on it and left
    // the canvas askew. Rotation waits until the twist accumulated over the
    // gesture says it was meant. The desktop keeps its immediate rotation:
    // there the same wobble is a smaller share of a much larger screen.
    const gestureRotationSlop = 5;
    let gestureTwist = 0;
    let gestureRotating = false;
    // Two fingers never land at the same instant, so the first one of a pinch
    // opened and committed a stroke before the second arrived, leaving an ink
    // dot at the start of every pan and zoom. A touch stroke is held here
    // until it crosses the slop or lifts; a second finger drops it.
    const touchStrokeSlop = 8;
    let pendingTouchStroke:
        | {
              x: number;
              y: number;
              pressure: number;
              timestamp: number;
              clientX: number;
              clientY: number;
              frame: number;
          }
        | null = null;
    let pendingPoints: number[] = [];
    let chain = Promise.resolve();
    let contentRevision = 0;
    let thumbnailTimer: ReturnType<typeof setTimeout> | null = null;
    let selectionDrag: DragShape | null = null;
    let selectionCombine: CombineValue = selectionCombines.replace;
    let antsFrame: number | null = null;
    // The outline the engine last sent, kept so a transform can be anchored on
    // the box the artist currently sees without asking for it again.
    let selectionContours: SelectionContour[] = [];
    let movingSelection = false;
    let moveStartX = 0;
    let moveStartY = 0;
    let moveBaseMatrix: Matrix = identity;
    // Latest matrix the pointer reached, and the one the engine has drawn. A
    // drag produces matrices faster than the engine renders them, so previews
    // coalesce to the newest instead of queueing every intermediate one.
    let queuedTransform: Matrix | null = null;
    let sentTransform: Matrix = identity;
    let transformPumping = false;
    // Playback renders at most one frame at a time. Without this the timer
    // queued a render every tick regardless of whether the engine had finished
    // the last one, so a document that renders slower than its frame interval
    // built a backlog that outlived the stop button by minutes.
    let playbackRenderPending = false;

    function loadRecentColors(): string[] {
        try {
            const stored = window.localStorage.getItem(
                "ugurugu-web-color-history",
            );
            const parsed = stored ? JSON.parse(stored) : [];
            return Array.isArray(parsed)
                ? parsed.filter(
                      (value) =>
                          typeof value === "string" &&
                          /^#[0-9a-f]{6}$/.test(value),
                  )
                : [];
        } catch {
            return [];
        }
    }

    function recordRecentColor(color: string) {
        recentColors = [
            color,
            ...recentColors.filter((existing) => existing !== color),
        ].slice(0, recentColorCapacity);
        try {
            window.localStorage.setItem(
                "ugurugu-web-color-history",
                JSON.stringify(recentColors),
            );
        } catch {
            // History is a convenience; drawing must not fail on storage.
        }
    }

    function chooseColor(color: string) {
        colorHex = color;
        // Picking a color says "paint with this", so the eyedropper hands the
        // canvas back to the brush. Every other tool keeps working.
        if (tool === "eyedropper") {
            tool = "brush";
        }
    }

    function describe(error: unknown): string {
        return String(error).replace(/^(Error:\s*)+/, "");
    }

    function scheduleThumbnailRefresh() {
        if (thumbnailTimer !== null) {
            clearTimeout(thumbnailTimer);
        }
        thumbnailTimer = setTimeout(() => {
            thumbnailTimer = null;
            enqueue(async () => {
                thumbnails = await engine.layerThumbnails(
                    window.devicePixelRatio || 1,
                );
            });
        }, 250);
    }

    function enqueue(operation: () => Promise<void>) {
        chain = chain.then(operation).catch((error) => {
            status = describe(error);
        });
    }

    // Same queue, but the caller can wait for its turn to finish. Swapping the
    // document has to run in order with everything else: posted out of band it
    // could land between the halves of an older operation, leaving queued work
    // to be applied to a document the user never chose.
    function enqueueExclusive(operation: () => Promise<void>): Promise<void> {
        const next = chain.then(operation).catch((error) => {
            status = describe(error);
        });
        chain = next;
        return next;
    }

    // Document properties the shell mirrors — canvas size, frames, fps,
    // wobble — only travel with the operations that change them.
    function applyMeta(next: DocumentMeta) {
        const resized =
            !meta || meta.width !== next.width || meta.height !== next.height;
        meta = next;
        frameIndex = Math.min(frameIndex, next.frameCount - 1);
        if (!resized) {
            return;
        }
        selectionMoveMode = false;
        selectionContours = [];
        overlay?.setContours([]);
        forgetTransformSession();
        presenter?.resizeDocument(next.width, next.height);
        resizeDisplay();
        zoomToFit();
    }

    function present(update: RegionUpdate) {
        if (update.meta) {
            applyMeta(update.meta);
        }
        layers = update.layers;
        canUndo = update.canUndo;
        canRedo = update.canRedo;
        hasSelection = update.selection.active;
        if (update.selection.contours) {
            selectionContours = update.selection.contours;
            overlay?.setContours(update.selection.contours);
            startAnts();
        }
        // A selection that went away takes the move mode and any floating
        // transform with it; there is nothing left to move.
        if (!hasSelection && (selectionMoveMode || transformActive)) {
            selectionMoveMode = false;
            forgetTransformSession();
        }
        if (!presenter) {
            return;
        }
        if (update.pixels && update.rect.width > 0) {
            presenter.writeRegion(update.rect, update.pixels);
        }
        presenter.draw(view);
        overlay?.draw(view);
    }

    // The ants only run while there is something to animate, so an idle
    // document costs no frames.
    function startAnts() {
        if (antsFrame !== null || !overlay) {
            return;
        }
        const step = () => {
            if (!overlay?.advance()) {
                antsFrame = null;
                overlay?.draw(view);
                return;
            }
            overlay.draw(view);
            antsFrame = requestAnimationFrame(step);
        };
        antsFrame = requestAnimationFrame(step);
    }

    function requestRender(frame: number) {
        enqueue(async () => {
            present(await engine.renderFrame(frame));
        });
    }

    function viewportSize() {
        const rect = displayCanvas.getBoundingClientRect();
        return { width: rect.width, height: rect.height };
    }

    function resizeDisplay() {
        if (!presenter || !viewportElement) {
            return;
        }
        const rect = viewportElement.getBoundingClientRect();
        // A viewport with no area is a layout the shell cannot draw into — a
        // hidden tab, a collapsed iframe. Keeping the surfaces at their last
        // real size means the view comes back unchanged instead of clamped to
        // the 1px floor and refitted to nothing.
        if (rect.width <= 0 || rect.height <= 0) {
            return;
        }
        const ratio = window.devicePixelRatio || 1;
        presenter.resizeDisplay(rect.width, rect.height, ratio);
        overlay?.resize(rect.width, rect.height, ratio);
        presenter.draw(view);
        overlay?.draw(view);
    }

    function zoomToFit() {
        const viewport = viewportSize();
        if (!meta || viewport.width <= 0 || viewport.height <= 0) {
            return;
        }
        view = fitToViewport(meta, viewport, view.rotation);
    }

    function zoomToActualSize() {
        if (!meta) {
            return;
        }
        view = {
            scale: 1,
            rotation: view.rotation,
            centerX: meta.width / 2,
            centerY: meta.height / 2,
        };
    }

    function zoomBy(factor: number) {
        const viewport = viewportSize();
        view = zoomAround(
            view,
            viewport,
            viewport.width / 2,
            viewport.height / 2,
            view.scale * factor,
        );
    }

    function rotationInputAvailable() {
        return (
            !drawing &&
            !picking &&
            !panning &&
            !rotating &&
            !selectionDrag &&
            !movingSelection &&
            activeTouches.size === 0
        );
    }

    function setRotation(degrees: number) {
        if (!Number.isFinite(degrees) || !rotationInputAvailable()) {
            return;
        }
        view = { ...view, rotation: normalizeRotation(degrees) };
    }

    function rotateBy(delta: number) {
        setRotation(view.rotation + delta);
    }

    function resetRotation() {
        setRotation(0);
    }

    function onRotationInput(event: Event) {
        setRotation(Number((event.currentTarget as HTMLInputElement).value));
    }

    $effect(() => {
        const red = Number.parseInt(colorHex.slice(1, 3), 16);
        const green = Number.parseInt(colorHex.slice(3, 5), 16);
        const blue = Number.parseInt(colorHex.slice(5, 7), 16);
        const erase = tool === "eraser";
        const width = erase ? settings.eraserSize : settings.brushSize;
        if (!meta) {
            return;
        }
        enqueue(() =>
            engine.setBrush({ red, green, blue, alpha: 255, width, erase }),
        );
    });

    $effect(() => {
        const index =
            tool === "eraser"
                ? settings.eraserPresetIndex
                : settings.presetIndex;
        const erasing = tool === "eraser";
        if (!meta) {
            return;
        }
        enqueue(() =>
            erasing
                ? engine.setEraserPreset(index)
                : engine.setBrushPreset(index),
        );
    });

    $effect(() => {
        const strength = settings.stabilization / 100;
        if (!meta) {
            return;
        }
        enqueue(() => engine.setStabilization(strength));
    });

    $effect(() => {
        const antialiasing = settings.brushAntialiasing;
        if (!meta) {
            return;
        }
        enqueue(() => engine.setBrushAntialiasing(antialiasing));
    });

    $effect(() => {
        const options = {
            reference: settings.fillReference,
            comparison: settings.fillComparison,
            tolerance: settings.fillTolerance,
            antialiasing: settings.bucketAntialiasing,
        };
        if (!meta) {
            return;
        }
        enqueue(() => engine.setFillOptions(options));
    });

    $effect(() => {
        saveToolSettings($state.snapshot(settings));
    });

    $effect(() => {
        const current = view;
        presenter?.draw(current);
        overlay?.draw(current);
    });

    $effect(() => {
        try {
            window.localStorage.setItem(
                "ugurugu-web-animate-while-drawing",
                animateWhileDrawing ? "1" : "0",
            );
        } catch {
            // A preference that cannot be stored still applies this session.
        }
    });

    $effect(() => {
        const enabled = tool === "text" && !!meta;
        const content = textDraft.text;
        const size = textDraft.size;
        textShape = null;
        if (!enabled) return;
        textMessage = "Preparing text…";
        let cancelled = false;
        void textGeometry(content, size)
            .then((shape) => {
                if (!cancelled) {
                    textShape = shape;
                    textMessage = "Apply adds the text as one undoable edit.";
                }
            })
            .catch((error: unknown) => {
                if (!cancelled) textMessage = describe(error);
            });
        return () => {
            cancelled = true;
        };
    });

    $effect(() => {
        const shape = textShape;
        const preview =
            tool === "text" &&
            shape &&
            Number.isFinite(textDraft.x) &&
            Number.isFinite(textDraft.y) &&
            Number.isFinite(textDraft.width)
                ? {
                      path: shape.path,
                      x: textDraft.x,
                      y: textDraft.y,
                      width: Math.max(1, textDraft.width),
                      filled: textDraft.filled,
                      color: colorHex,
                  }
                : null;
        overlay?.setText(preview);
        overlay?.draw(view);
    });

    function applyText() {
        const shape = textShape;
        const layerId = layers.find((layer) => layer.active)?.id;
        if (!textReady || !shape || !layerId || tool !== "text") return;
        const draft = $state.snapshot(textDraft);
        const color = colorHex;
        const generation = documentGeneration;
        stopPlayback();
        cancelTransformForBoundary(
            "The selection transform was canceled before adding text.",
        );
        textApplying = true;
        enqueue(async () => {
            try {
                if (generation !== documentGeneration)
                    throw new Error(
                        "The document changed; place the text again.",
                    );
                const index = layers.findIndex((layer) => layer.id === layerId);
                if (index < 0)
                    throw new Error("The text layer no longer exists.");
                present(
                    await engine.text(
                        frameIndex,
                        index,
                        shape.commands,
                        draft.x,
                        draft.y,
                        draft.width,
                        draft.filled,
                        (0xff000000 | parseInt(color.slice(1), 16)) >>> 0,
                    ),
                );
                contentRevision += 1;
                recordRecentColor(color);
                tool = "brush";
                status = "Text added";
                scheduleThumbnailRefresh();
            } finally {
                textApplying = false;
            }
        });
    }

    function onImageChosen(event: Event) {
        const input = event.currentTarget as HTMLInputElement;
        const file = input.files?.[0];
        input.value = "";
        if (!file || !meta || importingImage) return;
        const generation = documentGeneration;
        stopPlayback();
        if (tool === "text") tool = "brush";
        cancelTransformForBoundary(
            "The selection transform was canceled before importing an image.",
        );
        importingImage = true;
        // Reserve the queue before decoding: a later New/Open cannot overtake
        // the import and receive pixels chosen for the previous document.
        enqueue(async () => {
            try {
                if (generation !== documentGeneration)
                    throw new Error(
                        "The document changed; import the image again.",
                    );
                status = `Importing ${file.name}…`;
                const image = await decodeImage(file, profile);
                present(
                    await engine.insertImage(
                        frameIndex,
                        image.pixels,
                        image.width,
                        image.height,
                        file.name,
                    ),
                );
                contentRevision += 1;
                status = `${file.name} added as a new layer`;
                scheduleThumbnailRefresh();
            } finally {
                importingImage = false;
            }
        });
    }

    function adoptDocument(next: DocumentMeta, name: string) {
        documentGeneration += 1;
        if (tool === "text") tool = "brush";
        meta = next;
        documentName = name;
        frameIndex = 0;
        contentRevision = 0;
        autosave.reset();
        layers = next.layers;
        presets = next.presets;
        eraserPresets = next.eraserPresets;
        settings.presetIndex = Math.min(
            settings.presetIndex,
            Math.max(0, presets.length - 1),
        );
        settings.eraserPresetIndex = Math.min(
            settings.eraserPresetIndex,
            Math.max(0, eraserPresets.length - 1),
        );
        canUndo = next.canUndo;
        canRedo = next.canRedo;
        hasSelection = false;
        selectionMoveMode = false;
        selectionContours = [];
        // The engine handle this session belonged to is gone with the old
        // document, so only the shell's half has to be dropped.
        forgetTransformSession();
        overlay?.setContours([]);
        presenter?.resizeDocument(next.width, next.height);
        resizeDisplay();
        zoomToFit();
        thumbnails = [];
        requestRender(0);
        scheduleThumbnailRefresh();
    }

    function openDocument(bytes: ArrayBuffer, name: string): Promise<void> {
        return enqueueExclusive(async () => {
            stopPlayback();
            const verdict = checkImportSize(bytes.byteLength, profile);
            if (!verdict.allowed) {
                status = `Cannot open — ${verdict.reason}`;
                return;
            }
            status = `Opening ${name}…`;
            try {
                const next = await engine.open(bytes, profile.undoLimit);
                adoptDocument(next, name);
                const warning = verdict.warning ? ` ⚠ ${verdict.warning}` : "";
                status =
                    `${name} — ${next.width}×${next.height}, ` +
                    `${next.frameCount} frames @ ${next.fps} fps, ` +
                    `schema v${next.schemaVersion}${warning}`;
            } catch (error) {
                // The worker keeps the document that was already open when an
                // open fails, so the shell keeps its own state and reports the
                // failure rather than leaving the artist with nothing.
                status = `Open failed: ${describe(error)}`;
            }
        });
    }

    function createDocument(width: number, height: number): Promise<void> {
        showNewDocument = false;
        return enqueueExclusive(async () => {
            stopPlayback();
            status = `Creating a ${width}×${height} document…`;
            try {
                const next = await engine.create(
                    width,
                    height,
                    profile.undoLimit,
                );
                adoptDocument(next, "Untitled.ugu");
                status =
                    `New document — ${next.width}×${next.height}, ` +
                    `${next.frameCount} frames @ ${next.fps} fps, ` +
                    `schema v${next.schemaVersion}`;
            } catch (error) {
                status = `New document failed: ${describe(error)}`;
            }
        });
    }

    function stopPlayback() {
        playing = false;
        playbackRenderPending = false;
        if (playTimer !== null) {
            clearInterval(playTimer);
            playTimer = null;
        }
    }

    function togglePlayback() {
        if (tool === "text") tool = "brush";
        if (playing) {
            stopPlayback();
            return;
        }
        if (!meta || meta.frameCount < 2) {
            return;
        }
        cancelTransformForBoundary(
            "The pending selection transform was canceled before playback.",
        );
        playing = true;
        playTimer = setInterval(() => {
            // Mirrors CanvasWidget::advanceFrame: playback stays on through an
            // interaction and only stops advancing, so releasing the pointer
            // resumes the wobble instead of leaving it parked.
            if (!meta || panning || rotating || touchGesture || picking) {
                return;
            }
            if (drawing && !animateWhileDrawing) {
                return;
            }
            // Drop the tick rather than queue behind a render still in flight.
            // Playback shares the request queue with drawing and undo, so a
            // backlog here would stall the whole shell.
            if (playbackRenderPending) {
                return;
            }
            frameIndex = (frameIndex + 1) % meta.frameCount;
            const frame = frameIndex;
            playbackRenderPending = true;
            enqueue(async () => {
                try {
                    present(await engine.renderFrame(frame));
                } finally {
                    playbackRenderPending = false;
                }
            });
        }, 1000 / meta.fps);
    }

    function stepFrame(delta: number) {
        if (!meta) {
            return;
        }
        stopPlayback();
        // The lifted pixels belong to the frame they were lifted from, so
        // leaving that frame ends the session rather than carrying it along.
        cancelTransformForBoundary(
            "The pending selection transform was canceled before changing " +
                "frame.",
        );
        const count = meta.frameCount;
        frameIndex = (((frameIndex + delta) % count) + count) % count;
        requestRender(frameIndex);
    }

    function documentPosition(
        event: PointerEvent | { clientX: number; clientY: number },
    ) {
        const rect = displayCanvas.getBoundingClientRect();
        return toDocument(
            view,
            { width: rect.width, height: rect.height },
            event.clientX - rect.left,
            event.clientY - rect.top,
        );
    }

    function canvasPosition(event: PointerEvent) {
        const { x, y } = documentPosition(event);
        return {
            x,
            y,
            pressure:
                event.pointerType === "mouse"
                    ? 1
                    : event.pressure > 0
                      ? event.pressure
                      : 1,
        };
    }

    // The batch is taken here, not inside the queued task. Reading the buffer
    // when the task finally ran meant a fast second stroke could reset it
    // first, so the tail of the previous line vanished — or worse, the new
    // stroke's points were already in it and got appended to the line that was
    // still open. Claiming them synchronously ties every batch to the stroke
    // that produced it, because the queue preserves the order they went in.
    function flushPendingPoints() {
        if (pendingPoints.length === 0) {
            return;
        }
        const points = pendingPoints;
        pendingPoints = [];
        enqueue(async () => {
            present(await engine.strokeAppend(frameIndex, points));
        });
    }

    function pickColor(event: PointerEvent) {
        if (!presenter) {
            return;
        }
        const { x, y } = documentPosition(event);
        const [red, green, blue] = presenter.readPixel(x, y);
        colorHex = `#${((red << 16) | (green << 8) | blue)
            .toString(16)
            .padStart(6, "0")}`;
    }

    function beginPan(event: PointerEvent) {
        panning = true;
        lastPanX = event.clientX;
        lastPanY = event.clientY;
        displayCanvas.setPointerCapture(event.pointerId);
    }

    function beginRotation(event: PointerEvent) {
        rotating = true;
        rotationDragStartX = event.clientX;
        rotationDragStartAngle = view.rotation;
        displayCanvas.setPointerCapture(event.pointerId);
    }

    function markContentChanged() {
        contentRevision += 1;
        scheduleThumbnailRefresh();
    }

    function bucketFill(event: PointerEvent) {
        const { x, y } = documentPosition(event);
        const frame = frameIndex;
        const usedColor = colorHex;
        enqueue(async () => {
            present(await engine.bucketFill(frame, x, y));
            recordRecentColor(usedColor);
        });
        markContentChanged();
    }

    function wandSelect(event: PointerEvent) {
        const { x, y } = documentPosition(event);
        const frame = frameIndex;
        const combine = combineForModifiers(event);
        enqueue(async () => {
            present(await engine.selectionFlood(frame, x, y, combine));
        });
    }

    function beginSelectionDrag(event: PointerEvent) {
        const { x, y } = documentPosition(event);
        selectionCombine = combineForModifiers(event);
        selectionDrag = {
            shape: settings.selectionShape,
            points: [x, y],
        };
        overlay?.setDrag(selectionDrag);
        startAnts();
    }

    function continueSelectionDrag(event: PointerEvent) {
        if (!selectionDrag) {
            return;
        }
        const { x, y } = documentPosition(event);
        if (selectionDrag.shape === "freehand") {
            const points = selectionDrag.points;
            const lastX = points[points.length - 2] ?? x;
            const lastY = points[points.length - 1] ?? y;
            // Same 1px gate CanvasWidget::continueAreaSelection applies, so a
            // slow drag does not pile up thousands of coincident vertices.
            if (Math.hypot(x - lastX, y - lastY) < 1) {
                return;
            }
            points.push(x, y);
        } else if (selectionDrag.points.length >= 4) {
            selectionDrag.points[2] = x;
            selectionDrag.points[3] = y;
        } else {
            selectionDrag.points.push(x, y);
        }
        overlay?.setDrag(selectionDrag);
        overlay?.draw(view);
    }

    function endSelectionDrag() {
        const drag = selectionDrag;
        selectionDrag = null;
        overlay?.setDrag(null);
        if (!drag) {
            return;
        }
        const shape = selectionShapes[drag.shape];
        const points = drag.points;
        const combine = selectionCombine;
        const paint = settings.lassoMode === "paint";
        const frame = frameIndex;
        const usedColor = colorHex;
        if (points.length < 4) {
            overlay?.draw(view);
            return;
        }
        enqueue(async () => {
            present(
                await engine.selectionShape(
                    frame,
                    shape,
                    points,
                    combine,
                    paint,
                ),
            );
            if (paint) {
                recordRecentColor(usedColor);
            }
        });
        if (paint) {
            markContentChanged();
        }
    }

    function selectionAction(
        action: "all" | "invert" | "fill" | "delete" | "deselect",
    ) {
        if (!meta) {
            return;
        }
        // Delete and Backspace reach here whether or not anything is selected;
        // asking the engine anyway turned a stray keypress into an error in
        // the status bar.
        if (action !== "all" && action !== "deselect" && !hasSelection) {
            return;
        }
        cancelTransformForBoundary(
            "The pending selection transform was canceled before selecting.",
        );
        const frame = frameIndex;
        const usedColor = colorHex;
        enqueue(async () => {
            if (action === "all") {
                present(await engine.selectAll());
                return;
            }
            if (action === "invert") {
                present(await engine.invertSelection());
                return;
            }
            if (action === "deselect") {
                present(await engine.deselect());
                return;
            }
            if (action === "fill") {
                present(await engine.fillSelection(frame));
                recordRecentColor(usedColor);
                return;
            }
            present(await engine.deleteSelection(frame));
        });
        if (action === "fill" || action === "delete") {
            markContentChanged();
        }
    }

    // Copy and cut need a selection; paste only needs something on the
    // clipboard, which is the shell's own buffer inside the engine and so
    // survives opening another document but not reloading the tab.
    function clipboardAction(action: "copy" | "cut" | "paste") {
        if (!meta) {
            return;
        }
        if (action !== "paste" && !hasSelection) {
            return;
        }
        cancelTransformForBoundary(
            "The pending selection transform was canceled before copying.",
        );
        const frame = frameIndex;
        enqueue(async () => {
            if (action === "copy") {
                present(await engine.copySelection(frame));
                canPaste = true;
                // The copy lands on a new layer offset from the original, so
                // it can be dragged away at once — CanvasWidget::copySelection
                // arms the same move mode.
                selectionMoveMode = true;
                status = "Copied to a new layer. Drag inside it to move it.";
            } else if (action === "cut") {
                present(await engine.cutSelection(frame));
                canPaste = true;
                status = "Selection cut.";
            } else {
                present(await engine.paste(frame));
                status = "Pasted as a new layer.";
            }
        });
        markContentChanged();
    }

    // Local half of ending a session. The engine side is ended separately by
    // apply or cancel, so this never talks to the worker on its own.
    function forgetTransformSession() {
        transformActive = false;
        transformMatrix = identity;
        queuedTransform = null;
        sentTransform = identity;
        movingSelection = false;
        overlay?.setTransform(identity);
    }

    // Sets the pending matrix and shows it. The ants follow immediately from
    // the outline the shell already holds; the pixels follow when the engine
    // answers, which is why the outline is not waited on.
    function requestTransformPreview(matrix: Matrix) {
        transformMatrix = matrix;
        overlay?.setTransform(matrix);
        overlay?.draw(view);
        queuedTransform = matrix;
        if (transformPumping) {
            return;
        }
        transformPumping = true;
        const frame = frameIndex;
        enqueue(async () => {
            try {
                if (!transformActive) {
                    present(await engine.selectionTransformBegin(frame));
                    transformActive = true;
                }
                // Drains to whatever the gesture reached last: a preview the
                // pointer has already moved past is not worth rendering.
                while (queuedTransform) {
                    const next = queuedTransform;
                    queuedTransform = null;
                    present(await engine.selectionTransformUpdate(next));
                    sentTransform = next;
                }
            } catch (error) {
                // The lift was refused — a hidden layer, an empty selection.
                // Drop the local session so the ants stop showing a move that
                // is not happening.
                forgetTransformSession();
                overlay?.draw(view);
                throw error;
            } finally {
                transformPumping = false;
            }
        });
    }

    // Centre of the box the artist currently sees, so a second scale grows what
    // is on screen rather than the original selection.
    function transformAnchor() {
        const bounds = transformedBounds(selectionContours, transformMatrix);
        return bounds ? boundsCenter(bounds) : null;
    }

    function scaleSelectionBy(percent: number) {
        const factor = percent / 100;
        const anchor = transformAnchor();
        if (!anchor || !Number.isFinite(factor) || factor <= 0 || factor === 1) {
            return;
        }
        requestTransformPreview(
            compose(transformMatrix, scaleAbout(anchor.x, anchor.y, factor)),
        );
    }

    function rotateSelectionBy(degrees: number) {
        const anchor = transformAnchor();
        if (!anchor || !Number.isFinite(degrees) || degrees === 0) {
            return;
        }
        requestTransformPreview(
            compose(transformMatrix, rotationAbout(anchor.x, anchor.y, degrees)),
        );
    }

    function flipSelectionBy(horizontal: boolean) {
        const anchor = transformAnchor();
        if (!anchor) {
            return;
        }
        requestTransformPreview(
            compose(transformMatrix, flipAbout(anchor.x, anchor.y, horizontal)),
        );
    }

    function applySelectionTransform() {
        if (!transformActive) {
            return;
        }
        const committed = transformMatrix;
        const moved = !isIdentity(committed);
        const behind = sentTransform !== committed;
        forgetTransformSession();
        overlay?.draw(view);
        enqueue(async () => {
            // A coalesced preview may have been dropped, so the engine is told
            // the matrix the artist actually sees before it commits it.
            if (moved && behind) {
                present(await engine.selectionTransformUpdate(committed));
            }
            present(await engine.selectionTransformApply());
        });
        if (moved) {
            status = "Selection transform applied.";
            markContentChanged();
        }
    }

    function cancelSelectionTransform() {
        if (!transformActive) {
            return;
        }
        forgetTransformSession();
        overlay?.draw(view);
        enqueue(async () => {
            present(await engine.selectionTransformCancel());
        });
    }

    // Anything that changes the document underneath a floating transform drops
    // it rather than committing something the artist did not ask for, the same
    // call CanvasWidget::cancelSelectionTransformForBoundary makes.
    function cancelTransformForBoundary(message: string) {
        if (!transformActive) {
            return;
        }
        cancelSelectionTransform();
        status = message;
    }

    function beginSelectionMove(event: PointerEvent) {
        const { x, y } = documentPosition(event);
        movingSelection = true;
        moveStartX = x;
        moveStartY = y;
        moveBaseMatrix = transformMatrix;
        displayCanvas.setPointerCapture(event.pointerId);
    }

    function continueSelectionMove(event: PointerEvent) {
        const { x, y } = documentPosition(event);
        requestTransformPreview(
            compose(
                moveBaseMatrix,
                translation(x - moveStartX, y - moveStartY),
            ),
        );
    }

    function endSelectionMove(event: PointerEvent) {
        movingSelection = false;
        displayCanvas.releasePointerCapture(event.pointerId);
    }

    function toggleSelectionMoveMode() {
        if (!hasSelection) {
            return;
        }
        selectionMoveMode = !selectionMoveMode;
        if (!selectionMoveMode) {
            return;
        }
        status = "Move mode — drag inside the selection to move it.";
    }

    function onPointerDown(event: PointerEvent) {
        if (!meta) {
            return;
        }
        if (event.pointerType === "touch") {
            activeTouches.set(event.pointerId, {
                x: event.clientX,
                y: event.clientY,
            });
            displayCanvas.setPointerCapture(event.pointerId);
            if (activeTouches.size >= 2) {
                // A second finger turns an in-progress stroke into a gesture.
                // The line drawn so far is committed rather than dropped, so
                // starting a pinch never silently throws away a stroke; undo
                // is one keypress away if it was not wanted.
                if (drawing) {
                    drawing = false;
                    pendingPoints = [];
                    if (pendingTouchStroke) {
                        pendingTouchStroke = null;
                    } else {
                        enqueue(async () => {
                            present(await engine.strokeEnd(frameIndex));
                            contentRevision += 1;
                        });
                    }
                }
                if (selectionDrag) {
                    selectionDrag = null;
                    overlay?.setDrag(null);
                }
                // The pending transform survives the gesture; only the drag
                // that was steering it ends here.
                movingSelection = false;
                touchGesture = pinchMeasurement(activeTouches.values());
                gestureTwist = 0;
                gestureRotating = false;
                return;
            }
        }
        if (spaceHeld && event.shiftKey && event.button === 0) {
            event.preventDefault();
            beginRotation(event);
            return;
        }
        if (spaceHeld || event.button === 1) {
            event.preventDefault();
            beginPan(event);
            return;
        }
        if (event.button !== 0) {
            return;
        }
        displayCanvas.setPointerCapture(event.pointerId);
        // Move mode owns the pointer whatever tool is selected, the same
        // precedence CanvasWidgetEvents gives m_selectionMoveMode.
        if (selectionMoveMode && hasSelection) {
            const { x, y } = documentPosition(event);
            if (overlay?.containsDocumentPoint(x, y)) {
                beginSelectionMove(event);
            } else {
                status = "Drag inside the selection to move it.";
            }
            return;
        }
        if (tool === "eyedropper") {
            picking = true;
            pickColor(event);
            return;
        }
        cancelTransformForBoundary(
            "The pending selection transform was canceled.",
        );
        if (tool === "text") {
            const { x, y } = canvasPosition(event);
            textDraft.x = Math.round(x);
            textDraft.y = Math.round(y);
            return;
        }
        if (tool === "bucket") {
            bucketFill(event);
            return;
        }
        if (tool === "wand") {
            wandSelect(event);
            return;
        }
        if (tool === "lasso") {
            beginSelectionDrag(event);
            return;
        }
        drawing = true;
        pendingPoints = [];
        const { x, y, pressure } = canvasPosition(event);
        const frame = frameIndex;
        const timestamp = event.timeStamp;
        if (event.pointerType === "touch") {
            pendingTouchStroke = {
                x,
                y,
                pressure,
                timestamp,
                clientX: event.clientX,
                clientY: event.clientY,
                frame,
            };
            return;
        }
        beginStroke(frame, x, y, pressure, timestamp);
    }

    function beginStroke(
        frame: number,
        x: number,
        y: number,
        pressure: number,
        timestamp: number,
    ) {
        enqueue(async () => {
            try {
                present(
                    await engine.strokeBegin(frame, x, y, pressure, timestamp),
                );
            } catch (error) {
                // The engine refused the stroke — a hidden layer, or none to
                // paint on. Drop the local drawing state so the pointer up
                // does not chase a stroke that never started.
                drawing = false;
                pendingPoints = [];
                throw error;
            }
        });
    }

    // Opens the stroke on the point the finger went down on, not the one that
    // crossed the slop.
    function promoteTouchStroke() {
        if (!pendingTouchStroke) {
            return;
        }
        const { frame, x, y, pressure, timestamp } = pendingTouchStroke;
        pendingTouchStroke = null;
        beginStroke(frame, x, y, pressure, timestamp);
    }

    function onPointerMove(event: PointerEvent) {
        if (
            event.pointerType === "touch" &&
            activeTouches.has(event.pointerId)
        ) {
            activeTouches.set(event.pointerId, {
                x: event.clientX,
                y: event.clientY,
            });
            if (activeTouches.size >= 2) {
                const nextGesture = pinchMeasurement(
                    activeTouches.values(),
                );
                if (
                    touchGesture &&
                    nextGesture &&
                    touchGesture.distance > 0 &&
                    nextGesture.distance > 0
                ) {
                    const rect = displayCanvas.getBoundingClientRect();
                    const angleDelta = normalizeRotation(
                        nextGesture.angle - touchGesture.angle,
                    );
                    gestureTwist += angleDelta;
                    if (Math.abs(gestureTwist) >= gestureRotationSlop) {
                        gestureRotating = true;
                    }
                    view = transformAround(
                        view,
                        { width: rect.width, height: rect.height },
                        touchGesture.centerX - rect.left,
                        touchGesture.centerY - rect.top,
                        nextGesture.centerX - rect.left,
                        nextGesture.centerY - rect.top,
                        view.scale *
                            (nextGesture.distance / touchGesture.distance),
                        gestureRotating
                            ? view.rotation + angleDelta
                            : view.rotation,
                    );
                }
                touchGesture = nextGesture;
                return;
            }
        }
        if (rotating) {
            view = {
                ...view,
                rotation: normalizeRotation(
                    rotationDragStartAngle +
                        (event.clientX - rotationDragStartX) * 0.5,
                ),
            };
            return;
        }
        if (panning) {
            view = pan(
                view,
                event.clientX - lastPanX,
                event.clientY - lastPanY,
            );
            lastPanX = event.clientX;
            lastPanY = event.clientY;
            return;
        }
        if (picking) {
            pickColor(event);
            return;
        }
        if (movingSelection) {
            continueSelectionMove(event);
            return;
        }
        if (selectionDrag) {
            continueSelectionDrag(event);
            return;
        }
        if (!drawing) {
            return;
        }
        if (pendingTouchStroke) {
            if (
                Math.hypot(
                    event.clientX - pendingTouchStroke.clientX,
                    event.clientY - pendingTouchStroke.clientY,
                ) < touchStrokeSlop
            ) {
                return;
            }
            promoteTouchStroke();
        }
        const samples =
            "getCoalescedEvents" in event
                ? event.getCoalescedEvents()
                : [event];
        for (const sample of samples) {
            const { x, y, pressure } = canvasPosition(sample);
            pendingPoints.push(x, y, pressure, sample.timeStamp);
        }
        flushPendingPoints();
    }

    function onPointerUp(event: PointerEvent) {
        activeTouches.delete(event.pointerId);
        touchGesture = pinchMeasurement(activeTouches.values());
        gestureTwist = 0;
        gestureRotating = false;
        if (rotating) {
            rotating = false;
            displayCanvas.releasePointerCapture(event.pointerId);
            return;
        }
        if (panning) {
            panning = false;
            displayCanvas.releasePointerCapture(event.pointerId);
            return;
        }
        if (picking) {
            picking = false;
            displayCanvas.releasePointerCapture(event.pointerId);
            return;
        }
        if (movingSelection) {
            endSelectionMove(event);
            return;
        }
        if (selectionDrag) {
            displayCanvas.releasePointerCapture(event.pointerId);
            endSelectionDrag();
            return;
        }
        if (!drawing) {
            return;
        }
        drawing = false;
        displayCanvas.releasePointerCapture(event.pointerId);
        if (pendingTouchStroke) {
            if (event.type === "pointercancel") {
                pendingTouchStroke = null;
                pendingPoints = [];
                return;
            }
            promoteTouchStroke();
        }
        flushPendingPoints();
        const frame = frameIndex;
        const usedColor = tool === "brush" ? colorHex : null;
        enqueue(async () => {
            present(await engine.strokeEnd(frame));
            contentRevision += 1;
            if (usedColor) {
                recordRecentColor(usedColor);
            }
        });
        scheduleThumbnailRefresh();
    }

    function onWheel(event: WheelEvent) {
        if (!meta) {
            return;
        }
        event.preventDefault();
        if (
            drawing ||
            picking ||
            panning ||
            rotating ||
            selectionDrag ||
            movingSelection ||
            activeTouches.size > 0
        ) {
            return;
        }
        const rect = displayCanvas.getBoundingClientRect();
        const viewport = { width: rect.width, height: rect.height };
        if (event.shiftKey) {
            const delta = event.deltaY !== 0 ? event.deltaY : event.deltaX;
            const wheelUnit =
                event.deltaMode === WheelEvent.DOM_DELTA_PIXEL
                    ? 100
                    : event.deltaMode === WheelEvent.DOM_DELTA_LINE
                      ? 3
                      : 1;
            rotateBy((-5 * delta) / wheelUnit);
            return;
        }
        if (event.ctrlKey || event.metaKey) {
            view = zoomAround(
                view,
                viewport,
                event.clientX - rect.left,
                event.clientY - rect.top,
                view.scale * Math.exp(-event.deltaY / 320),
            );
            return;
        }
        view = pan(view, -event.deltaX, -event.deltaY);
    }

    function undo() {
        if (tool === "text") tool = "brush";
        stopPlayback();
        cancelTransformForBoundary(
            "The pending selection transform was canceled before undoing.",
        );
        enqueue(async () => {
            present(await engine.undo(frameIndex));
            contentRevision += 1;
        });
        scheduleThumbnailRefresh();
    }

    function redo() {
        if (tool === "text") tool = "brush";
        stopPlayback();
        cancelTransformForBoundary(
            "The pending selection transform was canceled before redoing.",
        );
        enqueue(async () => {
            present(await engine.redo(frameIndex));
            contentRevision += 1;
        });
        scheduleThumbnailRefresh();
    }

    function documentAction(
        action: (frame: number) => Promise<RegionUpdate>,
        boundary = "document change",
    ) {
        if (tool === "text") tool = "brush";
        stopPlayback();
        cancelTransformForBoundary(
            `The pending selection transform was canceled before the ` +
                `${boundary}.`,
        );
        enqueue(async () => {
            present(await action(frameIndex));
            contentRevision += 1;
        });
        scheduleThumbnailRefresh();
    }

    function layerAction(action: (frame: number) => Promise<RegionUpdate>) {
        documentAction(action, "layer change");
    }

    // Unlike the other document changes this one leaves playback running: the
    // point of moving a wobble slider is to watch the drawing move.
    function wobbleChanged(
        next: WobbleSettings | null,
        layerId: string | null = null,
    ) {
        cancelTransformForBoundary(
            "The pending selection transform was canceled before the wobble " +
                "change.",
        );
        enqueue(async () => {
            if (layerId) {
                const index = layers.findIndex((layer) => layer.id === layerId);
                if (index < 0) {
                    status = "That layer no longer exists";
                    return;
                }
                present(await engine.layerWobble(frameIndex, index, next));
            } else if (next) {
                present(await engine.wobble(frameIndex, next));
            }
            contentRevision += 1;
        });
    }

    // A row index is only valid against the layer list the click was made on.
    // Layer commands go through the shared queue, so a second click lands
    // while the first is still in flight and every row after a delete or a
    // move has already been renumbered — two quick presses of Delete used to
    // remove a second, unrelated layer. Resolving the id when the command
    // actually runs is what keeps the engine acting on the layer the artist
    // pointed at, or on nothing at all if it is already gone.
    function layerCommand(
        id: string,
        action: (frame: number, index: number) => Promise<RegionUpdate>,
    ) {
        if (tool === "text") tool = "brush";
        stopPlayback();
        cancelTransformForBoundary(
            "The pending selection transform was canceled before the layer " +
                "change.",
        );
        enqueue(async () => {
            const index = layers.findIndex((layer) => layer.id === id);
            if (index < 0) {
                status = "That layer no longer exists";
                return;
            }
            present(await action(frameIndex, index));
            contentRevision += 1;
        });
        scheduleThumbnailRefresh();
    }

    function onSliderInput(event: Event) {
        if (tool === "text") tool = "brush";
        stopPlayback();
        frameIndex = Number((event.currentTarget as HTMLInputElement).value);
        requestRender(frameIndex);
    }

    async function onFileChosen(event: Event) {
        const input = event.currentTarget as HTMLInputElement;
        const file = input.files?.[0];
        if (!file) {
            return;
        }
        await openDocument(await file.arrayBuffer(), file.name);
        input.value = "";
    }

    async function downloadDocument() {
        if (tool === "text" && textShape && !textApplying) {
            status = "Apply or cancel the text preview before saving.";
            return;
        }
        return enqueueExclusive(async () => {
            try {
                const bytes = await engine.serialize();
                downloadBlob(
                    new Blob([bytes], { type: "application/octet-stream" }),
                    documentName,
                );
            } catch (error) {
                status = `Save failed: ${describe(error)}`;
            }
        });
    }

    function exportFramePng() {
        if (tool === "text") tool = "brush";
        stopPlayback();
        // Export encodes the committed document, so a floating transform that
        // is not in it must not be left on screen either.
        cancelTransformForBoundary(
            "The pending selection transform was canceled before exporting.",
        );
        const frame = frameIndex;
        enqueue(async () => {
            present(await engine.renderFrame(frame));
            const blob = await presenter?.toBlob("image/png");
            if (!blob) {
                throw new Error("PNG encoding failed");
            }
            downloadBlob(
                blob,
                `${documentName.replace(/\.ugu$/i, "")}-frame` +
                    `${frame + 1}.png`,
            );
        });
    }

    function exportGif() {
        if (tool === "text") tool = "brush";
        stopPlayback();
        cancelTransformForBoundary(
            "The pending selection transform was canceled before exporting.",
        );
        exportingGif = true;
        status = "Exporting GIF…";
        enqueue(async () => {
            try {
                const bytes = await engine.exportGif();
                downloadBlob(
                    new Blob([bytes], { type: "image/gif" }),
                    `${documentName.replace(/\.ugu$/i, "")}.gif`,
                );
                status = "GIF export complete";
            } finally {
                exportingGif = false;
            }
        });
    }

    function onKeyDown(event: KeyboardEvent) {
        if (event.key === " " && !spaceHeld) {
            const target = event.target as HTMLElement | null;
            const editing =
                target?.isContentEditable ||
                (target && ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName));
            if (!editing && (event.shiftKey || target?.tagName !== "BUTTON")) {
                spaceHeld = true;
                event.preventDefault();
            }
            return;
        }
        if (showNewDocument) {
            return;
        }
        if (showNotices) {
            if (event.key === "Escape") {
                showNotices = false;
                event.preventDefault();
            }
            return;
        }
        // While pixels are floating, Enter and Escape belong to them: Enter
        // commits the move as one undo entry, Escape puts the pixels back.
        // Both fall through to playback and deselect once nothing is floating.
        const focus = event.target as HTMLElement | null;
        const typing =
            focus?.isContentEditable ||
            (focus && ["INPUT", "TEXTAREA", "SELECT"].includes(focus.tagName));
        if (!typing && transformActive) {
            if (event.key === "Enter") {
                applySelectionTransform();
                event.preventDefault();
                return;
            }
            if (event.key === "Escape") {
                cancelSelectionTransform();
                event.preventDefault();
                return;
            }
        }
        if (focus?.closest("button") && event.key === "Enter") {
            return;
        }
        if (!typing && tool === "text") {
            if (event.key === "Enter") { applyText(); event.preventDefault(); return; }
            if (event.key === "Escape") { tool = "brush"; event.preventDefault(); return; }
        }
        const handled = handleShortcut(event, {
            undo,
            redo,
            save: () => void downloadDocument(),
            open: () => fileInput?.click(),
            newDocument: () => (showNewDocument = true),
            selectTool: (next) => (tool = next),
            adjustBrushSize: (delta) => {
                const clamp = (value: number) =>
                    Math.min(
                        maximumBrushSize,
                        Math.max(minimumBrushSize, value),
                    );
                if (tool === "eraser") {
                    settings.eraserSize = clamp(settings.eraserSize + delta);
                } else {
                    settings.brushSize = clamp(settings.brushSize + delta);
                }
            },
            zoomBy,
            zoomToFit,
            zoomToActualSize,
            rotateBy,
            resetRotation,
            stepFrame,
            togglePlayback,
            selectAll: () => selectionAction("all"),
            toggleSelectionMove: toggleSelectionMoveMode,
            invertSelection: () => selectionAction("invert"),
            deselect: () => selectionAction("deselect"),
            fillSelection: () => selectionAction("fill"),
            deleteSelection: () => selectionAction("delete"),
            copySelection: () => clipboardAction("copy"),
            cutSelection: () => clipboardAction("cut"),
            paste: () => clipboardAction("paste"),
        });
        if (handled) {
            event.preventDefault();
        }
    }

    function onKeyUp(event: KeyboardEvent) {
        if (event.key === " ") {
            spaceHeld = false;
        }
    }

    // Switching windows swallows the key up, and a stuck space turns every
    // later click into a pan.
    function onWindowBlur() {
        spaceHeld = false;
        rotating = false;
        touchGesture = null;
    }

    onMount(() => {
        presenter = new CanvasPresenter(
            surfaceCanvas,
            displayCanvas,
            (webgl) => (usingWebGL = webgl),
        );
        overlay = new SelectionOverlay(overlayCanvas);
        usingWebGL = presenter.usingWebGL;
        const observer = new ResizeObserver(() => resizeDisplay());
        observer.observe(viewportElement);
        resizeDisplay();
        void (async () => {
            await autosave.readOffer();
            await createDocument(
                profile.defaultCanvasWidth,
                profile.defaultCanvasHeight,
            );
        })();
        const stopAutosave = autosave.start();
        const media = window.matchMedia(compactQuery);
        const syncCompact = () => (compact = media.matches);
        media.addEventListener("change", syncCompact);
        syncCompact();
        return () => {
            observer.disconnect();
            media.removeEventListener("change", syncCompact);
            stopAutosave();
            if (thumbnailTimer !== null) {
                clearTimeout(thumbnailTimer);
            }
            if (antsFrame !== null) {
                cancelAnimationFrame(antsFrame);
            }
            stopPlayback();
        };
    });
</script>

<svelte:window
    onkeydown={onKeyDown}
    onkeyup={onKeyUp}
    onblur={onWindowBlur}
/>

<!-- File inputs survive mobile sheet dismissal and desktop/mobile switches. -->
<input id="open-document" bind:this={fileInput} type="file" accept=".ugu" hidden onchange={onFileChosen} />
<input id="import-image" bind:this={imageInput} type="file" accept="image/png,image/jpeg,image/webp"
    hidden disabled={!meta || importingImage} onchange={onImageChosen} />

{#snippet historyActions()}
    <button
        id="undo"
        class="icon-button"
        title="Undo (Ctrl/Cmd Z)"
        onclick={undo}
        disabled={!canUndo}
    >
        <ToolIcon name="undo" size={18} />
    </button>
    <button
        id="redo"
        class="icon-button"
        title="Redo (Ctrl/Cmd Shift Z)"
        onclick={redo}
        disabled={!canRedo}
    >
        <ToolIcon name="redo" size={18} />
    </button>
{/snippet}

{#snippet fileActions()}
    <button id="new-document" onclick={() => (showNewDocument = true)}>
        New
    </button>
    <button
        id="document-size"
        onclick={() => (showDocumentSize = true)}
        disabled={!meta}
    >
        Size
    </button>
    <button id="open-document-button" class="file-button" onclick={() => fileInput.click()}>Open</button>
    <button id="import-image-button" class="file-button" disabled={!meta || importingImage}
        onclick={() => imageInput.click()}>{importingImage ? "Importing…" : "Import image"}</button>
    <button id="save-document" onclick={downloadDocument} disabled={!meta}>
        Save
    </button>
    <button id="export-png" onclick={exportFramePng} disabled={!meta}>
        PNG
    </button>
    <button
        id="export-gif"
        onclick={exportGif}
        disabled={!meta || exportingGif}
    >
        {exportingGif ? "Exporting…" : "GIF"}
    </button>
{/snippet}

<!--
  Not decoration: the browser build links Qt statically and ships no files
  beside the page, so this is the only place the licence and the source it
  points at can be reached from. On a phone it lives in the file sheet, which
  is why it is a snippet rather than markup in the bar.
-->
{#snippet aboutAction()}
    <button id="notices" onclick={() => (showNotices = true)}>About</button>
{/snippet}

{#snippet toolRail()}
    <ToolRail
        {tool}
        {hasSelection}
        {canPaste}
        onselect={(next) => (tool = next)}
        onselectionaction={(action) => {
            if (action === "copy" || action === "cut" || action === "paste") {
                clipboardAction(action);
            } else {
                selectionAction(action);
            }
        }}
    />
{/snippet}

{#snippet toolOptions()}
    {#if tool === "text"}
        <TextOptions draft={textDraft} ready={textReady} message={textMessage}
            canvasWidth={meta?.width ?? 0} canvasHeight={meta?.height ?? 0}
            onapply={applyText} oncancel={() => (tool = "brush")} />
    {:else}
        <ToolOptions {tool} {settings} {presets} {eraserPresets} />
    {/if}
{/snippet}

{#snippet wobblePanel()}
    {#if meta}
        <WobblePanel
            wobble={meta.wobble}
            frameCount={meta.frameCount}
            layer={layers.find((layer) => layer.active)}
            onchange={wobbleChanged}
            onfollow={(id) => wobbleChanged(null, id)}
        />
    {/if}
{/snippet}

{#snippet colorPanel()}
    <ColorPanel
        color={colorHex}
        {recentColors}
        onchange={(hex) => (colorHex = hex)}
        onrecent={chooseColor}
    />
{/snippet}

{#snippet layerPanel()}
    <LayerPanel
        {layers}
        {thumbnails}
        onactivate={(id) =>
            layerCommand(id, (frame, index) =>
                engine.layerActivate(frame, index),
            )}
        onvisible={(id, visible) =>
            layerCommand(id, (frame, index) =>
                engine.layerVisible(frame, index, visible),
            )}
        onreference={(id, reference) =>
            layerCommand(id, (_frame, index) =>
                engine.layerReference(index, reference),
            )}
        onopacity={(id, opacity) =>
            layerCommand(id, (frame, index) =>
                engine.layerOpacity(frame, index, opacity),
            )}
        onadd={() => layerAction((frame) => engine.layerAdd(frame))}
        onremove={(id) =>
            layerCommand(id, (frame, index) =>
                engine.layerRemove(frame, index),
            )}
        onrename={(id, name) =>
            layerCommand(id, (frame, index) =>
                engine.layerRename(frame, index, name),
            )}
        onmove={(id, offset) =>
            layerCommand(id, (frame, index) =>
                engine.layerMove(frame, index, offset),
            )}
        onaddgroup={(id) => {
            if (id === null) {
                layerAction((frame) => engine.layerAddGroup(frame, -1));
            } else {
                layerCommand(id, (frame, index) =>
                    engine.layerAddGroup(frame, index),
                );
            }
        }}
        onduplicate={(id) =>
            layerCommand(id, (frame, index) =>
                engine.layerDuplicate(frame, index),
            )}
        onmergedown={(id) =>
            layerCommand(id, (frame, index) =>
                engine.layerMergeDown(frame, index),
            )}
        onclear={(id) =>
            layerCommand(id, (frame, index) =>
                engine.layerClear(frame, index),
            )}
        onblendmode={(id, mode) =>
            layerCommand(id, (frame, index) =>
                engine.layerBlendMode(frame, index, mode),
            )}
        onclip={(id, clipped) =>
            layerCommand(id, (frame, index) =>
                engine.layerClipToBelow(frame, index, clipped),
            )}
        onparentgroup={(id, groupId) =>
            layerCommand(id, (frame, index) =>
                engine.layerParentGroup(
                    frame,
                    index,
                    groupId === null
                        ? -1
                        : (layers.find((layer) => layer.id === groupId)
                              ?.index ?? -1),
                ),
            )}
    />
{/snippet}

<main class:compact>
    {#if !compact}
        <header class="bar">
            <div class="identity">
                <span class="wordmark">Ugurugu</span>
                <span class="document" title={documentName}>
                    {documentName}
                </span>
            </div>

            <div class="bar-group">{@render historyActions()}</div>
            <div class="bar-group">{@render fileActions()}</div>
            <div class="bar-group">{@render aboutAction()}</div>
        </header>
    {/if}

    {#if autosave.offer}
        <div class="recovery-banner" role="alert">
            <span>
                Unsaved work from an earlier session —
                {autosave.offer.name},
                {new Date(autosave.offer.savedAt).toLocaleString()}
            </span>
            <button id="recovery-restore" onclick={() => autosave.restore()}>
                Restore
            </button>
            <button
                id="recovery-discard"
                onclick={() => void autosave.discard()}
            >
                Discard
            </button>
        </div>
    {/if}

    <!--
      Document-resolution surface. Hidden from layout but still the authority
      for pixels: the eyedropper samples it and PNG export encodes it, so both
      stay independent of the current zoom.
    -->
    <canvas id="document-surface" bind:this={surfaceCanvas} hidden></canvas>

    <div class="workspace">
        {#if !compact}
            {@render toolRail()}
            {@render toolOptions()}
        {/if}

        <!--
          Rendered here in both layouts on purpose. Moving it into the branch
          would destroy and rebuild the canvas whenever the breakpoint is
          crossed, and the presenter holds the WebGL context of the canvas it
          was given.
        -->
        <section class="viewport" bind:this={viewportElement}>
            <canvas
                id="display-canvas"
                bind:this={displayCanvas}
                aria-label="Drawing canvas"
                class:panning={panning || spaceHeld}
                class:rotating={rotating}
                class:moving={selectionMoveMode && !panning && !spaceHeld}
                onpointerdown={onPointerDown}
                onpointermove={onPointerMove}
                onpointerup={onPointerUp}
                onpointercancel={onPointerUp}
                onwheel={onWheel}
            ></canvas>
            <canvas
                id="selection-overlay"
                bind:this={overlayCanvas}
                aria-hidden="true"
            ></canvas>
            {#if hasSelection}
                <SelectionTransformBar
                    moveMode={selectionMoveMode}
                    pending={transformPending}
                    onmovetoggle={toggleSelectionMoveMode}
                    onscale={scaleSelectionBy}
                    onrotate={rotateSelectionBy}
                    onflip={flipSelectionBy}
                    onapply={applySelectionTransform}
                    oncancel={cancelSelectionTransform}
                />
            {/if}
            <div class="rotation-controls" role="group" aria-label="Canvas rotation">
                <button
                    id="rotate-left"
                    title="Rotate canvas left 5° (−)"
                    aria-label="Rotate canvas left 5 degrees"
                    onclick={() => rotateBy(-5)}
                >
                    ↶
                </button>
                <label class="rotation-angle" title="Canvas rotation angle">
                    <input
                        id="rotation-angle"
                        type="number"
                        min="-180"
                        max="180"
                        step="0.5"
                        value={rotationDegrees}
                        aria-label="Canvas rotation angle"
                        onchange={onRotationInput}
                    />
                    <span aria-hidden="true">°</span>
                </label>
                <button
                    id="rotate-right"
                    title="Rotate canvas right 5° (^)"
                    aria-label="Rotate canvas right 5 degrees"
                    onclick={() => rotateBy(5)}
                >
                    ↷
                </button>
                <button
                    id="rotation-reset"
                    title="Reset canvas rotation to 0°"
                    aria-label="Reset canvas rotation to 0 degrees"
                    onclick={resetRotation}
                >
                    0°
                </button>
            </div>
            <div class="zoom-controls">
                <button
                    title="Zoom out (Ctrl/Cmd −)"
                    onclick={() => zoomBy(1 / 1.25)}
                >
                    −
                </button>
                <button
                    id="zoom-fit"
                    title="Fit to window (Ctrl/Cmd 0)"
                    onclick={zoomToFit}
                >
                    {zoomPercent}%
                </button>
                <button
                    title="Zoom in (Ctrl/Cmd +)"
                    onclick={() => zoomBy(1.25)}
                >
                    +
                </button>
            </div>
        </section>

        {#if !compact}
            <div class="side">
                {@render wobblePanel()}
                {@render colorPanel()}
                {@render layerPanel()}
            </div>
        {/if}
    </div>

    {#if compact}
        <MobileChrome
            {tool}
            color={colorHex}
            {playing}
            hasDocument={Boolean(meta)}
            {embedded}
            ontoggleplay={togglePlayback}
            {toolRail}
            {toolOptions}
            {wobblePanel}
            {colorPanel}
            {layerPanel}
            {fileActions}
            {aboutAction}
            {historyActions}
            {timelineControls}
            {statusBar}
        />
    {:else}
        <footer>
            <div class="timeline">{@render timelineControls()}</div>
            {@render statusBar()}
        </footer>
    {/if}
</main>

{#snippet timelineControls()}
    {#if meta}
        <button
            class="play icon-button"
            onclick={togglePlayback}
            disabled={meta.frameCount < 2}
            title={playing ? "Stop (Enter)" : "Play (Enter)"}
        >
            <ToolIcon name={playing ? "pause" : "play"} size={18} />
        </button>
        <input
            type="range"
            min="0"
            max={meta.frameCount - 1}
            value={frameIndex}
            aria-label="Frame"
            oninput={onSliderInput}
        />
        <span class="frame-label">{frameIndex + 1}/{meta.frameCount}</span>
        <label class="spin" title="Animation frames">
            <span>Frames</span>
            <input
                id="animation-frames"
                type="number"
                min="2"
                max="60"
                step="1"
                value={meta.frameCount}
                aria-label="Animation frames"
                onchange={(event) => {
                    // Read here, not inside the queued action: by the time
                    // that runs the event has no target left.
                    const frames = Number(event.currentTarget.value);
                    documentAction((frame) =>
                        engine.animationFrames(frame, frames),
                    );
                }}
            />
        </label>
        <label class="spin" title="Playback speed">
            <span>FPS</span>
            <input
                id="frames-per-second"
                type="number"
                min="1"
                max="50"
                step="1"
                value={meta.fps}
                aria-label="Frames per second"
                onchange={(event) => {
                    const fps = Number(event.currentTarget.value);
                    documentAction(() => engine.framesPerSecond(fps));
                }}
            />
        </label>
        <label class="toggle" title="Keep the wobble running while drawing">
            <input
                id="animate-while-drawing"
                type="checkbox"
                bind:checked={animateWhileDrawing}
            />
            Wobble while drawing
        </label>
    {/if}
{/snippet}

{#snippet statusBar()}
    <div class="status-bar">
        <p id="status">{status}</p>
        <p id="autosave-status">{autosave.status}</p>
        <p id="presenter-status">
            {activeTool.label} · {usingWebGL ? "WebGL 2" : "Canvas 2D"} ·
            {profile.name}
        </p>
    </div>
{/snippet}

{#if showNewDocument}
    <NewDocumentDialog
        {profile}
        oncreate={(width, height) => void createDocument(width, height)}
        oncancel={() => (showNewDocument = false)}
    />
{/if}

{#if showDocumentSize && meta}
    <DocumentSizeDialog
        {meta}
        {profile}
        onimage={(width, height) => {
            showDocumentSize = false;
            documentAction((frame) =>
                engine.resizeImage(frame, width, height),
            );
        }}
        oncanvas={(width, height, offsetX, offsetY) => {
            showDocumentSize = false;
            documentAction((frame) =>
                engine.resizeCanvas(frame, width, height, offsetX, offsetY),
            );
        }}
        oncancel={() => (showDocumentSize = false)}
    />
{/if}

{#if showNotices}
    <NoticesDialog {meta} onclose={() => (showNotices = false)} />
{/if}

<style>
    /*
      Palette and control shapes come from the desktop's Theme.cpp, so the web
      shell reads as the same product rather than a separate tool.
    */
    /*
      The desktop app bundles Pretendard JP, so the shell ships the same family
      cut to Latin and Hangul rather than falling back to whatever system-ui
      resolves to — on Korean Windows that is Malgun Gothic, which sets the same
      words in a visibly different voice. Anyone who already has the font
      installed downloads nothing. BUILDING.md holds the subsetting command.
      The url is relative because itch.io serves from a subdirectory.
    */
    @font-face {
        font-family: "Pretendard JP UI";
        src:
            local("Pretendard JP Medium"),
            local("PretendardJP-Medium"),
            url("./assets/PretendardJP-ui.woff2") format("woff2");
        font-weight: 500 700;
        font-style: normal;
        font-display: swap;
    }

    :global(:root) {
        /* The compact layout reserves this at the foot of the page, the dock
           fills exactly it, and sheets rise from its top edge. One expression
           so the three cannot drift apart. */
        --dock-block-size: calc(3.25rem + env(safe-area-inset-bottom, 0px));
        --ink-950: #141518;
        --ink-900: #1b1d21;
        --ink-850: #202226;
        --ink-800: #24262b;
        --ink-750: #2e3138;
        --line: #3f434b;
        --paper: #e8e8ea;
        --paper-dim: #9aa0a8;
        --paper-faint: #6a6f78;
        --accent: #ffc94a;
        --accent-bed: rgba(255, 201, 74, 0.13);
        --mono: ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace;
    }

    :global(body) {
        margin: 0;
        background: var(--ink-900);
        color: var(--paper);
        font-family:
            "Pretendard JP UI",
            system-ui,
            -apple-system,
            "Segoe UI",
            sans-serif;
        font-size: 14px;
        -webkit-font-smoothing: antialiased;
    }

    main {
        box-sizing: border-box;
        display: flex;
        flex-direction: column;
        block-size: 100vh;
    }

    .bar {
        display: flex;
        align-items: center;
        gap: 1rem;
        padding: 0.45rem 0.75rem;
        background: var(--ink-900);
        border-block-end: 1px solid var(--line);
    }

    .identity {
        display: flex;
        align-items: baseline;
        gap: 0.6rem;
        min-inline-size: 0;
        margin-inline-end: auto;
        overflow: hidden;
    }

    .wordmark {
        font-size: 0.75rem;
        font-weight: 700;
        letter-spacing: 0.22em;
        text-transform: uppercase;
        color: var(--accent);
    }

    .document {
        overflow: hidden;
        font-family: var(--mono);
        font-size: 0.75rem;
        color: var(--paper-dim);
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .bar-group {
        display: flex;
        align-items: center;
        gap: 0.3rem;
    }

    .workspace {
        flex: 1;
        display: flex;
        min-block-size: 0;
        /* Only reachable between the stacking breakpoint and the width the
           four columns need. Scrolling there beats shrinking the canvas. */
        overflow-x: auto;
    }

    .viewport {
        position: relative;
        flex: 1 1 0;
        /* A floor, not zero. The rail, the option column and the side panels
           are wider than a phone on their own, and the canvas was the only
           column flex could take that difference out of: its basis is zero, so
           it absorbed the whole overflow and came out 0 wide with nothing left
           to draw on. */
        min-inline-size: 12rem;
        overflow: hidden;
        background:
            repeating-conic-gradient(var(--ink-800) 0% 25%, var(--ink-850) 0% 50%)
            50% / 24px 24px;
    }

    .viewport canvas {
        position: absolute;
        inset: 0;
        inline-size: 100%;
        block-size: 100%;
        touch-action: none;
    }

    #display-canvas {
        cursor: crosshair;
    }

    #display-canvas.panning {
        cursor: grab;
    }

    #display-canvas.rotating {
        cursor: grabbing;
    }

    #display-canvas.moving {
        cursor: move;
    }

    #selection-overlay {
        pointer-events: none;
    }

    .zoom-controls,
    .rotation-controls {
        position: absolute;
        inset-inline-end: 0.75rem;
        display: flex;
        align-items: center;
        gap: 2px;
        padding: 2px;
        border-radius: 9px;
        background: rgba(20, 21, 24, 0.82);
        backdrop-filter: blur(6px);
    }

    .zoom-controls {
        inset-block-end: 0.75rem;
    }

    .rotation-controls {
        inset-block-end: 3.4rem;
    }

    .zoom-controls button,
    .rotation-controls button {
        min-inline-size: 2.4rem;
        padding: 0.2rem 0.4rem;
        border: 0;
        border-radius: 7px;
        background: transparent;
        font-family: var(--mono);
        font-size: 0.75rem;
        font-variant-numeric: tabular-nums;
    }

    .rotation-angle {
        display: flex;
        align-items: center;
        block-size: 1.7rem;
        padding-inline: 0.35rem;
        border-radius: 7px;
        color: var(--paper);
        font-family: var(--mono);
        font-size: 0.75rem;
        font-variant-numeric: tabular-nums;
    }

    .rotation-angle:focus-within {
        outline: 1px solid var(--accent);
    }

    .rotation-angle input {
        inline-size: 3.4rem;
        padding: 0;
        border: 0;
        outline: 0;
        background: transparent;
        color: inherit;
        font: inherit;
        text-align: end;
    }

    .side {
        display: flex;
        flex: none;
        flex-direction: column;
        inline-size: 15rem;
        min-block-size: 0;
        border-inline-start: 1px solid var(--line);
        background: var(--ink-850);
    }

    /* Below this the four columns cannot stand side by side at any usable
       canvas width, so they stack instead: canvas first at full width, tools
       and panels under it. The full responsive layout is still to come; this
       is the part that has to hold, because a canvas nobody can draw on is a
       broken shell rather than a cramped one. */
    /* Below the breakpoint the panels move into MobileChrome's sheets, so the
       workspace holds nothing but the canvas. All that is left to arrange is
       the strip the dock occupies. */
    main.compact {
        padding-block-end: var(--dock-block-size);
        --sheet-lift: var(--dock-block-size);
    }

    /* The foot of the canvas belongs to the status toast in this layout, so
       the view controls move under the file button instead of colliding with
       it. Pinching covers both of them; what they are still needed for is
       reading the numbers back and resetting them. */
    main.compact .rotation-controls {
        inset-block-start: 3.6rem;
        inset-block-end: auto;
    }

    main.compact .zoom-controls {
        inset-block-start: 6.4rem;
        inset-block-end: auto;
    }

    footer {
        border-block-start: 1px solid var(--line);
        background: var(--ink-900);
    }

    .timeline {
        display: flex;
        align-items: center;
        gap: 0.75rem;
        padding: 0.4rem 0.75rem;
        min-block-size: 1.6rem;
    }

    .timeline input[type="range"] {
        flex: 1;
        accent-color: var(--accent);
    }

    .frame-label {
        min-inline-size: 3.5rem;
        font-family: var(--mono);
        font-size: 0.75rem;
        font-variant-numeric: tabular-nums;
        text-align: end;
        color: var(--paper-dim);
    }

    .spin {
        display: flex;
        align-items: center;
        gap: 0.3rem;
        font-size: 0.625rem;
        font-weight: 700;
        letter-spacing: 0.1em;
        text-transform: uppercase;
        color: var(--paper-dim);
    }

    .spin input {
        inline-size: 3.2rem;
        padding: 0.15rem 0.3rem;
        border: 1px solid var(--line);
        border-radius: 6px;
        background: var(--ink-800);
        color: var(--paper);
        font-family: var(--mono);
        font-size: 0.6875rem;
        font-variant-numeric: tabular-nums;
    }

    .status-bar {
        display: flex;
        align-items: center;
        gap: 1.2rem;
        padding: 0.3rem 0.75rem 0.45rem;
        border-block-start: 1px solid var(--ink-850);
    }

    .status-bar p {
        margin: 0;
        overflow: hidden;
        font-size: 0.75rem;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    #status {
        flex: 1;
        color: var(--paper-dim);
    }

    #autosave-status,
    #presenter-status {
        color: var(--paper-faint);
    }

    #presenter-status {
        font-family: var(--mono);
    }

    .toggle {
        display: flex;
        align-items: center;
        gap: 0.35rem;
        font-size: 0.75rem;
        color: var(--paper-dim);
        white-space: nowrap;
        cursor: pointer;
    }

    .toggle input {
        accent-color: var(--accent);
    }

    .recovery-banner {
        display: flex;
        align-items: center;
        gap: 0.75rem;
        padding: 0.5rem 0.75rem;
        background: var(--accent-bed);
        border-block-end: 1px solid var(--line);
        color: var(--accent);
        font-size: 0.8rem;
    }

    button,
    .file-button {
        padding: 0.3rem 0.7rem;
        border: 1px solid var(--line);
        border-radius: 7px;
        background: var(--ink-800);
        color: var(--paper);
        font: inherit;
        font-size: 0.78rem;
        cursor: pointer;
    }

    button:hover:not(:disabled),
    .file-button:hover {
        background: var(--ink-750);
    }

    button:focus-visible,
    .file-button:focus-within {
        outline: 2px solid var(--accent);
        outline-offset: 1px;
    }

    button:disabled {
        opacity: 0.38;
        cursor: default;
    }

    .icon-button {
        display: flex;
        align-items: center;
        justify-content: center;
        inline-size: 2rem;
        block-size: 1.85rem;
        padding: 0;
        color: var(--paper-dim);
    }

    .icon-button:hover:not(:disabled) {
        color: var(--paper);
    }


</style>
