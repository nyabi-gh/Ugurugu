import {
    deleteRecoveryRecord,
    newRecoveryKey,
    readRecoveryRecords,
    writeRecoveryRecord,
} from "./RecoveryStore";
import type { RecoveryRecord } from "./RecoveryStore";

export interface DocumentCapture {
    document: string;
    revision: number;
    name: string;
    bytes: ArrayBuffer;
}

interface AutosaveHost {
    // Whether a snapshot may be taken at all: there has to be a document, and
    // a stroke in progress must not be interrupted by a serialize.
    ready: () => boolean;
    document: () => string | null;
    // Bumped by every change worth saving.
    revision: () => number;
    // Serializes in the shell's operation queue, so the bytes, the document
    // they belong to, its revision and its name are taken together.
    capture: () => Promise<DocumentCapture | null>;
    // Opens the bytes as the document with this id. Resolves false when the
    // open failed or the artist declined to replace their document.
    open: (bytes: ArrayBuffer, name: string, document: string) => Promise<boolean>;
}

const sessionLockPrefix = "ugurugu-session:";

// The IndexedDB recovery records: one per document, written on an interval
// and when the tab is hidden. Records of sessions that are gone are offered
// back; a session holds a Web Lock for as long as its tab lives, which is how
// another tab tells a live document from an abandoned one.
export class AutosaveController {
    offers = $state<RecoveryRecord[]>([]);
    status = $state("");

    readonly session = newRecoveryKey();
    #host: AutosaveHost;
    #saved: { document: string; revision: number } | null = null;
    #busy = false;
    #restoring = false;
    // Writes and deletes run one at a time in the order they were asked for,
    // and a document that was let go is never written again, so a snapshot
    // that lands late cannot bring it back.
    #writes = Promise.resolve();
    #forgotten = new Set<string>();

    constructor(host: AutosaveHost) {
        this.#host = host;
    }

    get offer(): RecoveryRecord | null {
        return this.offers[0] ?? null;
    }

    #store(operation: () => Promise<void>): Promise<void> {
        const result = this.#writes.then(operation);
        this.#writes = result.catch(() => undefined);
        return result;
    }

    async readOffers() {
        try {
            const records = await readRecoveryRecords();
            const live = await liveSessions();
            this.offers = records
                .filter(
                    (record) =>
                        record.session !== this.session &&
                        !(record.session && live?.has(record.session)),
                )
                .sort((a, b) => b.savedAt - a.savedAt);
        } catch (error) {
            this.status = `Could not read the recovery slot — ${error}`;
        }
    }

    async snapshot() {
        const document = this.#host.document();
        if (
            !this.#host.ready() ||
            this.#busy ||
            document === null ||
            (this.#saved?.document === document &&
                this.#saved.revision === this.#host.revision())
        ) {
            return;
        }
        this.#busy = true;
        try {
            const capture = await this.#host.capture();
            if (!capture) {
                return;
            }
            await this.#store(async () => {
                if (this.#forgotten.has(capture.document)) {
                    return;
                }
                await writeRecoveryRecord({
                    key: capture.document,
                    session: this.session,
                    name: capture.name,
                    bytes: capture.bytes,
                    savedAt: Date.now(),
                });
            });
            this.#saved = {
                document: capture.document,
                revision: capture.revision,
            };
            const time = new Date().toLocaleTimeString();
            this.status = `Recovery snapshot saved ${time}`;
        } catch (error) {
            const detail =
                error instanceof Error
                    ? `${error.name}: ${error.message}`
                    : String(error);
            this.status = `Recovery save failed — ${detail}`;
        } finally {
            this.#busy = false;
        }
    }

    // The offer stays until its document is open: a restore that fails or is
    // declined leaves it to be tried again or discarded.
    async restore() {
        const offer = this.offer;
        if (!offer || this.#restoring) {
            return;
        }
        this.#restoring = true;
        try {
            // A copy, because the open transfers its buffer to the worker.
            if (!(await this.#host.open(offer.bytes.slice(0), offer.name, offer.key))) {
                return;
            }
            this.offers = this.offers.filter((record) => record !== offer);
            this.#saved = { document: offer.key, revision: 0 };
            // The record now belongs to this session, so no other tab offers
            // the document while it is open here.
            await this.#store(() =>
                writeRecoveryRecord({ ...offer, session: this.session }),
            );
        } catch (error) {
            this.status = `Could not take over the recovery record — ${error}`;
        } finally {
            this.#restoring = false;
        }
    }

    async discard() {
        const offer = this.offer;
        if (!offer) {
            return;
        }
        this.offers = this.offers.filter((record) => record !== offer);
        try {
            await this.forget(offer.key);
        } catch (error) {
            this.status = `Could not clear the recovery slot — ${error}`;
        }
    }

    // For a document the artist let go of: its record is deleted, and any
    // snapshot of it still in flight is dropped.
    forget(document: string): Promise<void> {
        this.#forgotten.add(document);
        return this.#store(() => deleteRecoveryRecord(document));
    }

    // Starts the interval and the hidden-tab hook; the returned function stops
    // both.
    start(): () => void {
        void navigator.locks?.request(
            sessionLockPrefix + this.session,
            () => new Promise<never>(() => {}),
        );
        const timer = setInterval(() => {
            void this.snapshot();
        }, autosaveIntervalMs());
        const onHidden = () => {
            if (document.visibilityState === "hidden") {
                void this.snapshot();
            }
        };
        document.addEventListener("visibilitychange", onHidden);
        return () => {
            clearInterval(timer);
            document.removeEventListener("visibilitychange", onHidden);
        };
    }
}

// null when the browser cannot tell, in which case every other session's
// record is offered.
async function liveSessions(): Promise<Set<string> | null> {
    if (!navigator.locks) {
        return null;
    }
    const { held = [] } = await navigator.locks.query();
    const sessions = new Set<string>();
    for (const lock of held) {
        if (lock.name?.startsWith(sessionLockPrefix)) {
            sessions.add(lock.name.slice(sessionLockPrefix.length));
        }
    }
    return sessions;
}

// Reload-safety knob: the interval is short because a browser tab can go away
// without any reliable shutdown callback. Tests pass ?autosave=1.
export function autosaveIntervalMs() {
    const parameter = new URLSearchParams(window.location.search).get(
        "autosave",
    );
    const seconds = Number(parameter);
    if (!Number.isFinite(seconds) || seconds <= 0) {
        return 15000;
    }
    return Math.min(600, Math.max(1, seconds)) * 1000;
}
