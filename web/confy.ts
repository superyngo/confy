// Typed wrapper around the wasm `ConfySession`. The raw wasm bindings use `any`
// at the serde-wasm-bindgen boundary; this module restores the `types.ts` types
// and centralizes the one command channel (`dispatch`).
import init, {
  ConfySession as RawSession,
} from "./pkg/confy_ffi.js";
import type {
  ChildView,
  DiagEvent,
  EditHint,
  Intent,
  KindOptionView,
  PasteSlot,
  Path,
  SessionSnapshot,
  ViewRow,
} from "./types.js";

// The per-node convertible-kind entry is the serde `KindOptionView` from
// types.ts; re-exported under this module's traditional name.
export type KindOption = KindOptionView;

let bootstrapped = false;

/**
 * Load the wasm module. Must be awaited once before constructing a session.
 * In a browser, `wasmUrl` is the URL to `confy_ffi_bg.wasm`.
 */
export async function load(wasmUrl: string | URL): Promise<void> {
  if (bootstrapped) return;
  await init(wasmUrl);
  bootstrapped = true;
}

/**
 * A typed handle on a confy session. `dispatch` is the single command channel:
 * send one `Intent`, get one full-state `SessionSnapshot` (PORTING §8.3/§8.4).
 */
export class Session {
  private constructor(private raw: RawSession) {}

  /** Parse `text` as `format` and open a session. `load()` must have resolved. */
  static fromText(text: string, format: "toml" | "json" | "yaml" | "yml"): Session {
    return new Session(new RawSession(text, format));
  }

  /** The one command channel. */
  dispatch(i: Intent): SessionSnapshot {
    return this.raw.dispatch(i) as SessionSnapshot;
  }

  snapshot(): SessionSnapshot {
    return this.raw.snapshot() as SessionSnapshot;
  }

  diagLog(): DiagEvent[] {
    return this.raw.diag_log() as DiagEvent[];
  }

  visibleRows(): ViewRow[] {
    return this.raw.visible_rows() as ViewRow[];
  }

  serialize(): string {
    return this.raw.serialize();
  }

  isDirty(): boolean {
    return this.raw.is_dirty();
  }

  /**
   * Host-supplied: true iff the open document's real file extension is
   * plain `.json` (not `.jsonc`) — the wasm core is extension-blind, so
   * only the host knows this. Drives the per-row `comment_advisory`
   * decoration. Call once right after `fromText`.
   */
  setStrictJson(v: boolean): void {
    this.raw.set_strict_json(v);
  }

  /**
   * Whether authored comments are currently legal in the open document —
   * true from load if the raw text already contained a `//` line comment
   * or a block comment.
   */
  supportsComments(): boolean {
    return this.raw.supports_comments();
  }

  docFormat(): string {
    return this.raw.doc_format();
  }

  /** About-tab body text for the session's current language (core catalog). */
  aboutText(): string {
    return this.raw.about_text();
  }

  kindOptions(path: Path): KindOption[] {
    return this.raw.kind_options(path) as KindOption[];
  }

  /**
   * Schema-driven editing constraint for the node at `path` — enum/const
   * options or numeric bounds, `"None"` when unconstrained or no schema is
   * loaded. Read-only, does not enter edit mode.
   */
  schemaHint(path: Path): EditHint {
    return this.raw.schema_hint(path) as EditHint;
  }

  /**
   * Non-widget descriptive schema info for the node at `path` —
   * `description`/`type`/`format`/`pattern` from the resolved subschema,
   * `undefined` when unresolvable or none of those keywords are present.
   * Orthogonal to `schemaHint`: that resolves a widget (enum/const picker,
   * numeric bounds) and stays `"None"` for a plain-typed field; this covers
   * that common case so the detail panel still has something to show.
   */
  schemaInfo(path: Path): string | undefined {
    return this.raw.schema_info(path) as string | undefined;
  }

  /** Immediate children of the node at `path` (breadcrumb mini-tree). */
  children(path: Path): ChildView[] {
    return this.raw.children(path) as ChildView[];
  }

  /**
   * Pointer-drop classification (ADR 0004 §1): "this row, this relative
   * vertical position" (`0` = row top, `1` = row bottom) -> the `PasteSlot`
   * it represents, or `undefined` if the row is no longer visible.
   */
  pointerSlot(path: Path, relY: number): PasteSlot | undefined {
    return this.raw.pointer_slot(path, relY) as PasteSlot | undefined;
  }

  /** Free the underlying wasm memory. */
  free(): void {
    this.raw.free();
  }
}
