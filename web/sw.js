// confy PWA service worker — network-first with cache fallback.
//
// Strategy: every same-origin GET goes to the network (so a fresh deploy is
// picked up immediately, matching the push-to-main → Cloudflare flow) and the
// successful response is copied into the cache; only when the network fails
// (offline) is the cached copy served. The app shell is precached on install
// so the app works offline after the very first visit.
const CACHE = "confy-shell-v1";
const SHELL = [
  "./",
  "./index.html",
  "./touch.html",
  "./style.css",
  "./ui.js",
  "./touch/style.css",
  "./touch/app.js",
  "./pkg/confy_ffi.js",
  "./pkg/confy_ffi_bg.wasm",
  "./manifest.webmanifest",
];

self.addEventListener("install", (e) => {
  e.waitUntil(
    caches
      .open(CACHE)
      .then((c) => c.addAll(SHELL))
      .then(() => self.skipWaiting())
  );
});

self.addEventListener("activate", (e) => {
  e.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))))
      .then(() => self.clients.claim())
  );
});

self.addEventListener("fetch", (e) => {
  const url = new URL(e.request.url);
  if (e.request.method !== "GET" || url.origin !== location.origin) return;
  // Navigations carry volatile query strings (?ui=, ?url=) — ignore them when
  // falling back to the cached page.
  const ignoreSearch = e.request.mode === "navigate";
  e.respondWith(
    fetch(e.request)
      .then((res) => {
        if (res.ok) {
          const copy = res.clone();
          caches.open(CACHE).then((c) => c.put(e.request, copy));
        }
        return res;
      })
      .catch(async () => (await caches.match(e.request, { ignoreSearch })) ?? Response.error())
  );
});
