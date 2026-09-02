// Reverse guard (touch entry): if the visitor explicitly asked for desktop, or
// arrived on a fine pointer without forcing ?ui=touch, bounce back to the
// desktop UI so one URL serves both. External file, not an inline <script>, so
// a strict CSP (no 'unsafe-inline') still runs it.
(function () {
  var p = new URLSearchParams(location.search);
  var ui = p.get("ui");
  if (ui === "touch") return;
  if (ui === "desktop" || !matchMedia("(pointer:coarse)").matches) {
    location.replace("index.html" + location.search);
  }
})();
