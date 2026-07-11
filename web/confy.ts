// Typed wrapper around the wasm `ConfySession`. The raw wasm bindings use `any`
// at the serde-wasm-bindgen boundary; this module restores the `types.ts` types
// and centralizes the one command channel (`dispatch`).
import init, {
  ConfySession as RawSession,
} from "./pkg/confy_ffi.js";
import type {
  Intent,
  KindOptionView,
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

  visibleRows(): ViewRow[] {
    return this.raw.visible_rows() as ViewRow[];
  }

  serialize(): string {
    return this.raw.serialize();
  }

  isDirty(): boolean {
    return this.raw.is_dirty();
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

  /** Free the underlying wasm memory. */
  free(): void {
    this.raw.free();
  }
}
