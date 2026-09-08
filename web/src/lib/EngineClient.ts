export interface LayerInfo {
    // null inherits the whole drawing; otherwise this is the layer override.
    wobble: WobbleSettings | null;
    index: number;
    // Stable identity. Row indexes shift under every add, remove and move, so
    // anything queued has to name the layer by this instead.
    id: string;
    name: string;
    group: boolean;
    visible: boolean;
    reference: boolean;
    opacity: number;
    active: boolean;
    depth: number;
    blendMode: LayerBlendMode;
    clipped: boolean;
    // MergeLayerDownStatus: 0 means the merge would go through, anything else
    // is the reason it would not.
    mergeStatus: number;
}

export const layerBlendModes = ["Normal", "Multiply", "Screen", "Overlay"];

export type LayerBlendMode = 0 | 1 | 2 | 3;

// Mirrors DocumentController::MergeLayerDownStatus.
export const mergeBlockReasons: Record<number, string> = {
    1: "There is no such layer.",
    2: "There is no paint layer below this one.",
    3: "The layers have properties that would change the picture if merged.",
    4: "This layer has strokes that cannot be merged.",
    5: "The layers were drawn on different canvas sizes.",
    6: "Merging would exceed the stroke limit.",
};

export interface BrushPresetInfo {
    index: number;
    name: string;
    defaultSize: number;
}

export interface LayerThumbnail {
    index: number;
    width: number;
    height: number;
    pixels: Uint8ClampedArray<ArrayBuffer> | null;
}

// Document.motion plus Document.wobbleAmount, the fields the wobble panel
// edits as one unit.
export interface WobbleSettings {
    amount: number;
    style: number;
    poseCount: number;
    detail: number;
    linked: number;
    randomness: number;
    brokenLine: boolean;
    breakAmount: number;
    breakRange: number;
}

export interface DocumentMeta {
    abiVersion: number;
    schemaVersion: number;
    width: number;
    height: number;
    frameCount: number;
    layerCount: number;
    fps: number;
    wobble: WobbleSettings;
    presets: BrushPresetInfo[];
    eraserPresets: BrushPresetInfo[];
    layers: LayerInfo[];
    canUndo: boolean;
    canRedo: boolean;
}

// One closed ring of the selection boundary in document coordinates, as a
// flat x, y list ready for Path2D.
export type SelectionContour = Float32Array<ArrayBuffer>;

export interface SelectionUpdate {
    revision: number;
    active: boolean;
    // Absent when the outline has not changed since the last reply, so the
    // shell keeps the contours it already holds.
    contours: SelectionContour[] | null;
}

export interface RegionUpdate {
    rect: { x: number; y: number; width: number; height: number };
    pixels: Uint8ClampedArray<ArrayBuffer> | null;
    layers: LayerInfo[];
    selection: SelectionUpdate;
    // Only when the operation changed a document property the shell mirrors.
    meta: DocumentMeta | null;
    canUndo: boolean;
    canRedo: boolean;
}

export type SelectionShape = 0 | 1 | 2;
export type SelectionCombine = 0 | 1 | 2;

export interface FillOptions {
    reference: number;
    comparison: number;
    tolerance: number;
    antialiasing: boolean;
}

export interface BrushSettings {
    red: number;
    green: number;
    blue: number;
    alpha: number;
    width: number;
    erase: boolean;
}

interface PendingRequest {
    resolve: (value: unknown) => void;
    reject: (reason: Error) => void;
}

interface RegionResponse {
    rect: { x: number; y: number; width: number; height: number };
    pixels: ArrayBuffer | null;
    layers: LayerInfo[];
    selection: { revision: number; active: boolean; outline: ArrayBuffer | null };
    meta: DocumentMeta | null;
    canUndo: boolean;
    canRedo: boolean;
}

// The engine packs every contour into one buffer: a vertex count, then that
// many x, y pairs, repeated to the end.
function splitContours(outline: ArrayBuffer): SelectionContour[] {
    const values = new Float32Array(outline);
    const contours: SelectionContour[] = [];
    let index = 0;
    while (index < values.length) {
        const vertices = values[index] ?? 0;
        index += 1;
        const span = vertices * 2;
        if (span <= 0 || index + span > values.length) {
            break;
        }
        contours.push(values.slice(index, index + span));
        index += span;
    }
    return contours;
}

