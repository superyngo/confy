// editors/vscode/src/schemaSessionManager.ts
import type { EditHint, Path, SchemaSource, SessionSnapshot, ViolationView } from "../../../web/types.js";
import type { ConfySessionCtor, ConfySessionHandle, OutlineNode } from "./wasmSession.js";
// Runtime imports use `.ts` specifiers so `node --experimental-strip-types`
// can execute this module directly in tests (type-only `.js` imports above
// are erased; esbuild resolves `.ts` specifiers identically).
import { needsSchemaReload } from "./schemaDedup.ts";
import { resolveLocalSchemaPath } from "./schemaPathResolve.ts";

export interface SchemaSessionDeps {
  readFile: (path: string) => Promise<string>;
  fetchUrl: (url: string) => Promise<string>;
}

export interface DocSyncResult {
  violations: ViolationView[];
  loadError: string | undefined;
  invalidSyntax: boolean;
}

interface ManagedDoc {
  session: ConfySessionHandle;
  fsPath: string;
  loadedSchemaSource: SchemaSource | undefined;
  generation: number;
}

/**
 * One persistent `ConfySession` per open document (ADR 0007), keyed by an
 * opaque caller-chosen string (the caller uses `document.uri.toString()`).
 * Edits go through `reparse()`'s `Intent::ApplyReplace{path: [], text}`
 * against the *same* session rather than constructing a new one, so the
 * compiled schema `Validator` survives every edit; schema fetch/reload is
 * only re-triggered when `needsSchemaReload` says the detected source
 * actually changed.
 */
export class SchemaSessionManager {
  private docs = new Map<string, ManagedDoc>();
  private readonly SessionCtor: ConfySessionCtor;
  private readonly deps: SchemaSessionDeps;

  constructor(SessionCtor: ConfySessionCtor, deps: SchemaSessionDeps) {
    this.SessionCtor = SessionCtor;
    this.deps = deps;
  }

  async open(key: string, fsPath: string, text: string, format: string): Promise<DocSyncResult> {
    const session = new this.SessionCtor(text, format);
    const doc: ManagedDoc = { session, fsPath, loadedSchemaSource: undefined, generation: 0 };
    this.docs.set(key, doc);
    return this.syncSchema(key, doc);
  }

  async reparse(key: string, text: string): Promise<DocSyncResult | undefined> {
    const doc = this.docs.get(key);
    if (!doc) return undefined;
    doc.generation += 1;
    const snap = doc.session.dispatch({ ApplyReplace: { path: [], text } }) as SessionSnapshot & { error?: string };
    if (snap.error) {
      // Mid-edit invalid syntax: the session's tree (and therefore its
      // violations/text_range) is untouched at the last valid parse, whose
      // byte positions no longer correspond to the live buffer. Report
      // invalidSyntax so the caller clears diagnostics instead of
      // displaying drifted ranges (spec §"Error handling", Q7).
      return { violations: [], loadError: undefined, invalidSyntax: true };
    }
    return this.syncSchema(key, doc);
  }

  outline(key: string): OutlineNode[] | undefined {
    return this.docs.get(key)?.session.outline();
  }

  schemaHint(key: string, path: Path): EditHint | undefined {
    return this.docs.get(key)?.session.schema_hint(path);
  }

  close(key: string): void {
    this.docs.delete(key);
  }

  private async syncSchema(key: string, doc: ManagedDoc): Promise<DocSyncResult> {
    let snap = doc.session.dispatch("DetectSchema") as SessionSnapshot;
    const detected = snap.schema_fetch_request;
    if (needsSchemaReload(detected, doc.loadedSchemaSource, snap.schema_status)) {
      const generation = doc.generation;
      const text = await this.resolveSchemaText(doc.fsPath, detected!);
      // Stale-fetch guard: discard if the document closed or moved on to a
      // later reparse while this fetch/read was in flight (spec §"Error
      // handling").
      const stillCurrent = this.docs.get(key) === doc && doc.generation === generation;
      if (stillCurrent) {
        snap = doc.session.dispatch({ SchemaLoaded: { source: detected!, text } }) as SessionSnapshot;
        doc.loadedSchemaSource = detected!;
      }
    }
    return {
      violations: doc.session.schema_violations(),
      loadError: snap.schema_status?.load_error,
      invalidSyntax: false,
    };
  }

  private async resolveSchemaText(
    fsPath: string,
    source: SchemaSource,
  ): Promise<{ Ok: string } | { Err: string }> {
    try {
      if ("Local" in source) {
        const resolved = resolveLocalSchemaPath(fsPath, source.Local);
        return { Ok: await this.deps.readFile(resolved) };
      }
      return { Ok: await this.deps.fetchUrl(source.Url) };
    } catch (e) {
      return { Err: e instanceof Error ? e.message : String(e) };
    }
  }
}
