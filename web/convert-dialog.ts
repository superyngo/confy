// Shared Save/Convert dialog, rendered identically for the desktop and touch
// UIs. Pure DOM + string HTML over the `#convFmt`/`#convPath`/`#convWarns`/
// `#convRun`/`#convCancel` children the per-page HTML provides. The host passes
// element refs, a host-owned **surface** (so the markup can live in a native
// `<dialog>` on desktop or a bottom `.sheet` on touch), and host-owned I/O
// callbacks (`fileStem`, `doSaveAsCopy`); this module never reaches for the DOM
// by id or touches `window`.
import type { ConvertView, DocFormat, Intent, SessionSnapshot } from "./types.js";
import { escapeHtml } from "./render.js";
import { t, tArgs } from "./i18n.js";

// Where the convert form is mounted: a native `<dialog>` (desktop) or a bottom
// `.sheet` (touch). The host supplies open/close/isOpen + a cancel hook (Esc /
// backdrop / scrim) so this module stays container-agnostic.
export interface ConvertSurface {
  isOpen(): boolean;
  open(): void;
  close(): void;
  onCancel(cb: () => void): void;
}

// The five form children plus the host-owned surface they live in.
export interface ConvertRefs {
  surface: ConvertSurface;
  fmt: HTMLSelectElement;
  path: HTMLInputElement;
  warns: HTMLElement;
  run: HTMLElement;
  cancel: HTMLElement;
}

// The default output extension for a `DocFormat` tag.
export function extForTag(tag: string): string {
  return tag === "Json" ? ".json" : tag === "Yaml" ? ".yaml" : ".toml";
}

// Open/update the convert `<dialog>`: fill the format `<select>` (current format
// leads → default save-as), seed the output-path `<input>`, render the warnings
// list, and set the run-label. Assumes `Mode::Convert` is active (the host gates
// the closed case).
export function renderConvertDialog(
  refs: ConvertRefs,
  cv: ConvertView,
  snap: SessionSnapshot,
): void {
  const { surface, fmt: sel, path, warns, run } = refs;
  // Unified "Save / Convert" panel: the current format leads the list (default)
  // so picking it is a plain save-as; the other two are cross-format converts.
  const all = [snap.doc_format, ...cv.options];
  if (!surface.isOpen()) {
    sel.innerHTML = all
      .map((f) => `<option value="${f}">${f.toUpperCase()}</option>`)
      .join("");
    sel.value = cv.target;
    path.value = cv.path;
    surface.open();
  } else {
    if (sel.value !== cv.target) sel.value = cv.target;
    // Don't clobber the box while the user is typing the path.
    if (document.activeElement !== path) path.value = cv.path;
  }
  // Same format → faithful save (no loss); only a cross-format convert warns.
  const crossFmt = cv.target !== snap.doc_format;
  const hasWarn = crossFmt && cv.warnings.length > 0;
  warns.innerHTML = hasWarn
    ? `<strong>${t("web.convert.warn.title")}</strong><div class="warns-note">${escapeHtml(tArgs("web.convert.warn.note", [cv.target.toUpperCase()]))}</div>` +
      `<ul>${cv.warnings.map((w) => `<li>${escapeHtml(w)}</li>`).join("")}</ul>`
    : "";
  warns.classList.toggle("hide", !hasWarn);
  run.textContent = !crossFmt
    ? t("web.convert.run.save")
    : cv.step === "Confirm"
      ? t("web.convert.run.confirm")
      : t("web.convert.run.convert");
}

// Run the panel's action: a same-format pick is a faithful save-as of the live
// document; a cross-format pick drives core's convert (warnings → confirm).
export function runSaveConvert(
  snap: SessionSnapshot,
  { send, doSaveAsCopy }: { send: (i: Intent) => void; doSaveAsCopy: (path: string) => void },
): void {
  const m = snap.mode;
  if (typeof m !== "object" || !("Convert" in m)) return;
  const cv = m.Convert;
  if (cv.target === snap.doc_format) return void doSaveAsCopy(cv.path);
  send(cv.step === "Confirm" ? "ConvertConfirm" : "ConvertRun");
}

// Attach the dialog's change/click/cancel listeners. `getSnap` returns the latest
// snapshot at click time (so the action reads live mode state).
export function wireConvertDialog(
  refs: ConvertRefs,
  {
    send,
    fileStem,
    doSaveAsCopy,
    getSnap,
  }: {
    send: (i: Intent) => void;
    fileStem: () => string;
    doSaveAsCopy: (path: string) => void;
    getSnap: () => SessionSnapshot | null;
  },
): void {
  refs.fmt.addEventListener("change", (e) => {
    const tag = (e.target as HTMLSelectElement).value as DocFormat;
    send({ SetConvertFormat: tag });
    // SetConvertFormat reseeds the path to "out.<ext>"; restore the real stem.
    send({ SetConvertPath: fileStem() + extForTag(tag) });
  });
  refs.path.addEventListener("input", (e) =>
    send({ SetConvertPath: (e.target as HTMLInputElement).value }),
  );
  refs.run.addEventListener("click", () => {
    const snap = getSnap();
    if (snap) runSaveConvert(snap, { send, doSaveAsCopy });
  });
  refs.cancel.addEventListener("click", () => send("ExitConvert"));
  // Surface-level cancel (native dialog Esc / sheet scrim) → leave Convert mode
  // (render then closes the surface).
  refs.surface.onCancel(() => send("ExitConvert"));
}