export class EngineClient {
    #worker: Worker;
    #pending = new Map<number, PendingRequest>();
    #nextId = 0;
    // Set once the worker is known to be dead. Every later request fails fast
    // with the same reason instead of waiting for a reply that cannot come.
    #failure: Error | null = null;

    constructor() {
        // Relative to the page so the worker stays same-origin under
        // itch.io's project subdirectory as well as the dev server.
        this.#worker = new Worker(
            new URL("engine/engine-worker.js", document.baseURI),
        );
        this.#worker.onmessage = (event) => {
            const { id, ok, error } = event.data;
            const pending = this.#pending.get(id);
            if (!pending) {
                return;
            }
            this.#pending.delete(id);
            if (ok) {
                pending.resolve(event.data);
            } else {
                pending.reject(new Error(error));
            }
        };
        // A worker that fails to load — a missing or blocked engine artifact,
        // an exception before its onmessage handler is installed — never
        // answers anything. Without this the shell sat on "Loading engine…"
        // for as long as the tab was open, with no way to tell why.
        this.#worker.onerror = (event) => {
            this.#fail(
                new Error(
                    typeof event === "string" || !event.message
                        ? "the drawing engine could not be loaded"
                        : event.message,
                ),
            );
        };
        this.#worker.onmessageerror = () => {
            this.#fail(
                new Error("a reply from the drawing engine could not be read"),
            );
        };
    }

    #fail(error: Error) {
        this.#failure ??= error;
        const waiting = [...this.#pending.values()];
        this.#pending.clear();
        for (const pending of waiting) {
            pending.reject(this.#failure);
        }
    }

    #request<T>(
        message: Record<string, unknown>,
        transfer: Transferable[] = [],
    ): Promise<T> {
        if (this.#failure) {
            return Promise.reject(this.#failure);
        }
        const id = this.#nextId;
        this.#nextId += 1;
        return new Promise<T>((resolve, reject) => {
            this.#pending.set(id, {
                resolve: resolve as (value: unknown) => void,
                reject,
            });
            this.#worker.postMessage({ id, ...message }, transfer);
        });
    }

    static #toRegion(response: RegionResponse): RegionUpdate {
        return {
            rect: response.rect,
            pixels: response.pixels
                ? new Uint8ClampedArray(response.pixels)
                : null,
            layers: response.layers,
            selection: {
                revision: response.selection.revision,
                active: response.selection.active,
                contours: response.selection.outline
                    ? splitContours(response.selection.outline)
                    : null,
            },
            meta: response.meta ?? null,
            canUndo: response.canUndo,
            canRedo: response.canRedo,
        };
    }

    async #regionRequest(
        message: Record<string, unknown>,
    ): Promise<RegionUpdate> {
        const response = await this.#request<RegionResponse>(message);
        return EngineClient.#toRegion(response);
    }

    async open(bytes: ArrayBuffer, undoLimit: number): Promise<DocumentMeta> {
        const response = await this.#request<{ meta: DocumentMeta }>(
            { type: "open", bytes, undoLimit },
            [bytes],
        );
        return response.meta;
    }

    async create(
        width: number,
        height: number,
        undoLimit: number,
    ): Promise<DocumentMeta> {
        const response = await this.#request<{ meta: DocumentMeta }>({
            type: "create",
            width,
            height,
            undoLimit,
        });
        return response.meta;
    }

    renderFrame(frame: number): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "render", frame });
    }

    async setBrush(brush: BrushSettings): Promise<void> {
        await this.#request({ type: "brush", ...brush });
    }

    async setBrushPreset(index: number): Promise<void> {
        await this.#request({ type: "brushPreset", index });
    }

    async setEraserPreset(index: number): Promise<void> {
        await this.#request({ type: "eraserPreset", index });
    }

    async setBrushAntialiasing(antialiasing: boolean): Promise<void> {
        await this.#request({ type: "brushAntialiasing", antialiasing });
    }

    async setStabilization(strength: number): Promise<void> {
        await this.#request({ type: "stabilization", strength });
    }

    async setFillOptions(options: FillOptions): Promise<void> {
        await this.#request({ type: "fillOptions", ...options });
    }

    layerWobble(
        frame: number,
        index: number,
        wobble: WobbleSettings | null,
    ): Promise<RegionUpdate> {
        return this.#regionRequest({
            type: "layerWobble",
            frame,
            index,
            wobble,
        });
    }

    text(
        frame: number,
        index: number,
        commands: number[],
        x: number,
        y: number,
        width: number,
        filled: boolean,
        color: number,
    ): Promise<RegionUpdate> {
        return this.#regionRequest({
            type: "text",
            frame,
            index,
            commands,
            x,
            y,
            width,
            filled,
            color,
        });
    }

    async insertImage(
        frame: number,
        pixels: ArrayBuffer,
        width: number,
        height: number,
        name: string,
    ): Promise<RegionUpdate> {
        const response = await this.#request<RegionResponse>(
            { type: "insertImage", frame, pixels, width, height, name },
            [pixels],
        );
        return EngineClient.#toRegion(response);
    }

    bucketFill(frame: number, x: number, y: number): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "bucketFill", frame, x, y });
    }

    // points is a flat x, y list: the traced path for a freehand lasso, or the
    // two drag corners for a rectangle or ellipse.
    selectionShape(
        frame: number,
        shape: SelectionShape,
        points: number[],
        combine: SelectionCombine,
        paint: boolean,
    ): Promise<RegionUpdate> {
        return this.#regionRequest({
            type: "selectionShape",
            frame,
            shape,
            points,
            combine,
            paint,
        });
    }

    selectionFlood(
        frame: number,
        x: number,
        y: number,
        combine: SelectionCombine,
    ): Promise<RegionUpdate> {
        return this.#regionRequest({
            type: "selectionFlood",
            frame,
            x,
            y,
            combine,
        });
    }

    selectAll(): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "selectionAll" });
    }

    invertSelection(): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "selectionInvert" });
    }

    deselect(): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "selectionClear" });
    }

    fillSelection(frame: number): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "selectionFill", frame });
    }

    deleteSelection(frame: number): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "selectionDelete", frame });
    }

    // Lifts the selected pixels off the layer. Until apply or cancel the frame
    // shows them floating at the pending matrix and the document is unchanged,
    // so a whole move-scale-rotate gesture costs one undo entry.
    selectionTransformBegin(frame: number): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "selectionTransformBegin", frame });
    }

    // matrix is QTransform's row-vector layout: m11, m12, m21, m22, dx, dy.
    selectionTransformUpdate(
        matrix: [number, number, number, number, number, number],
    ): Promise<RegionUpdate> {
        return this.#regionRequest({
            type: "selectionTransformUpdate",
            matrix,
        });
    }

    selectionTransformApply(): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "selectionTransformApply" });
    }

    selectionTransformCancel(): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "selectionTransformCancel" });
    }

    strokeBegin(
        frame: number,
        x: number,
        y: number,
        pressure: number,
        timestamp: number,
    ): Promise<RegionUpdate> {
        return this.#regionRequest({
            type: "strokeBegin",
            frame,
            x,
            y,
            pressure,
            timestamp,
        });
    }

    strokeAppend(frame: number, points: number[]): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "strokeAppend", frame, points });
    }

    strokeEnd(frame: number): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "strokeEnd", frame });
    }

    undo(frame: number): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "undo", frame });
    }

    redo(frame: number): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "redo", frame });
    }

    layerActivate(frame: number, index: number): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "layerActivate", frame, index });
    }

    layerVisible(
        frame: number,
        index: number,
        visible: boolean,
    ): Promise<RegionUpdate> {
        return this.#regionRequest({
            type: "layerVisible",
            frame,
            index,
            visible,
        });
    }

    wobble(frame: number, wobble: WobbleSettings): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "wobble", frame, wobble });
    }

    animationFrames(frame: number, frames: number): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "animationFrames", frame, frames });
    }

    framesPerSecond(fps: number): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "framesPerSecond", fps });
    }

    resizeImage(
        frame: number,
        width: number,
        height: number,
    ): Promise<RegionUpdate> {
        return this.#regionRequest({
            type: "resizeImage",
            frame,
            width,
            height,
        });
    }

    resizeCanvas(
        frame: number,
        width: number,
        height: number,
        offsetX: number,
        offsetY: number,
    ): Promise<RegionUpdate> {
        return this.#regionRequest({
            type: "resizeCanvas",
            frame,
            width,
            height,
            offsetX,
            offsetY,
        });
    }

    layerReference(index: number, reference: boolean): Promise<RegionUpdate> {
        return this.#regionRequest({
            type: "layerReference",
            index,
            reference,
        });
    }

    layerOpacity(
        frame: number,
        index: number,
        opacity: number,
    ): Promise<RegionUpdate> {
        return this.#regionRequest({
            type: "layerOpacity",
            frame,
            index,
            opacity,
        });
    }

    layerAdd(frame: number): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "layerAdd", frame });
    }

    layerRemove(frame: number, index: number): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "layerRemove", frame, index });
    }

    layerRename(
        frame: number,
        index: number,
        name: string,
    ): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "layerRename", frame, index, name });
    }

    layerMove(
        frame: number,
        index: number,
        offset: number,
    ): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "layerMove", frame, index, offset });
    }

    // A negative index makes an empty group; otherwise the layer at index is
    // wrapped in the new one.
    layerAddGroup(frame: number, index: number): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "layerAddGroup", frame, index });
    }

    layerDuplicate(frame: number, index: number): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "layerDuplicate", frame, index });
    }

    layerClear(frame: number, index: number): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "layerClear", frame, index });
    }

    layerMergeDown(frame: number, index: number): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "layerMergeDown", frame, index });
    }

    layerBlendMode(
        frame: number,
        index: number,
        mode: LayerBlendMode,
    ): Promise<RegionUpdate> {
        return this.#regionRequest({
            type: "layerBlendMode",
            frame,
            index,
            mode,
        });
    }

    layerClipToBelow(
        frame: number,
        index: number,
        clipped: boolean,
    ): Promise<RegionUpdate> {
        return this.#regionRequest({
            type: "layerClipToBelow",
            frame,
            index,
            clipped,
        });
    }

    // A negative groupIndex moves the layer out to the top level.
    layerParentGroup(
        frame: number,
        index: number,
        groupIndex: number,
    ): Promise<RegionUpdate> {
        return this.#regionRequest({
            type: "layerParentGroup",
            frame,
            index,
            groupIndex,
        });
    }

    copySelection(frame: number): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "clipboardCopy", frame });
    }

    cutSelection(frame: number): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "clipboardCut", frame });
    }

    paste(frame: number): Promise<RegionUpdate> {
        return this.#regionRequest({ type: "clipboardPaste", frame });
    }

    async serialize(): Promise<ArrayBuffer> {
        const response = await this.#request<{ bytes: ArrayBuffer }>({
            type: "serialize",
        });
        return response.bytes;
    }

    async exportGif(): Promise<ArrayBuffer> {
        const response = await this.#request<{ bytes: ArrayBuffer }>({
            type: "exportGif",
        });
        return response.bytes;
    }

    async layerThumbnails(
        devicePixelRatio: number,
    ): Promise<LayerThumbnail[]> {
        const response = await this.#request<{
            thumbnails: Array<{
                index: number;
                width: number;
                height: number;
                pixels: ArrayBuffer | null;
            }>;
        }>({ type: "layerThumbnails", devicePixelRatio });
        return response.thumbnails.map((thumbnail) => ({
            index: thumbnail.index,
            width: thumbnail.width,
            height: thumbnail.height,
            pixels: thumbnail.pixels
                ? new Uint8ClampedArray(thumbnail.pixels)
                : null,
        }));
    }
}
