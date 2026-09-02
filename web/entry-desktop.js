// Entry router (desktop entry): coarse-pointer (or ?ui=touch) → dedicated touch
// UI. ?ui=desktop forces this desktop UI on any device. Loaded as an external
// file rather than an inline <script> so a strict CSP (no 'unsafe-inline';
// the Tauri desktop shell sets one) still runs it. Must stay first in <head>
// so a touch device never paints the desktop chrome first.
(function () {
  var p = new URLSearchParams(location.search);
  var ui = p.get("ui");
  if (ui === "desktop") return;
  if (ui === "touch" || matchMedia("(pointer:coarse)").matches) {
    location.replace("touch.html" + location.search);
  }
})();
