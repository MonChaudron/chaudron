// Copie de travail sur l'appareil : IndexedDB sur le web (une petite table clé → valeur, sans bibliothèque).
// Sans IndexedDB (navigation privée, rendu serveur), une mémoire le temps de la session.

const DB = 'chaudron';
const STORE = 'kv';

let opening: Promise<IDBDatabase | null> | null = null;
const memory = new Map<string, unknown>();

function open(): Promise<IDBDatabase | null> {
  if (opening) return opening;
  opening = new Promise((resolve) => {
    if (typeof indexedDB === 'undefined') return resolve(null);
    try {
      const request = indexedDB.open(DB, 1);
      request.onupgradeneeded = () => request.result.createObjectStore(STORE);
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => resolve(null);
    } catch {
      resolve(null);
    }
  });
  return opening;
}

export async function getItem<T>(key: string): Promise<T | undefined> {
  const db = await open();
  if (!db) return memory.get(key) as T | undefined;
  return new Promise((resolve) => {
    const request = db.transaction(STORE).objectStore(STORE).get(key);
    request.onsuccess = () => resolve(request.result as T | undefined);
    request.onerror = () => resolve(undefined);
  });
}

/** Efface toute la copie de l'appareil (déconnexion : rien du foyer ne reste sur un appareil partagé). */
export async function clearAll(): Promise<void> {
  memory.clear();
  const db = await open();
  if (!db) return;
  return new Promise((resolve) => {
    const tx = db.transaction(STORE, 'readwrite');
    tx.objectStore(STORE).clear();
    tx.oncomplete = () => resolve();
    tx.onerror = () => resolve();
  });
}

export async function setItem(key: string, value: unknown): Promise<void> {
  const db = await open();
  if (!db) {
    memory.set(key, value);
    return;
  }
  return new Promise((resolve) => {
    const tx = db.transaction(STORE, 'readwrite');
    tx.objectStore(STORE).put(value, key);
    tx.oncomplete = () => resolve();
    tx.onerror = () => resolve();
  });
}
