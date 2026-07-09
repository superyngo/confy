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
}

// ---- Tauri desktop host ----
// Inside the Tauri shell (`confy-desktop`) file I/O goes through native Rust
// commands instead of the browser File System Access API. The absolute path is
// the durable "handle"; we wrap it in an object that conforms to the `FsHandle`
// shape (getFile / createWritable, delegating to `invoke`), so the rest of this
// module — and ui.ts — treats both hosts uniformly with no extra branching.
interface TauriCore {
  invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T>;
}
function tauriCore(): TauriCore | null {
  const w = window as unknown as { __TAURI__?: { core?: TauriCore } };
  return w.__TAURI__?.core ?? null;
}

/** True when running inside the Tauri desktop shell. */
export function isTauri(): boolean {
  return tauriCore() !== null;
}

interface RustOpenedFile {
  path: string;
  name: string;
  text: string;
}

// An `FsHandle` backed by a real filesystem path via Tauri commands.
function tauriHandle(path: string, name: string): FsHandle {
  const core = tauriCore()!;
  return {
    async getFile(): Promise<File> {
      const text = await core.invoke<string>("read_file_text", { path });
      return new File([text], name);
    },
    async createWritable(): Promise<FsWritable> {
      return {
        async write(data: string) {
          await core.invoke<void>("write_file", { path, contents: data });
        },
        async close() {},
      };
    },
  };
}

/** Open the file passed on the command line, if any (Tauri only). */
export async function tauriStartupFile(): Promise<OpenedFile | null> {
  const core = tauriCore();
  if (!core) return null;
  const f = await core.invoke<RustOpenedFile | null>("startup_file");
  if (!f) return null;
  return { handle: tauriHandle(f.path, f.name), name: f.name, text: f.text };
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

/** Open a real file via the native dialog (Tauri) or FS Access API; `null` on cancel. */
export async function pickOpenFile(): Promise<OpenedFile | null> {
  const core = tauriCore();
  if (core) {
    const f = await core.invoke<RustOpenedFile | null>("open_dialog");
    if (!f) return null;
    return { handle: tauriHandle(f.path, f.name), name: f.name, text: f.text };
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
  const core = tauriCore();
  if (core) {
    const path = await core.invoke<string | null>("save_dialog", {
      suggested: suggestedName,
    });
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
