// File System Access API integration with a download fallback.
//
// The capability boundary is clean: core `Intent::Save` only marks the doc
// saved; this module owns all real file I/O on the host side. When the API is
// unavailable (Firefox, Safari, older browsers) every call degrades to the
// download/textarea fallback so the UI stays fully functional.
//
// References:
//   https://developer.mozilla.org/en-US/docs/Web/API/File_System_Access_API

// Minimal ambient types — `lib.dom` may not ship these yet, and we never want
// a hard `tsconfig` lib bump just for them.
export interface FsHandle {
  getFile(): Promise<File>;
  createWritable(): Promise<FsWritable>;
  /** Absolute path, present only for Tauri-backed handles (desktop recent-files menu). */
  path?: string;
}
export interface FsWritable {
  write(data: string): Promise<void>;
  close(): Promise<void>;
}

type SavePicker = (
  opts?: object,
) => Promise<FsHandle>;
type OpenPicker = (opts?: object) => Promise<[FsHandle]>;

function savePicker(): SavePicker | null {
  const w = window as unknown as { showSaveFilePicker?: SavePicker };
  return w.showSaveFilePicker ?? null;
}
function openPicker(): OpenPicker | null {
  const w = window as unknown as { showOpenFilePicker?: OpenPicker };
  return w.showOpenFilePicker ?? null;
}

/** True when the host can open/save real files in place (Tauri or Chromium). */
export function fsAccessAvailable(): boolean {
  return isTauri() || (savePicker() !== null && openPicker() !== null);
}

const ACCEPT: Record<string, string> = {
  toml: "application/toml",
  json: "application/json",
  yaml: "application/yaml",
};

function acceptFor(format: string) {
  const ext = format === "Toml" ? "toml" : format === "Json" ? "json" : "yaml";
  return [
    {
      description: `${ext.toUpperCase()} config`,
      accept: { [ACCEPT[ext]]: [`.${ext}`] },
    },
  ];
}

/** `Toml`/`Json`/`Yaml` (the serde tag) → file extension. */
export function extFor(docFormat: string): string {
  return docFormat === "Toml" ? ".toml" : docFormat === "Json" ? ".json" : ".yaml";
}

export interface OpenedFile {
  handle: FsHandle;
  name: string;
  text: string;
  /** Absolute path, present only for Tauri-backed opens (desktop recent-files menu). */
  path?: string;
}

// ---- Tauri desktop/mobile host ----
// Inside the Tauri shell file I/O goes through `tauri-plugin-fs`/
// `tauri-plugin-dialog`'s JS bindings (exposed as `window.__TAURI__.fs` /
// `.dialog` via `app.withGlobalTauri`) instead of the browser File System
// Access API. The absolute path is the durable "handle"; we wrap it in an
// object that conforms to the `FsHandle` shape (getFile / createWritable,
// delegating to the plugin calls), so the rest of this module — and ui.ts —
// treats both hosts uniformly with no extra branching. `startup_file` stays a
// custom Rust command (no stock plugin covers CLI-arg open).
interface TauriCore {
  invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T>;
}
interface TauriDialogNs {
  open(opts?: {
    filters?: { name: string; extensions: string[] }[];
    multiple?: boolean;
  }): Promise<string | string[] | null>;
  save(opts?: { defaultPath?: string }): Promise<string | null>;
}
interface TauriFsNs {
  readTextFile(path: string): Promise<string>;
  writeTextFile(path: string, contents: string): Promise<void>;
}
interface TauriEventNs {
  listen<T>(event: string, handler: (e: { payload: T }) => void): Promise<() => void>;
}
interface TauriGlobal {
  core?: TauriCore;
  dialog?: TauriDialogNs;
  fs?: TauriFsNs;
  event?: TauriEventNs;
}
function tauriGlobal(): TauriGlobal | null {
  const w = window as unknown as { __TAURI__?: TauriGlobal };
  return w.__TAURI__ ?? null;
}

/** True when running inside the Tauri desktop/mobile shell. */
export function isTauri(): boolean {
  return tauriGlobal()?.core != null;
}

/** True on Tauri mobile (Android/iOS) — no file extension UTI, same lesson as
 * the web `fileInput`. UA-sniffed: no `tauri-plugin-os` dependency needed. */
export function isTauriMobile(): boolean {
  return isTauri() && /Android|iPhone|iPad|iPod/.test(navigator.userAgent);
}

/** True on Tauri Android specifically — `tauri-plugin-confy-picker` (Task 0's
 * write-capable picker fix) is registered only there, not on iOS/desktop. */
export function isTauriAndroid(): boolean {
  return isTauri() && /Android/.test(navigator.userAgent);
}

