// Host-side I/O + theme flows shared by the two orchestrators (desktop
// web/ui.ts and touch web/touch/app.ts). Each function is parameterized on the
// small `HostIo` surface where the hosts actually differ (status line vs toast,
// sheet dismissal, download hints), so the save-copy / convert-write /
// open-from-URL flows are written once and cannot drift between UIs.
import {
  downloadText,
  extFor,
  fetchUrlFile,
  pickSaveFile,
  writeFile,
  type FsHandle,
} from "./fs.js";
import { extForTag } from "./convert-dialog.js";
import { Session } from "./confy.js";
import { t } from "./i18n.js";
import type { Intent, SessionSnapshot } from "./types.js";
import type { ConfigFormat } from "./vscode-protocol.js";

export type { ConfigFormat } from "./vscode-protocol.js";

// The differing surface between the two hosts.
export interface HostIo {
  fsAvailable: boolean;
  /** Can the host pick a *new* save destination? False on Tauri mobile in M1
   * (see `fs.ts`'s `canSaveAs`) — an already-open handle can still be saved
   * in place regardless of this flag. */
  canSaveAs: boolean;
  getSnap(): SessionSnapshot | null;
  send(i: Intent): void;
  /** Dispatch several intents with a single re-render at the end. */
  batch(fn: () => void): void;
  /** Live document text (`session.serialize()`), or null with no session. */
  serialize(): string | null;
  getFileName(): string | null;
  getHandle(): FsHandle | null;
  setHandle(h: FsHandle | null): void;
  ok(msg: string): void; // success feedback (status line / toast)
  err(msg: string): void; // failure feedback
  /** Host hook before a convert result is written (touch closes its sheets). */
  beforeConvertWrite?(): void;
  /** Host hook after a download-fallback write (touch toasts + FxiOS hint). */
  afterDownload?(filename: string, msg: string): void;
  /** Host hook after a first Save adopts a new handle (desktop: recent-files
   * bookkeeping; touch has nothing extra to do). */
  afterSaveAs?(handle: FsHandle, name: string): void;
  /** Adopt a freshly written file as the new open document — re-parse `text`
   * and switch to `handle`/`name`, so the app continues editing what was
   * just saved (like Open). Used by a first Save of a new/sample doc, Save
   * As, and Convert; an in-place Quick Save to an already-open handle never
   * calls this since it's already editing that exact file. */
  adoptFile(text: string, format: ConfigFormat, handle: FsHandle, name: string): void;
}

export function formatFromName(name: string): ConfigFormat {
  return name.endsWith(".json") || name.endsWith(".jsonc")
    ? "json"
    : name.endsWith(".yaml") || name.endsWith(".yml")
      ? "yaml"
      : "toml";
}

// Like formatFromName, but falls back to the HTTP Content-Type when the URL's
// last path segment has no recognizable extension (defaults to toml).
export function formatFromNameOrType(
  name: string,
  contentType: string | null,
): ConfigFormat {
  if (/\.(toml|json|jsonc|ya?ml)$/i.test(name)) return formatFromName(name);
  const ct = (contentType ?? "").toLowerCase();
  if (ct.includes("json")) return "json";
  if (ct.includes("yaml")) return "yaml";
  return "toml";
}

// The `DocFormat` tag a convert output path implies (extension → target).
function targetTagFor(path: string): string {
  return path.endsWith(".json")
    ? "Json"
    : path.endsWith(".yaml") || path.endsWith(".yml")
      ? "Yaml"
      : "Toml";
}

// The open file's stem (no directory, no extension) — the suggested output name.
export function fileStem(io: HostIo): string {
  const base = (io.getFileName() ?? "config").split("/").pop()!;
  const dot = base.lastIndexOf(".");
  return dot > 0 ? base.slice(0, dot) : base;
}

