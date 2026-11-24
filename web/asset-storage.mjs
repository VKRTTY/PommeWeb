// Installer storage. A record is committed atomically by IndexedDB; extraction
// writes its completion marker only after all asset transactions have committed.
let database;

function openDatabase() {
  if (!database) database = new Promise((resolve, reject) => {
    const request = indexedDB.open('pomme-client-assets', 1);
    request.onupgradeneeded = () => request.result.createObjectStore('files');
    request.onsuccess = () => {
      const db = request.result;
      db.onversionchange = () => { db.close(); database = undefined; };
      resolve(db);
    };
    request.onerror = () => { database = undefined; reject(request.error); };
  });
  return database;
}

async function operation(mode, run) {
  const db = await openDatabase();
  return new Promise((resolve, reject) => {
    const transaction = db.transaction('files', mode);
    const request = run(transaction.objectStore('files'));
    transaction.oncomplete = () => resolve(request.result);
    transaction.onabort = () => reject(transaction.error || new Error('Asset transaction aborted'));
  });
}

export function assetRead(path) {
  return operation('readonly', store => store.get(path));
}

export async function assetHas(path) {
  return (await operation('readonly', store => store.getKey(path))) !== undefined;
}

export async function assetWrite(path, bytes) {
  await operation('readwrite', store => store.put(bytes, path));
}

/// Every stored file but the asset index's objects, as [path, bytes, ...]:
/// what the game's in-memory file tree starts with.
export async function assetPreload() {
  const db = await openDatabase();
  return new Promise((resolve, reject) => {
    const out = [];
    const transaction = db.transaction('files', 'readonly');
    const request = transaction.objectStore('files').openCursor();
    request.onsuccess = () => {
      const cursor = request.result;
      if (!cursor) return;
      if (!String(cursor.key).includes('/assets/objects/')) out.push(cursor.key, cursor.value);
      cursor.continue();
    };
    transaction.oncomplete = () => resolve(out);
    transaction.onabort = () => reject(transaction.error || new Error('Asset preload aborted'));
  });
}

/// Removes `path` and everything under it.
export async function assetRemove(path) {
  await operation('readwrite', store => {
    store.delete(path);
    return store.delete(IDBKeyRange.bound(path + '/', path + '/￿'));
  });
}

/// The first stored file whose path ends with `suffix`, or undefined:
/// found among the keys alone, then read.
export async function assetFind(suffix) {
  const keys = await operation('readonly', store => store.getAllKeys());
  const key = keys.find(k => String(k).endsWith(suffix));
  return key === undefined ? undefined : assetRead(key);
}
