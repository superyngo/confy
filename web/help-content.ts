// Shared Help/About/KIND-legend text content for the Help overlay (desktop
// `web/ui.ts`) and the touch edit UI (Task 7). Extracted from `web/ui.ts` so
// both surfaces render identical copy.
import { escapeHtml } from "./escape.js";
import { t, tArgs, getLang } from "./i18n.js";
export const HELP_TEXT = `confy web — keys
j/k or ↑/↓     move cursor
Enter/Space    toggle branch / edit leaf / activate
e              edit (inline or multiline modal)
a              add node · d delete · c copy · x cut · v paste
r              remark (toggle node ↔ comment)
+/- or ←/→     nudge numeric value
z / y          undo / redo
s              toggle select · 0 collapse-all · 9 expand-all
1 / 2          expand / collapse one level
/              filter · f type-filter · K kind-switch · C convert
i              detail popup · ? this help · Ctrl-s save · Ctrl-o open
q              quit (prompts if dirty)

── pointer ──────────────────────────────────────
click          select          ⇧click   range-select
⌘click         multi-select    drag     marquee / move
right-click    context menu

Open (Ctrl-o) and in-place Save need the File System Access API
(Chrome/Edge). Other browsers fall back to the paste-load / download path.`;

// Per-format KIND legend appended to the Help overlay, keyed by `doc_format`
// (ported from the TUI's TOML_HELP/JSON_HELP/YAML_HELP KIND column). The kind
// badge shows the friendly label + notation suffix; this explains what each
// notation means for the open file's backend.
// One Help line → HTML: the aligned columns alternate key/description (some
// lines carry two pairs), so wrap every even content segment in a .help-key
// span. Splitting on runs of 2+ spaces with a capture keeps the separators, so
// the <pre> alignment survives untouched. Lines without a 2+-space split
// (titles, prose, "Containers…:" headings) stay plain; `──` rules get their
// own .help-sect span.
function helpLineHTML(line: string): string {
  if (line.startsWith("──"))
    return `<span class="help-sect">${escapeHtml(line)}</span>`;
  const parts = line.split(/(\s{2,})/);
  const contentCount = parts.filter((p, i) => i % 2 === 0 && p !== "").length;
  if (contentCount < 2) return escapeHtml(line);
  let content = 0;
  return parts
    .map((p, i) => {
      if (i % 2 === 1 || p === "") return escapeHtml(p);
      return content++ % 2 === 0
        ? `<span class="help-key">${escapeHtml(p)}</span>`
        : escapeHtml(p);
    })
    .join("");
}

// Shared Help/About body composition, used by both the desktop overlay
// (`web/ui.ts`) and the touch sheet (`web/touch/app.ts`). Returns HTML ready
// to drop inside a <pre> — the caller must NOT escape it again.
//
// `aboutText` is the core-catalog body (`ConfySession.about_text()`, mirrors
// `crates/confy-core/src/session/state.rs::about_text`) — the web layer no
// longer hand-mirrors it (that was a documented drift hazard). Two host-owned
// lines are appended, mirroring the TUI's `tui.about.language`/`Config:`
// disclosure: the active language code, and where the preference is stored
// (browser localStorage — no filesystem path to disclose on the web/desktop
// host, unlike the TUI's config file).
export function helpBodyHTML(
  tab: "Help" | "About",
  docFormat: string,
  aboutText: string,
): string {
  if (tab === "About") {
    const body =
      aboutText.replace(/\n+$/, "") +
      "\n\n" +
      tArgs("web.about.language", [getLang()]) +
      "\n" +
      t("web.about.storage");
    return escapeHtml(body).replace(
      /(https:\/\/\S+)/,
      '<a href="$1" target="_blank" rel="noopener noreferrer">$1</a>',
    );
  }
  return (HELP_TEXT + "\n" + (KIND_LEGEND[docFormat] ?? ""))
    .split("\n")
    .map(helpLineHTML)
    .join("\n");
}

export const KIND_LEGEND: Record<string, string> = {
  Toml: `── KIND badge (TOML) ──────────────────────────────
Containers (label·notation):
  table·scope    standard [header] table
  table·dotted   dotted-key table (a.b.c = …)
  inline         inline table { … }
  array·inline   inline array        array·multi  multiline array
  AoT            array-of-tables  [[…]]

Scalars (label·notation):
  str            basic string        str·"…"  (quoted)
  str·'…'        literal string
  str·"""        multiline basic     str·'''  multiline literal
  int            decimal integer
  int·0x int·0o int·0b   hex / octal / binary
  float / float·dec      float        float·1e  exponent
  float·inf float·nan    infinity / NaN
  bool · date · time · null`,
  Json: `── KIND badge (JSON / JSONC) ──────────────────────
Containers (label·notation):
  table          object { … }        table·multi  multiline object
  inline         inline object
  array·inline   inline array        array·multi  multiline array

Scalars (label·notation):
  str            string              null
  int            integer
  float          float               float·1e  exponent
  bool`,
  Yaml: `── KIND badge (YAML) ──────────────────────────────
Containers (label·notation):
  table·block    block mapping       table·flow  flow mapping { … }
  array·block    block sequence      array·flow  flow sequence [ … ]
  (opaque nodes — anchors/aliases/merge/tags — are read-only)

Scalars (label·notation):
  str            plain string        str·'…'  single-quoted
  str·"…"        double-quoted       str·|    literal block
  str·>          folded block
  int            decimal integer     int·0x int·0o  hex / octal
  float          float               float·1e  exponent
  float·inf float·nan    infinity / NaN
  bool · null`,
};
