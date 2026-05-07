// idb-schema.js — shared IndexedDB open/purge for offline reading.
// Loaded as a classic script from reader.html and importScripts'd from sw.js.
// Exposes globals: openOfflineDb, purgeStaleVersionRows.

(function (root) {
  const DB_NAME = "ropds-offline";
  const DB_VERSION = 1;
  const STORE = "books";

  function openOfflineDb() {
    return new Promise(function (resolve, reject) {
      const req = indexedDB.open(DB_NAME, DB_VERSION);
      req.onupgradeneeded = function () {
        const db = req.result;
        if (!db.objectStoreNames.contains(STORE)) {
          db.createObjectStore(STORE, { keyPath: "book_id" });
        }
      };
      req.onsuccess = function () { resolve(req.result); };
      req.onerror = function () { reject(req.error); };
    });
  }

  function purgeStaleVersionRows(db, currentVersion) {
    return new Promise(function (resolve) {
      if (!db.objectStoreNames.contains(STORE)) { resolve(0); return; }
      const tx = db.transaction(STORE, "readwrite");
      const store = tx.objectStore(STORE);
      const cursorReq = store.openCursor();
      let deleted = 0;
      cursorReq.onsuccess = function () {
        const cursor = cursorReq.result;
        if (!cursor) return;
        if (cursor.value && cursor.value.app_version !== currentVersion) {
          cursor.delete();
          deleted++;
        }
        cursor.continue();
      };
      tx.oncomplete = function () { resolve(deleted); };
      tx.onerror = function () { resolve(deleted); };
    });
  }

  root.openOfflineDb = openOfflineDb;
  root.purgeStaleVersionRows = purgeStaleVersionRows;
  root.OFFLINE_DB_NAME = DB_NAME;
  root.OFFLINE_DB_STORE = STORE;
})(typeof self !== "undefined" ? self : window);
