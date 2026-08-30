import type { ChatMessage } from "./chatTypes";

const DB_NAME = "vibearound-web-chat";
const DB_VERSION = 1;
const STORE_NAME = "session-transcripts";
const CACHE_SCHEMA_VERSION = 2;
const MAX_CACHED_SESSIONS = 20;
const MAX_CACHED_BYTES = 256 * 1024 * 1024;
const KEY_SEPARATOR = "\u001f";

interface CacheKeyParts {
  agentId: string;
  workspace: string;
  sessionId: string;
}

interface CacheReadRequest extends CacheKeyParts {
  updatedAt: number;
}

interface CacheWriteRequest extends CacheReadRequest {
  messages: ChatMessage[];
}

interface CachedSessionTranscript {
  key: string;
  schemaVersion?: number;
  agentId: string;
  workspace: string;
  sessionId: string;
  updatedAt: number;
  cachedAt: number;
  payloadBytes?: number;
  messages: ChatMessage[];
}

function cacheKey({ agentId, workspace, sessionId }: CacheKeyParts) {
  return [agentId, workspace, sessionId].join(KEY_SEPARATOR);
}

function estimatePayloadBytes(value: unknown) {
  const json = JSON.stringify(value);
  if (!json) return 0;
  if (typeof Blob !== "undefined") {
    return new Blob([json]).size;
  }
  return json.length;
}

function entryPayloadBytes(entry: CachedSessionTranscript) {
  return entry.payloadBytes ?? estimatePayloadBytes(entry);
}

function requestResult<T>(request: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error ?? new Error("IndexedDB request failed"));
  });
}

function transactionDone(transaction: IDBTransaction): Promise<void> {
  return new Promise((resolve, reject) => {
    transaction.oncomplete = () => resolve();
    transaction.onabort = () =>
      reject(transaction.error ?? new Error("IndexedDB transaction aborted"));
    transaction.onerror = () =>
      reject(transaction.error ?? new Error("IndexedDB transaction failed"));
  });
}

function openSessionCache(): Promise<IDBDatabase | null> {
  if (typeof window === "undefined" || !("indexedDB" in window)) {
    return Promise.resolve(null);
  }

  return new Promise((resolve, reject) => {
    const request = window.indexedDB.open(DB_NAME, DB_VERSION);
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(STORE_NAME)) {
        db.createObjectStore(STORE_NAME, { keyPath: "key" });
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error ?? new Error("Failed to open IndexedDB"));
  });
}

async function withStore<T>(
  mode: IDBTransactionMode,
  callback: (store: IDBObjectStore) => Promise<T>,
): Promise<T | null> {
  const db = await openSessionCache();
  if (!db) return null;
  try {
    const transaction = db.transaction(STORE_NAME, mode);
    const store = transaction.objectStore(STORE_NAME);
    const result = await callback(store);
    await transactionDone(transaction);
    return result;
  } finally {
    db.close();
  }
}

async function pruneCachedSessions(store: IDBObjectStore, protectedKey?: string) {
  const entries = await requestResult<CachedSessionTranscript[]>(store.getAll());
  const totalBytes = entries.reduce((sum, entry) => sum + entryPayloadBytes(entry), 0);
  if (entries.length <= MAX_CACHED_SESSIONS && totalBytes <= MAX_CACHED_BYTES) return;

  const keptKeys = new Set<string>();
  let keptBytes = 0;
  const sortedEntries = [...entries].sort((a, b) => {
    if (a.key === protectedKey) return -1;
    if (b.key === protectedKey) return 1;
    return b.cachedAt - a.cachedAt;
  });

  sortedEntries.forEach((entry) => {
    const bytes = entryPayloadBytes(entry);
    const keepProtected = entry.key === protectedKey;
    const keepWithinBudget =
      keptKeys.size < MAX_CACHED_SESSIONS && keptBytes + bytes <= MAX_CACHED_BYTES;
    if (keepProtected || keepWithinBudget) {
      keptKeys.add(entry.key);
      keptBytes += bytes;
    }
  });

  entries
    .filter((entry) => !keptKeys.has(entry.key))
    .forEach((entry) => {
      store.delete(entry.key);
    });
}

export interface CachedChatSession {
  messages: ChatMessage[];
  /// The stamp this transcript last synced to; reported to the server so it
  /// can decide between a silent resume and a replay.
  updatedAt: number;
}

export async function readCachedChatSession({
  agentId,
  workspace,
  sessionId,
  updatedAt,
}: CacheReadRequest): Promise<CachedChatSession | null> {
  const key = cacheKey({ agentId, workspace, sessionId });
  return withStore("readwrite", async (store) => {
    const entry = await requestResult<CachedSessionTranscript | undefined>(store.get(key));
    if (
      !entry ||
      entry.schemaVersion !== CACHE_SCHEMA_VERSION ||
      entry.updatedAt < updatedAt
    ) {
      return null;
    }
    store.put({ ...entry, cachedAt: Date.now() });
    return { messages: entry.messages, updatedAt: entry.updatedAt };
  });
}

export async function writeCachedChatSession({
  agentId,
  workspace,
  sessionId,
  updatedAt,
  messages,
}: CacheWriteRequest): Promise<void> {
  const key = cacheKey({ agentId, workspace, sessionId });
  await withStore("readwrite", async (store) => {
    const entry = {
      key,
      schemaVersion: CACHE_SCHEMA_VERSION,
      agentId,
      workspace,
      sessionId,
      updatedAt,
      cachedAt: Date.now(),
      messages,
    } satisfies CachedSessionTranscript;
    store.put({
      ...entry,
      payloadBytes: estimatePayloadBytes(entry),
    } satisfies CachedSessionTranscript);
    await pruneCachedSessions(store, key);
  });
}

export async function deleteCachedChatSession({
  agentId,
  workspace,
  sessionId,
}: CacheKeyParts): Promise<void> {
  const key = cacheKey({ agentId, workspace, sessionId });
  await withStore("readwrite", async (store) => {
    store.delete(key);
  });
}
