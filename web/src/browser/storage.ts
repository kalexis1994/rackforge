/**
 * Keeps the host's storage across visits.
 *
 * A native RackForge writes to a directory that is still there tomorrow. The
 * browser host writes to a filesystem that lives in the audio thread's memory,
 * so without somewhere to put it, every installed plugin, saved preset and
 * edited Rack would last until the tab closed.
 *
 * IndexedDB is that somewhere. It is the only storage a page can rely on
 * having: the Origin Private File System would be a closer match, but its
 * synchronous handles are unavailable in an audio worklet, which is exactly
 * where the host runs. So the worklet reports what it has written, the page
 * files it here, and the next visit boots from it.
 *
 * The bundled packages RackForge ships with the site are not stored: they come
 * from the site itself, and a copy here would go stale the moment the site is
 * rebuilt.
 */

import type { SeedFile } from "./protocol";

const DATABASE = "rackforge-storage";
const STORE = "files";
const VERSION = 1;

function open(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DATABASE, VERSION);
    request.onupgradeneeded = () => {
      const database = request.result;
      if (!database.objectStoreNames.contains(STORE)) {
        // Keyed by path below the RackForge data root.
        database.createObjectStore(STORE);
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error ?? new Error("IndexedDB is unavailable"));
  });
}

function settle(transaction: IDBTransaction): Promise<void> {
  return new Promise((resolve, reject) => {
    transaction.oncomplete = () => resolve();
    transaction.onabort = () => reject(transaction.error ?? new Error("storage write aborted"));
    transaction.onerror = () => reject(transaction.error ?? new Error("storage write failed"));
  });
}

/** Reads everything the host wrote on earlier visits. */
export async function readStoredFiles(): Promise<SeedFile[]> {
  try {
    const database = await open();
    try {
      const store = database.transaction(STORE, "readonly").objectStore(STORE);
      const [paths, values] = await Promise.all([
        request<IDBValidKey[]>(store.getAllKeys()),
        request<ArrayBuffer[]>(store.getAll()),
      ]);
      return paths.map((path, index) => ({
        path: String(path),
        bytes: new Uint8Array(values[index]),
      }));
    } finally {
      database.close();
    }
  } catch {
    // A private window, a full disk or a browser without IndexedDB: the host
    // still runs, it just starts from the packaged storage every time.
    return [];
  }
}

/**
 * Replaces the stored copy with what the host currently has.
 *
 * The whole set is written rather than a difference, because the host is the
 * only writer and its storage is small — packages, presets and a performance
 * library. Anything no longer present is removed, so an uninstall sticks.
 */
export async function writeStoredFiles(files: SeedFile[]): Promise<void> {
  const database = await open();
  try {
    const transaction = database.transaction(STORE, "readwrite");
    const store = transaction.objectStore(STORE);
    store.clear();
    for (const file of files) {
      // Stored as a plain buffer: a view would keep the whole underlying
      // memory alive.
      store.put(file.bytes.slice().buffer, file.path);
    }
    await settle(transaction);
  } finally {
    database.close();
  }
}

/** Forgets everything, so the next visit starts from the packaged storage. */
export async function clearStoredFiles(): Promise<void> {
  const database = await open();
  try {
    const transaction = database.transaction(STORE, "readwrite");
    transaction.objectStore(STORE).clear();
    await settle(transaction);
  } finally {
    database.close();
  }
}

/**
 * Asks the browser not to evict this storage when space runs short.
 *
 * Browsers grant this to sites people have engaged with, and refuse it
 * elsewhere; either way RackForge keeps working, so the answer is only worth
 * reporting, not acting on.
 */
export async function requestPersistentStorage(): Promise<boolean> {
  try {
    return (await navigator.storage?.persist?.()) ?? false;
  } catch {
    return false;
  }
}

function request<T>(source: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    source.onsuccess = () => resolve(source.result);
    source.onerror = () => reject(source.error ?? new Error("storage read failed"));
  });
}
