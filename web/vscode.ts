// VS Code webview host adapter — the third host shell (see fs.ts's Tauri
// branch for the pattern). Everything VS Code-specific on the webview side
// lives here: acquireVsCodeApi detection, the typed postMessage channel, and
// the body-class → data-theme mapping. Pure module: no imports from ui.ts.
import type { HostToWebview, ThemeMode, WebviewToHost } from "./vscode-protocol.js";

interface VsCodeApi {
  postMessage(msg: unknown): void;
}
declare const acquireVsCodeApi: (() => VsCodeApi) | undefined;

// acquireVsCodeApi throws if called twice — cache the one acquisition.
let acquired = false;
let api: VsCodeApi | null = null;
function vsapi(): VsCodeApi | null {
  if (!acquired) {
    acquired = true;
    api = typeof acquireVsCodeApi === "function" ? acquireVsCodeApi() : null;
  }
  return api;
}

/** True when running inside a VS Code webview. */
export function isVsCode(): boolean {
  return vsapi() !== null;
}

export function post(msg: WebviewToHost): void {
  vsapi()?.postMessage(msg);
}

export function onHostMessage(handler: (msg: HostToWebview) => void): void {
  window.addEventListener("message", (e: MessageEvent) => {
    handler(e.data as HostToWebview);
  });
}

// VS Code stamps the active theme kind on <body> (`vscode-light` /
// `vscode-dark` / `vscode-high-contrast`, plus `vscode-high-contrast-light`
// for light HC) and re-stamps it live on theme switch. In "auto" mode this
// maps onto confy's existing `:root[data-theme]` palettes and keeps tracking
// (the spec's host→webview `theme` message, done here with no protocol
// needed); in "light"/"dark" mode the menu-picked palette is pinned instead
// and VS Code's own theme changes are ignored.
let themeMode: ThemeMode = "auto";
let observingVsCodeTheme = false;

function applyVsCodeTheme(): void {
  if (themeMode !== "auto") {
    document.documentElement.dataset.theme = themeMode;
    return;
  }
  const cl = document.body.classList;
  const dark =
    cl.contains("vscode-dark") ||
    (cl.contains("vscode-high-contrast") && !cl.contains("vscode-high-contrast-light"));
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}

// Called at boot (default "auto", before the host's `init` reply arrives) and
// again on every `init`/`set-theme` message once the persisted mode is known.
// Idempotent: the class observer is only attached once.
export function trackVsCodeTheme(mode: ThemeMode): void {
  themeMode = mode;
  applyVsCodeTheme();
  if (!observingVsCodeTheme) {
    observingVsCodeTheme = true;
    new MutationObserver(applyVsCodeTheme).observe(document.body, {
      attributes: true,
      attributeFilter: ["class"],
    });
  }
}