// Parse `text` into a fresh Session; a parse failure is reported through
// `err` and returns null, leaving `prev` untouched and still usable (the host
// keeps its state, as the old inline scaffold did). `prev` is freed only once
// the replacement has actually parsed — freeing it first would leave the
// caller's still-referenced `session` variable dangling on failure, and a
// later call passing that same freed object back in double-frees it (wasm
// "null pointer passed to rust"). The rest of openText (file/name/sample
// bookkeeping + render) stays host-owned.
export function replaceSession(
  prev: Session | null,
  text: string,
  format: ConfigFormat | "yml",
  err: (msg: string) => void,
): Session | null {
  let next: Session;
  try {
    next = Session.fromText(text, format);
  } catch (e) {
    err(String((e as Error).message ?? e));
    return null;
  }
  prev?.free();
  return next;
}

// Open a config file fetched from a URL. No on-disk handle, so a later Save
// falls back to Save As / download (same as the file-input path). Returns
// whether the fetch succeeded (the desktop host focuses the tree on success).
export async function openFromUrl(
  io: HostIo,
  openText: (
    text: string,
    format: ConfigFormat,
    handle: null,
    name: string,
  ) => void,
  url: string,
): Promise<boolean> {
  try {
    const { name, text, contentType } = await fetchUrlFile(url);
    openText(text, formatFromNameOrType(name, contentType), null, name);
    return true;
  } catch (e) {
    io.err(`Open URL failed: ${String((e as Error).message ?? e)}`);
    return false;
  }
}

// Best-effort file name from a handle (falls back to the last-known name on
// read failure, matching the old per-host `deriveName` helpers).
async function deriveName(handle: FsHandle, fallback: string): Promise<string> {
  try {
    return (await handle.getFile()).name;
  } catch {
    return fallback;
  }
}

// Quick Save: write in place to the currently open handle, or — if there is
// none yet (a new/unsaved doc) — behave like a first Save As. Unlike
// `doSaveAsCopy`/`doConvertWrite`, an in-place write to an *already open*
// handle never needs `canSaveAs` — that flag only gates picking a NEW
// destination. This is the fast path both hosts' primary Save action uses;
// `openSaveConvert` (below) is the separate, explicit "choose a destination /
// format" flow.
export async function doQuickSave(io: HostIo): Promise<void> {
  const text = io.serialize();
  if (text === null) return;
  const handle = io.getHandle();
  if (handle) {
    try {
      await writeFile(handle, text);
      io.send("Save");
      io.ok("Saved");
    } catch (e) {
      io.err(`save failed: ${String((e as Error).message ?? e)}`);
    }
    return;
  }
  // No handle yet — first save. Picking a destination needs `canSaveAs`.
  if (!io.canSaveAs) {
    io.err(t("web.mobile.saveAsUnavailable"));
    return;
  }
  const fmt = io.getSnap()!.doc_format;
  const suggested = (io.getFileName() ?? "confy-export") + extFor(fmt);
  if (io.fsAvailable) {
    try {
      const picked = await pickSaveFile(fmt, suggested);
      if (!picked) return; // cancelled (Tauri: null)
      await writeFile(picked, text);
      const name = await deriveName(picked, suggested);
      io.adoptFile(text, formatFromName(name), picked, name);
      io.afterSaveAs?.(picked, name);
      io.send("Save");
      io.ok("Saved");
    } catch (e) {
      // Browsers reject `showSaveFilePicker()` with AbortError on cancel
      // (rather than resolving null like the Tauri path) — treat it the same.
      if (e instanceof Error && e.name === "AbortError") return;
      io.err(`save failed: ${String((e as Error).message ?? e)}`);
    }
    return;
  }
  downloadText(suggested, text);
  io.afterDownload?.(suggested, "Downloaded");
  io.send("Save");
}