/**
 * True when the host can pick a *new* save destination (Save As / Convert to
 * a new file / first Save with no open handle). False only on Tauri mobile in
 * M1: stock `tauri-plugin-dialog`'s Android save path (`ACTION_CREATE_DOCUMENT`)
 * is untested and the spec flags it as a research item, so M1 disables picking
 * a new destination there — only in-place saves to an already-open, already
 * write-granted handle (via `tauri-plugin-confy-picker`) are supported. Desktop
 * Tauri and every browser path (FS Access API or download fallback) stay true.
 */
export function canSaveAs(): boolean {
  return !isTauriMobile();
}

interface RustOpenedFile {
  path: string;
  name: string;
  text: string;
}

// An `FsHandle` backed by a real filesystem path via the fs plugin.
function tauriHandle(path: string, name: string): FsHandle {
  const fs = tauriGlobal()!.fs!;
  return {
    path,
    async getFile(): Promise<File> {
      const text = await fs.readTextFile(path);
      return new File([text], name);
    },
    async createWritable(): Promise<FsWritable> {
      return {
        async write(data: string) {
          await fs.writeTextFile(path, data);
        },
        async close() {},
      };
    },
  };
}

/** Open the file passed on the command line, if any (Tauri only). */
export async function tauriStartupFile(): Promise<OpenedFile | null> {
  const core = tauriGlobal()?.core;
  if (!core) return null;
  const f = await core.invoke<RustOpenedFile | null>("startup_file");
  if (!f) return null;
  return { handle: tauriHandle(f.path, f.name), name: f.name, text: f.text, path: f.path };
}

/** Reopen a previously-known Tauri path (desktop Open Recent menu). `null` if
 * the file no longer exists or can't be read — the caller drops it from the
 * recent list. */
export async function openTauriPath(path: string): Promise<OpenedFile | null> {
  const fs = tauriGlobal()?.fs;
  if (!fs) return null;
  try {
    const text = await fs.readTextFile(path);
    const name = path.split(/[\\/]/).pop() ?? path;
    return { handle: tauriHandle(path, name), name, text, path };
  } catch {
    return null;
  }
}

/**
 * URLs (`content://…` on Android, `file://…` elsewhere) the OS asked us to
 * open before this frontend was ready to listen — a cold start via a
 * file-association "Open with". The Rust side drains its buffer on read, so
 * calling this a second time returns nothing new; a warm app instead gets
 * these via `onTauriOpened`. Feed each URL through `openTauriPath` — the
 * granted `content://`/`file://` URI reads the same way an already-known
 * Tauri path does.
 */
export async function tauriOpenedUrls(): Promise<string[]> {
  const core = tauriGlobal()?.core;
  if (!core) return [];
  return core.invoke<string[]>("opened_urls");
}

/**
 * Subscribe to files the OS opens the app with while it's already running
 * (mobile "Open with" on a warm app). Returns an unlisten function, or a
 * no-op outside Tauri.
 */
export async function onTauriOpened(cb: (url: string) => void): Promise<() => void> {
  const event = tauriGlobal()?.event;
  if (!event) return () => {};
  return event.listen<string[]>("opened", (e) => {
    for (const url of e.payload) cb(url);
  });
}

/** Fetch a config file's text from a URL. Throws on network/HTTP failure. */
export async function fetchUrlFile(
  url: string,
): Promise<{ name: string; text: string; contentType: string | null }> {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`HTTP ${res.status} ${res.statusText}`);
  const text = await res.text();
  const contentType = res.headers.get("content-type");
  // Derive a display name from the URL's last path segment (may lack an extension).
  let name = "config";
  try {
    const seg = new URL(url).pathname.split("/").filter(Boolean).pop();
    if (seg) name = decodeURIComponent(seg);
  } catch { /* leave default */ }
  return { name, text, contentType };
}

interface PickWritableResponse {
  uri: string | null;
  name: string | null;
}

