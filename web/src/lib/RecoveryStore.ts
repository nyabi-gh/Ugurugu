// One record per document, keyed by the document's id, so two tabs or two
// documents in one tab never write over each other. session names the tab
// that owns the record; records written before sessions existed have none.
export interface RecoveryRecord {
    key: string;
    session: string | null;
    name: string;
    bytes: ArrayBuffer;
    savedAt: number;
}

// crypto.randomUUID exists only in secure contexts; a dev server reached over
// plain http on a LAN address is not one.
export function newRecoveryKey(): string {
    const bytes = crypto.getRandomValues(new Uint8Array(16));
    return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

const databaseName = "ugurugu-web";
const storeName = "recovery";

function openDatabase(): Promise<IDBDatabase> {
    if (typeof indexedDB === "undefined") {
        return Promise.reject(
            new Error("IndexedDB is unavailable in this browser"),
        );
    }
    return new Promise((resolve, reject) => {
        const request = indexedDB.open(databaseName, 1);
        request.onupgradeneeded = () => {
            if (!request.result.objectStoreNames.contains(storeName)) {
                request.result.createObjectStore(storeName);
            }
        };
        request.onsuccess = () => resolve(request.result);
        request.onerror = () =>
            reject(request.error ?? new Error("IndexedDB open failed"));
        request.onblocked = () =>
            reject(new Error("IndexedDB open is blocked by another tab"));
    });
}

// operation issues its requests and returns how to read the result once the
// transaction has committed; any failed request aborts the whole transaction.
async function withStore<T>(
    mode: IDBTransactionMode,
    operation: (store: IDBObjectStore) => () => T,
): Promise<T> {
    const database = await openDatabase();
    try {
        const transaction = database.transaction(storeName, mode);
        const result = operation(transaction.objectStore(storeName));
        await new Promise<void>((resolve, reject) => {
            transaction.oncomplete = () => resolve();
            transaction.onabort = () =>
                reject(
                    transaction.error ?? new Error("IndexedDB write aborted"),
                );
            transaction.onerror = () =>
                reject(
                    transaction.error ?? new Error("IndexedDB write failed"),
                );
        });
        return result();
    } finally {
        database.close();
    }
}

function toRecord(key: IDBValidKey, value: unknown): RecoveryRecord | null {
    if (!value || typeof value !== "object") {
        return null;
    }
    const record = value as Partial<RecoveryRecord>;
    if (!(record.bytes instanceof ArrayBuffer)) {
        return null;
    }
    return {
        key: String(key),
        session: typeof record.session === "string" ? record.session : null,
        name: typeof record.name === "string" ? record.name : "Untitled.ugu",
        bytes: record.bytes,
        savedAt: typeof record.savedAt === "number" ? record.savedAt : 0,
    };
}

export function readRecoveryRecords(): Promise<RecoveryRecord[]> {
    return withStore("readonly", (store) => {
        const records: RecoveryRecord[] = [];
        const request = store.openCursor();
        request.onsuccess = () => {
            const cursor = request.result;
            if (!cursor) {
                return;
            }
            const record = toRecord(cursor.key, cursor.value);
            if (record) {
                records.push(record);
            }
            cursor.continue();
        };
        return () => records;
    });
}

export function writeRecoveryRecord(record: RecoveryRecord): Promise<void> {
    const { key, ...value } = record;
    return withStore("readwrite", (store) => {
        store.put(value, key);
        return () => undefined;
    });
}

export function deleteRecoveryRecord(key: string): Promise<void> {
    return withStore("readwrite", (store) => {
        store.delete(key);
        return () => undefined;
    });
}
