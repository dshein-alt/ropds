// sw.js — Service worker for ROPDS PWA.
// Caches /static/* (version-pinned) and provides offline fallback.
// Book-cache writes are page-driven; the SW only reads ropds-books-v1-* on offline replay
// and purges stale-version IDB rows during activate.
//
// Registered URL: /static/sw.js?v=<APP_VERSION>
// SW reads APP_VERSION from its own URL.

importScripts("/static/js/idb-schema.js");

const APP_VERSION = (function () {
  try {
    return new URL(self.location).searchParams.get("v") || "";
  } catch (_e) {
    return "";
  }
})();

const STATIC_CACHE = "ropds-static-v2-" + APP_VERSION;
const BOOKS_CACHE = "ropds-books-v1-" + APP_VERSION;
const OFFLINE_FALLBACK = "/static/offline.html";

// Precache list — small enough to download on every install.
const PRECACHE_URLS = [
  "/static/offline.html",
  "/static/css/bootstrap.min.css",
  "/static/css/bootstrap-icons.min.css",
  "/static/css/ropds.css",
  "/static/fonts/bootstrap-icons.woff2",
  "/static/fonts/bootstrap-icons.woff",
  "/static/js/reader.js",
  "/static/js/reader-offline.js",
  "/static/js/idb-schema.js",
  "/static/js/ropds.js",
  "/static/js/bootstrap.bundle.min.js",
];

self.addEventListener("install", function (event) {
  event.waitUntil(
    (async function () {
      const cache = await caches.open(STATIC_CACHE);
      // Use individual put()s with ignoreSearch-friendly keys (no query string).
      await Promise.all(
        PRECACHE_URLS.map(async function (url) {
          try {
            const resp = await fetch(url, { cache: "reload" });
            if (resp && resp.ok) await cache.put(url, resp.clone());
          } catch (e) {
            console.warn("[sw] precache miss:", url, e);
          }
        })
      );
      self.skipWaiting();
    })()
  );
});

self.addEventListener("activate", function (event) {
  event.waitUntil(
    (async function () {
      // Delete any cache that does not match the current versioned pair.
      const keep = new Set([STATIC_CACHE, BOOKS_CACHE]);
      const keys = await caches.keys();
      await Promise.all(
        keys.filter(function (k) {
          return (k.startsWith("ropds-static-") || k.startsWith("ropds-books-")) && !keep.has(k);
        }).map(function (k) { return caches.delete(k); })
      );

      // Purge stale-version IDB rows.
      try {
        const db = await self.openOfflineDb();
        await self.purgeStaleVersionRows(db, APP_VERSION);
        db.close();
      } catch (e) {
        console.warn("[sw] IDB purge failed:", e);
      }

      await self.clients.claim();
    })()
  );
});

self.addEventListener("fetch", function (event) {
  const request = event.request;
  if (request.method !== "GET") return;

  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;

  // Static assets — version-mismatch bypass + ignoreSearch lookup + strip-on-write.
  if (url.pathname.startsWith("/static/")) {
    if (url.pathname === "/static/sw.js") return; // never intercept SW itself
    event.respondWith(handleStatic(request, url));
    return;
  }

  // Reader navigation: HTML page.
  if (url.pathname.match(/^\/web\/reader\/\d+$/)) {
    event.respondWith(handleReaderNav(request, url));
    return;
  }

  // Book bytes.
  if (url.pathname.match(/^\/web\/read\/\d+$/)) {
    event.respondWith(handleBookBytes(request, url));
    return;
  }

  // Other navigations — fall back to offline.html on network failure.
  if (request.mode === "navigate") {
    event.respondWith(handleOtherNav(request));
    return;
  }
});

async function handleStatic(request, url) {
  // Version-mismatch bypass: if ?v=<X> is present and != APP_VERSION, skip cache.
  const requestedVersion = url.searchParams.get("v");
  if (requestedVersion && requestedVersion !== APP_VERSION) {
    try { return await fetch(request); } catch (_e) { /* network unreachable */ }
    // Fall through to whatever cache may have (rare).
  }

  const cache = await caches.open(STATIC_CACHE);
  const cached = await cache.match(request, { ignoreSearch: true });

  const networkPromise = fetch(request)
    .then(function (response) {
      if (response && response.status === 200 && (response.type === "basic" || response.type === "default")) {
        // Strip query string before put.
        const canonicalUrl = url.origin + url.pathname;
        cache.put(canonicalUrl, response.clone()).catch(function () {});
      }
      return response;
    })
    .catch(function () { return null; });

  if (cached) return cached;
  const fresh = await networkPromise;
  if (fresh) return fresh;
  return new Response("Offline", { status: 503, headers: { "content-type": "text/plain" } });
}

async function handleReaderNav(request, _url) {
  try {
    const fresh = await fetch(request);
    return fresh;
  } catch (_e) {
    const cache = await caches.open(BOOKS_CACHE);
    // ignoreSearch so /web/reader/123?return=... matches /web/reader/123
    const cached = await cache.match(request, { ignoreSearch: true });
    if (cached) return cached;
    return offlineFallback();
  }
}

async function offlineFallback() {
  const cached = await caches.match(OFFLINE_FALLBACK);
  if (cached) return cached;
  // Last-ditch synthetic response so we never resolve to undefined.
  return new Response(
    "<!doctype html><meta charset=utf-8><title>Offline</title><body style=\"font:14px system-ui;padding:2rem\">You are offline and the offline shell is not yet cached. Reload while online to install it.</body>",
    { status: 200, headers: { "content-type": "text/html; charset=utf-8" } }
  );
}

async function handleBookBytes(request, _url) {
  try {
    return await fetch(request);
  } catch (_e) {
    const cache = await caches.open(BOOKS_CACHE);
    const cached = await cache.match(request);
    if (cached) return cached;
    return new Response("Offline", { status: 503 });
  }
}

async function handleOtherNav(request) {
  try {
    return await fetch(request);
  } catch (_e) {
    return offlineFallback();
  }
}