/** Open a real file via the native dialog (Tauri) or FS Access API; `null` on cancel. */
export async function pickOpenFile(): Promise<OpenedFile | null> {
  const g = tauriGlobal();
  // Android: `tauri-plugin-dialog`'s picker uses `ACTION_GET_CONTENT`, which
  // never grants write access (Task 0 finding) — route through the custom
  // `tauri-plugin-confy-picker` plugin instead, which takes a persistable
  // read+write grant on the chosen URI up front.
  if (g?.core && g.fs && isTauriAndroid()) {
    const res = await g.core.invoke<PickWritableResponse>("plugin:confy-picker|pick_writable");
    if (!res.uri) return null;
    const text = await g.fs.readTextFile(res.uri);
    // `content://` URIs are opaque — the Downloads provider hands out
    // ".../document/31" with no filename at all, silently dropping the
    // extension (and the format guess) if we fall back to path-splitting.
    // The plugin queries SAF's DISPLAY_NAME column for the real name; only
    // fall back to the URI's last segment if that somehow comes back empty.
    const name = res.name ?? (res.uri.split(/[\\/]/).pop() ?? res.uri);
    return { handle: tauriHandle(res.uri, name), name, text, path: res.uri };
  }
  if (g?.dialog && g.fs) {
    // No extension filter on mobile: iOS/Android have no UTI for `.toml`/
    // `.yaml`, which would grey the files out — same lesson as `fileInput`.
    const filters = isTauriMobile()
      ? undefined
      : [{ name: "Config", extensions: ["toml", "json", "jsonc", "yaml", "yml"] }];
    const picked = await g.dialog.open({ filters, multiple: false });
    const path = Array.isArray(picked) ? picked[0] : picked;
    if (!path) return null;
    const text = await g.fs.readTextFile(path);
    const name = path.split(/[\\/]/).pop() ?? path;
    return { handle: tauriHandle(path, name), name, text, path };
  }
  const picker = openPicker();
  if (!picker) return null;
  const [handle] = await picker();
  const file = await handle.getFile();
  const text = await file.text();
  return { handle, name: file.name, text };
}

/**
 * Pick a destination for a new file (Save As). Returns the handle, or `null`
 * on cancel. Only callable when `fsAccessAvailable()` is true.
 */
export async function pickSaveFile(
  docFormat: string,
  suggestedName: string,
): Promise<FsHandle | null> {
  const g = tauriGlobal();
  if (g?.dialog && g.fs) {
    const path = await g.dialog.save({ defaultPath: suggestedName });
    if (!path) return null;
    const name = path.split(/[\\/]/).pop() ?? suggestedName;
    return tauriHandle(path, name);
  }
  const picker = savePicker();
  if (!picker) return null;
  return picker({ suggestedName, types: acceptFor(docFormat) });
}

/** Write text to an existing handle (in-place save). */
export async function writeFile(handle: FsHandle, text: string): Promise<void> {
  const w = await handle.createWritable();
  await w.write(text);
  await w.close();
}

// ---- Download fallback (always available) ----

/**
 * True on Firefox for iOS. It exposes no Web Share for files *and* WebKit
 * ignores the `<a download>` filename, deriving the extension from the MIME
 * type instead — and iOS has no UTI for `.toml`/`.yaml`, so those download
 * extension-less. (`.json` still works; Safari works via Web Share.) The host
 * uses this to hint the user toward Safari rather than fail silently.
 */
export function isFirefoxIos(): boolean {
  return /FxiOS/.test(navigator.userAgent);
}

/** File extension → MIME type, so iOS doesn't coerce the download to `.txt`. */
function mimeFor(filename: string): string {
  if (filename.endsWith(".toml")) return "application/toml";
  if (filename.endsWith(".json")) return "application/json";
  if (filename.endsWith(".yaml") || filename.endsWith(".yml")) return "application/yaml";
  return "text/plain";
}

type Sharer = (data: { files: File[]; title?: string }) => Promise<void>;
type ShareChecker = (data: { files: File[] }) => boolean;

/**
 * Trigger a browser download — the universal fallback for save/convert.
 *
 * iOS Safari has no FS Access API *and* ignores the `<a download>` filename
 * (it names the file after the blob UUID and appends `.txt` from the MIME),
 * so the requested name/extension is lost. The Web Share API preserves both:
 * the `File`'s name and extension survive into "Save to Files". We prefer it
 * when the host can share files, else fall back to the anchor download (which
 * works fine on desktop Firefox/Safari).
 *
 * Firefox iOS doesn't expose `canShare`, so we still *attempt* `share` whenever
 * it exists (best-effort) and fall back to the anchor only if the share rejects
 * with a non-cancellation error.
 */
export function downloadText(filename: string, text: string): void {
  const type = mimeFor(filename);
  const nav = navigator as Navigator & {
    canShare?: ShareChecker;
    share?: Sharer;
  };
  const file = new File([text], filename, { type });
  const anchorDownload = () => {
    const blob = new Blob([text], { type });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = filename;
    a.click();
    URL.revokeObjectURL(url);
  };
  // Share when supported, or attempt it when `canShare` is simply absent
  // (Firefox iOS) — the filename/extension survive into "Save to Files".
  if (nav.share && (!nav.canShare || nav.canShare({ files: [file] }))) {
    nav.share({ files: [file], title: filename }).catch((err: unknown) => {
      // User cancellation is a normal choice — don't double up with a download.
      if (err && (err as { name?: string }).name === "AbortError") return;
      anchorDownload();
    });
    return;
  }
  anchorDownload();
}