// Open the unified "Save / Convert" panel from the root node. `open_convert`
// leaves `target` = the current format (the panel's default), so the dialog
// opens on "save in the current format"; seed the output name from the open
// file's stem (core would otherwise default to "out.<ext>").
export function openSaveConvert(io: HostIo): void {
  io.batch(() => {
    io.send({ SetCursor: [] });
    io.send("OpenConvert");
    io.send({
      SetConvertPath: fileStem(io) + extForTag(io.getSnap()?.doc_format ?? "Toml"),
    });
  });
}

// Faithful "save a copy" of the live document (byte-for-byte `serialize()`),
// used when the panel's format equals the open format. Adopts the new file
// as the open document (see `HostIo.adoptFile`) — the app continues editing
// what was just saved, like Open.
export async function doSaveAsCopy(io: HostIo, path: string): Promise<void> {
  const text = io.serialize();
  if (text === null) return;
  const fmt = io.getSnap()!.doc_format;
  const baseName = path.split("/").pop() || "confy-export" + extFor(fmt);
  io.send("ExitConvert");
  if (!io.canSaveAs) {
    io.err(t("web.mobile.saveAsUnavailable"));
    return;
  }
  if (io.fsAvailable) {
    try {
      const handle = await pickSaveFile(fmt, baseName);
      if (!handle) return; // cancelled (Tauri: null)
      await writeFile(handle, text);
      const name = await deriveName(handle, baseName);
      io.adoptFile(text, formatFromName(name), handle, name);
      io.afterSaveAs?.(handle, name);
      io.ok(`Saved copy → ${name}`);
    } catch (e) {
      if (e instanceof Error && e.name === "AbortError") return; // browser cancel
      io.err(`save failed: ${String((e as Error).message ?? e)}`);
    }
    return;
  }
  downloadText(baseName, text);
  io.afterDownload?.(baseName, "Downloaded");
}

// The convert flow always produces a new file. Prefer Save As when the host
// supports it, else download. The Save-As picker gets the *target* format
// (derived from the output path core produced), not the source `doc_format`.
// Adopts the new file as the open document (see `HostIo.adoptFile`) — the app
// continues editing what was just converted, like Open.
export async function doConvertWrite(
  io: HostIo,
  path: string,
  text: string,
): Promise<void> {
  io.beforeConvertWrite?.();
  const baseName = path.split("/").pop() ?? "confy-converted";
  if (!io.canSaveAs) {
    io.err(t("web.mobile.saveAsUnavailable"));
    return;
  }
  if (io.fsAvailable) {
    const target = targetTagFor(path);
    const outExt = extFor(target);
    try {
      const handle = await pickSaveFile(
        target,
        baseName.endsWith(outExt) ? baseName : baseName + outExt,
      );
      if (!handle) return; // cancelled (Tauri: null)
      await writeFile(handle, text);
      const name = await deriveName(handle, baseName);
      io.adoptFile(text, formatFromName(name), handle, name);
      io.afterSaveAs?.(handle, name);
      io.ok(`Converted → ${name}`);
    } catch (e) {
      if (e instanceof Error && e.name === "AbortError") return; // browser cancel
      io.err(`convert write failed: ${String((e as Error).message ?? e)}`);
    }
    return;
  }
  downloadText(baseName, text);
  io.afterDownload?.(baseName, "Converted (downloaded)");
}

// ---- theme (identical in both hosts) ----
type Theme = "dark" | "light";

export function initTheme(): void {
  let stored: string | null = null;
  try {
    stored = localStorage.getItem("confy-theme");
  } catch {
    // storage blocked (sandboxed webview) — fall through to the dark default
  }
  document.documentElement.dataset.theme = stored === "light" ? "light" : "dark";
}

export function toggleTheme(): void {
  const next: Theme =
    document.documentElement.dataset.theme === "light" ? "dark" : "light";
  try {
    localStorage.setItem("confy-theme", next);
  } catch {
    // storage blocked — theme still applies for this session
  }
  document.documentElement.dataset.theme = next;
}
