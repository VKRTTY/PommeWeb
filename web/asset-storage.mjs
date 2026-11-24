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
