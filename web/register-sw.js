// PWA: register the service worker on the deployed (https) site only — the dev
// server stays SW-free so its no-store caching keeps working. External file,
// not an inline <script>, so a strict CSP (no 'unsafe-inline') still runs it.
if ("serviceWorker" in navigator && location.protocol === "https:")
  addEventListener("load", function () {
    navigator.serviceWorker.register("./sw.js");
  });
