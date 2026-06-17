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

/** True when the host can open/save real files in place (Chromium-based). */
export function fsAccessAvailable(): boolean {
  return savePicker() !== null && openPicker() !== null;
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

/** Open a real file via the FS Access API; returns `null` if the user cancels. */
export async function pickOpenFile(): Promise<OpenedFile | null> {
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

/** Read a handle's current text (used to resync after an external change). */
export async function readHandle(handle: FsHandle): Promise<string> {
  return (await handle.getFile()).text();
}

// ---- Download fallback (always available) ----

/** Trigger a browser download — the universal fallback for save/convert. */
export function downloadText(filename: string, text: string): void {
  const blob = new Blob([text], { type: "text/plain" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}
