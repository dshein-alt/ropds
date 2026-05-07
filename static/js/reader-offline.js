// reader-offline.js — page-driven offline cache for the reader.
// Loaded as a classic script from reader.html. Reads body data-* for config.
// Public API on window.ROpdsOffline:
//   primeAndCacheBook({ blob, response }) — call after the reader fetches book bytes.
//   recordPositionLocal({ position, progress }) — call from savePosition before network.
//   markPositionSynced() — call after a successful network POST of position.
//   getLocalPosition() — returns null or { position, progress, ts } from localStorage.
//   resolveInitialPosition({ serverPosition, serverProgress, serverTs }) — picks local or server.
//   maybeWipeIfDisabled() — clears all offline state when data-offline-max="0".

(function () {
  const body = document.body;
  const ds = body.dataset;
  const APP_VERSION = ds.appVersion || (window.ROpdsAppVersion || "");
  const BOOK_ID = parseInt(ds.bookId, 10);
  const FORMAT = ds.format;
  const TITLE = ds.bookTitle || "";
  const AUTHORS = ds.bookAuthors || "";
  const MAX = parseInt(ds.offlineMax || "0", 10) || 0;

  const STATIC_CACHE = "ropds-static-v2-" + APP_VERSION;
  const BOOKS_CACHE = "ropds-books-v1-" + APP_VERSION;
  const POS_KEY = "ropds-pos-" + BOOK_ID;
  const HTML_URL = "/web/reader/" + BOOK_ID;
  const FILE_URL = "/web/read/" + BOOK_ID;
  const ACTIVE_CHANNEL = "ropds-active-books";

  // Format-specific runtime URLs that must be cached so the reader can boot offline.
  // Captured from Chrome DevTools Network panel by opening one book of each format
  // online with "Disable cache" enabled. EPUB pulls zip.js as its container vendor;
  // MOBI pulls fflate.js; FB2 reuses mobi.js for shared decode helpers.
  const FOLIATE_CORE = [
    "/static/lib/foliate/view.js",
    "/static/lib/foliate/epubcfi.js",
    "/static/lib/foliate/progress.js",
    "/static/lib/foliate/overlayer.js",
    "/static/lib/foliate/text-walker.js",
    "/static/lib/foliate/paginator.js",
  ];
  const URLS_FOR = {
    epub: FOLIATE_CORE.concat([
      "/static/lib/foliate/epub.js",
      "/static/lib/foliate/vendor/zip.js",
    ]),
    fb2: FOLIATE_CORE.concat([
      "/static/lib/foliate/fb2.js",
      "/static/lib/foliate/mobi.js",
    ]),
    mobi: FOLIATE_CORE.concat([
      "/static/lib/foliate/mobi.js",
      "/static/lib/foliate/vendor/fflate.js",
    ]),
    djvu: ["/static/lib/djvu/djvu.js", "/static/lib/djvu/djvu_viewer.js"],
  };

  const isOfflineEligible = function () {
    if (MAX <= 0) return false;
    if (FORMAT === "pdf") return false;
    if (!Number.isFinite(BOOK_ID)) return false;
    // Refuse to cache a book whose runtime asset list is empty — caching the bytes
    // without the runtime would produce a non-bootable offline copy.
    const urls = URLS_FOR[FORMAT];
    if (!urls || urls.length === 0) return false;
    return true;
  };

  const broadcast = (function () {
    try { return new BroadcastChannel(ACTIVE_CHANNEL); } catch (_e) { return null; }
  })();
  const liveBookIds = new Set([BOOK_ID]);
  if (broadcast) {
    broadcast.postMessage({ type: "open", book_id: BOOK_ID });
    broadcast.onmessage = function (e) {
      const m = e.data || {};
      if (m.type === "open" && Number.isFinite(m.book_id)) liveBookIds.add(m.book_id);
      if (m.type === "close" && Number.isFinite(m.book_id)) liveBookIds.delete(m.book_id);
    };
    window.addEventListener("pagehide", function () {
      try { broadcast.postMessage({ type: "close", book_id: BOOK_ID }); } catch (_e) {}
    });
  }

  async function evictOldest() {
    const db = await self.openOfflineDb();
    try {
      const rows = await new Promise(function (resolve) {
        const tx = db.transaction(self.OFFLINE_DB_STORE, "readonly");
        const req = tx.objectStore(self.OFFLINE_DB_STORE).getAll();
        req.onsuccess = function () { resolve(req.result || []); };
        req.onerror = function () { resolve([]); };
      });
      const candidates = rows
        .filter(function (r) { return !liveBookIds.has(r.book_id); })
        .sort(function (a, b) { return (a.last_opened || 0) - (b.last_opened || 0); });
      if (candidates.length === 0) return;
      const victim = candidates[0];

      // Delete IDB row first (authoritative).
      await new Promise(function (resolve) {
        const tx = db.transaction(self.OFFLINE_DB_STORE, "readwrite");
        tx.objectStore(self.OFFLINE_DB_STORE).delete(victim.book_id);
        tx.oncomplete = resolve;
        tx.onerror = resolve;
      });

      // Then best-effort cache + queue cleanup for the evicted book.
      try {
        const cache = await caches.open(BOOKS_CACHE);
        await cache.delete(victim.html_url, { ignoreSearch: true });
        await cache.delete(victim.file_url);
      } catch (_e) {}
      try { localStorage.removeItem("ropds-pos-" + victim.book_id); } catch (_e) {}
    } finally {
      db.close();
    }
  }

  async function countRows() {
    const db = await self.openOfflineDb();
    try {
      return await new Promise(function (resolve) {
        const tx = db.transaction(self.OFFLINE_DB_STORE, "readonly");
        const req = tx.objectStore(self.OFFLINE_DB_STORE).count();
        req.onsuccess = function () { resolve(req.result || 0); };
        req.onerror = function () { resolve(0); };
      });
    } finally { db.close(); }
  }

  async function writeIdbRow(initial) {
    const db = await self.openOfflineDb();
    try {
      await new Promise(function (resolve, reject) {
        const tx = db.transaction(self.OFFLINE_DB_STORE, "readwrite");
        const store = tx.objectStore(self.OFFLINE_DB_STORE);
        const getReq = store.get(BOOK_ID);
        getReq.onsuccess = function () {
          const existing = getReq.result || {};
          const existingTs = typeof existing.position_ts === "number" ? existing.position_ts : 0;
          const initialTs = typeof initial.position_ts === "number" ? initial.position_ts : 0;
          // Pick the fresher mirror by timestamp. The caller-supplied (server-rendered) values
          // only win when strictly fresher than the device-local mirror. Tie or older → keep
          // the existing mirror so a re-cache after offline reading does not regress position.
          const useInitial = initialTs > existingTs;
          const row = {
            book_id: BOOK_ID,
            title: TITLE,
            authors: AUTHORS,
            format: FORMAT,
            last_opened: Date.now(),
            position: useInitial ? (initial.position || "") : (existing.position || ""),
            progress: useInitial
              ? (typeof initial.progress === "number" ? initial.progress : 0)
              : (typeof existing.progress === "number" ? existing.progress : 0),
            position_ts: useInitial ? initialTs : existingTs,
            html_url: HTML_URL,
            file_url: FILE_URL,
            app_version: APP_VERSION,
          };
          store.put(row);
        };
        tx.oncomplete = resolve;
        tx.onerror = function () { reject(tx.error); };
      });
    } finally { db.close(); }
  }

  async function updateIdbPosition(position, progress) {
    try {
      const db = await self.openOfflineDb();
      try {
        await new Promise(function (resolve) {
          const tx = db.transaction(self.OFFLINE_DB_STORE, "readwrite");
          const store = tx.objectStore(self.OFFLINE_DB_STORE);
          const getReq = store.get(BOOK_ID);
          getReq.onsuccess = function () {
            const row = getReq.result;
            if (row) {
              row.position = position;
              row.progress = progress;
              row.position_ts = Date.now();
              store.put(row);
            }
          };
          tx.oncomplete = resolve;
          tx.onerror = resolve;
        });
      } finally { db.close(); }
    } catch (_e) {}
  }

  async function purgeStaleVersionsOnPage() {
    try {
      const db = await self.openOfflineDb();
      await self.purgeStaleVersionRows(db, APP_VERSION);
      db.close();
    } catch (_e) {}
  }

  async function maybeWipeIfDisabled() {
    if (MAX !== 0) return;
    // Wipe all IDB rows.
    try {
      const db = await self.openOfflineDb();
      await new Promise(function (resolve) {
        const tx = db.transaction(self.OFFLINE_DB_STORE, "readwrite");
        tx.objectStore(self.OFFLINE_DB_STORE).clear();
        tx.oncomplete = resolve;
        tx.onerror = resolve;
      });
      db.close();
    } catch (_e) {}
    // Wipe all books caches (any version).
    try {
      const keys = await caches.keys();
      await Promise.all(
        keys.filter(function (k) { return k.startsWith("ropds-books-v1-"); })
            .map(function (k) { return caches.delete(k); })
      );
    } catch (_e) {}
    // Wipe all queued positions.
    try {
      Object.keys(localStorage).forEach(function (k) {
        if (k.indexOf("ropds-pos-") === 0) localStorage.removeItem(k);
      });
    } catch (_e) {}
  }

  async function primeAndCacheBook(opts) {
    if (!isOfflineEligible()) return;
    const { response } = opts || {};
    if (!response) return;

    // Step 1: prime format-specific runtime assets (hard prerequisite).
    const primeUrls = URLS_FOR[FORMAT];
    if (primeUrls && primeUrls.length > 0) {
      try {
        const cache = await caches.open(STATIC_CACHE);
        await cache.addAll(primeUrls);
      } catch (e) {
        console.warn("[offline] runtime priming failed; not caching book:", e);
        return;
      }
    }

    // Step 2: book bytes.
    let booksCache;
    try {
      booksCache = await caches.open(BOOKS_CACHE);
      await booksCache.put(FILE_URL, response.clone());
    } catch (e) {
      console.warn("[offline] book bytes cache failed:", e);
      return;
    }

    // Step 3: re-fetch reader page HTML and cache it (page can't capture its own response).
    try {
      const htmlResp = await fetch(HTML_URL);
      if (!htmlResp || !htmlResp.ok) throw new Error("html fetch failed");
      await booksCache.put(HTML_URL, htmlResp.clone());
    } catch (e) {
      console.warn("[offline] reader HTML cache failed; rolling back bytes:", e);
      try { await booksCache.delete(FILE_URL); } catch (_) {}
      return;
    }

    // Step 4: write IDB row. The "initial" mirror is the fresher of (server-rendered,
    // localStorage queue). The localStorage check closes a race: if the user flipped a page
    // between the book fetch and this point, recordPositionLocal already wrote a queue entry —
    // we must not overwrite it with a stale server snapshot. writeIdbRow's existing-row-vs-
    // initial check provides a second layer of protection if a row was somehow written earlier.
    const serverProgress = parseFloat(ds.savedProgress || "0") || 0;
    const serverPosition = ds.savedPosition || "";
    const serverTs = parseInt(ds.savedPositionTs || "0", 10) || 0;

    let initial = { position: serverPosition, progress: serverProgress, position_ts: serverTs };
    try {
      const queued = getLocalPosition();
      if (queued && (queued.ts || 0) > serverTs) {
        initial = { position: queued.position, progress: queued.progress, position_ts: queued.ts };
      }
    } catch (_) {}

    try {
      await writeIdbRow(initial);
    } catch (e) {
      console.warn("[offline] IDB write failed:", e);
      return;
    }

    // Step 5: eviction.
    try {
      const n = await countRows();
      if (n > MAX) await evictOldest();
    } catch (e) {
      console.warn("[offline] eviction failed:", e);
    }
  }

  function getLocalPosition() {
    try {
      const raw = localStorage.getItem(POS_KEY);
      if (!raw) return null;
      const parsed = JSON.parse(raw);
      if (parsed && parsed.book_id === BOOK_ID) return parsed;
      return null;
    } catch (_e) { return null; }
  }

  function recordPositionLocal(p) {
    if (!isOfflineEligible()) return;
    // Synchronous queue write for offline replay (sync so sendBeacon close-tab path captures it).
    // The `format` field is consumed by syncPositionToMirror to distinguish "deferred — will be
    // promoted later" from "unreachable — book won't ever be cached" (PDF, unknown format).
    try {
      localStorage.setItem(POS_KEY, JSON.stringify({
        book_id: BOOK_ID,
        format: FORMAT,
        position: p.position,
        progress: p.progress,
        ts: Date.now(),
      }));
    } catch (_e) {}
    // Async IDB mirror update — does not block.
    updateIdbPosition(p.position, p.progress);
  }

  // How long we'll keep a "deferred" queue entry waiting for primeAndCacheBook to promote it.
  // Caching takes a few seconds at most; after this threshold we treat the book as a failed
  // cache attempt and clear the queue to prevent perpetual replays.
  const CACHE_PROMOTE_TIMEOUT_MS = 10 * 60 * 1000;

  // Shared helper used by both markPositionSynced (current book) and replayQueuedPositions
  // (any book in the queue). Decides what to do with a localStorage queue entry after the
  // server has confirmed the position. Returns one of:
  //   "mirrored"    — IDB row exists and was updated; safe to clear queue.
  //   "unreachable" — book is not cacheable on this device (PDF, unknown format, or stale-
  //                   waiting beyond CACHE_PROMOTE_TIMEOUT_MS); clear queue, mirror is not
  //                   needed and never will be written.
  //   "deferred"    — book is eligible for caching but the IDB row hasn't been written yet
  //                   (primeAndCacheBook still in flight); keep queue so it can be promoted.
  async function syncPositionToMirror(bookId, queued) {
    if (!queued || !Number.isFinite(bookId)) return "unreachable";
    let db;
    try { db = await self.openOfflineDb(); } catch (_) { return "deferred"; }
    let rowExists = false;
    try {
      // db.transaction(), tx.objectStore(), and store.get() can all throw synchronously
      // (e.g. NotFoundError if the store is missing). Wrap the entire transaction setup so
      // we never leak an unhandled rejection back to fire-and-forget callers.
      await new Promise(function (resolve) {
        let tx;
        try {
          tx = db.transaction(self.OFFLINE_DB_STORE, "readwrite");
        } catch (_e) { resolve(); return; }
        try {
          const store = tx.objectStore(self.OFFLINE_DB_STORE);
          const getReq = store.get(bookId);
          getReq.onsuccess = function () {
            const row = getReq.result;
            if (row) {
              rowExists = true;
              row.position = queued.position;
              row.progress = queued.progress;
              row.position_ts = queued.ts;
              store.put(row);
            }
          };
          getReq.onerror = function () { /* let tx events drive resolution */ };
        } catch (_e) { /* fall through to tx events */ }
        tx.oncomplete = resolve;
        tx.onerror = resolve;
        tx.onabort = resolve;
      });
    } catch (_e) { /* defensive — the inner Promise above should not reject, but treat it as deferred */ }
    finally {
      try { db.close(); } catch (_) {}
    }
    if (rowExists) return "mirrored";

    // No IDB row. Decide between "unreachable" (never going to be cached) and "deferred"
    // (eligible, primeAndCacheBook still possibly in flight).
    const fmt = queued.format;
    if (!fmt || !URLS_FOR[fmt] || URLS_FOR[fmt].length === 0) return "unreachable";
    if (typeof queued.ts === "number" && Date.now() - queued.ts > CACHE_PROMOTE_TIMEOUT_MS) {
      return "unreachable";
    }
    return "deferred";
  }

  async function markPositionSynced() {
    let queued;
    try { queued = getLocalPosition(); } catch (_) { return; }
    if (!queued) return;
    const result = await syncPositionToMirror(BOOK_ID, queued);
    if (result === "mirrored" || result === "unreachable") {
      try { localStorage.removeItem(POS_KEY); } catch (_) {}
    }
  }

  async function resolveInitialPosition(args) {
    // Source of truth offline = IDB mirror. localStorage is a queue, not a mirror.
    const serverTs = args.serverTs || 0;
    let mirror = null;
    try {
      const db = await self.openOfflineDb();
      try {
        mirror = await new Promise(function (resolve) {
          const tx = db.transaction(self.OFFLINE_DB_STORE, "readonly");
          const req = tx.objectStore(self.OFFLINE_DB_STORE).get(BOOK_ID);
          req.onsuccess = function () { resolve(req.result || null); };
          req.onerror = function () { resolve(null); };
        });
      } finally { db.close(); }
    } catch (_e) { mirror = null; }

    if (mirror && (mirror.position_ts || 0) > serverTs + 999 && mirror.position) {
      return { source: "local", position: mirror.position, progress: mirror.progress || 0 };
    }
    return { source: "server", position: args.serverPosition, progress: args.serverProgress };
  }

  async function replayQueuedPositions(csrfToken) {
    if (!csrfToken) return;
    const keys = [];
    try {
      for (let i = 0; i < localStorage.length; i++) {
        const k = localStorage.key(i);
        if (k && k.indexOf("ropds-pos-") === 0) keys.push(k);
      }
    } catch (_) { return; }
    for (const k of keys) {
      let entry;
      try { entry = JSON.parse(localStorage.getItem(k)); } catch (_) { continue; }
      if (!entry || !Number.isFinite(entry.book_id)) continue;
      try {
        const resp = await fetch("/web/api/reading-position", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            book_id: entry.book_id,
            position: entry.position,
            progress: entry.progress,
            csrf_token: csrfToken,
          }),
        });
        if (resp && resp.ok) {
          // Clear the queue when the helper says the position is mirrored OR when the book is
          // unreachable on this device (PDF, unknown format, or aged beyond the promote window).
          // Only "deferred" keeps the queue alive so primeAndCacheBook can promote it.
          const result = await syncPositionToMirror(entry.book_id, entry);
          if (result === "mirrored" || result === "unreachable") {
            localStorage.removeItem(k);
          }
        } else if (resp && (resp.status === 401 || resp.status === 403)) {
          // Auth lapsed — keep entry, retry after re-login.
          break;
        }
      } catch (_) { /* still offline; try next online event */ break; }
    }
  }

  // Run housekeeping once on script load.
  purgeStaleVersionsOnPage();
  maybeWipeIfDisabled();

  window.ROpdsOffline = {
    primeAndCacheBook: primeAndCacheBook,
    recordPositionLocal: recordPositionLocal,
    markPositionSynced: markPositionSynced,
    getLocalPosition: getLocalPosition,
    resolveInitialPosition: resolveInitialPosition,
    replayQueuedPositions: replayQueuedPositions,
    isOfflineEligible: isOfflineEligible,
  };
})();
