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

// zh-TW translation of HELP_TEXT (Phase 4). Shortcut key names (j/k, Ctrl-s,
// …) and mouse-button names stay untranslated — project/platform vocabulary,
// same rule as the TUI's tui.help.* catalog entries.
const HELP_TEXT_ZH_TW = `confy web — 按鍵
j/k 或 ↑/↓     移動游標
Enter/Space    展開分支／編輯葉節點／啟用
e              編輯（inline 或多行對話框）
a              新增節點 · d 刪除 · c 複製 · x 剪下 · v 貼上
r              remark（節點 ↔ comment 切換）
+/- 或 ←/→     微調數值
z / y          復原／重做
s              切換選取 · 0 全部摺疊 · 9 全部展開
1 / 2          展開／摺疊一層
/              篩選 · f 類型篩選 · K kind 切換 · C 轉換格式
i              詳細資訊彈出視窗 · ? 本說明 · Ctrl-s 儲存 · Ctrl-o 開啟
q              離開（若有未儲存變更會提示）

── 指標裝置 ──────────────────────────────────────
click          選取            ⇧click   範圍選取
⌘click         多選            drag     套索選取／拖曳移動
right-click    右鍵選單

開啟（Ctrl-o）與原地儲存需要 File System Access API
（Chrome/Edge）。其他瀏覽器會改用貼上載入／下載路徑。`;

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
  const helpText = getLang() === "zh-TW" ? HELP_TEXT_ZH_TW : HELP_TEXT;
  const legend =
    getLang() === "zh-TW"
      ? (KIND_LEGEND_ZH_TW[docFormat] ?? "")
      : (KIND_LEGEND[docFormat] ?? "");
  return (helpText + "\n" + legend).split("\n").map(helpLineHTML).join("\n");
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

// zh-TW translation of KIND_LEGEND (Phase 4). Notation suffixes and the
// scalar/container labels themselves (table, inline, array, str, int, float,
// bool, AoT, …) stay untranslated — they're the KIND badge's own vocabulary,
// same rule as the TUI's KIND column legend.
const KIND_LEGEND_ZH_TW: Record<string, string> = {
  Toml: `── KIND 標籤（TOML）──────────────────────────────
容器（label·notation）：
  table·scope    標準 [header] table
  table·dotted   dotted-key table（a.b.c = …）
  inline         inline table { … }
  array·inline   inline array        array·multi  multiline array
  AoT            array-of-tables  [[…]]

純量（label·notation）：
  str            basic string        str·"…"（quoted）
  str·'…'        literal string
  str·"""        multiline basic     str·'''  multiline literal
  int            decimal integer
  int·0x int·0o int·0b   hex／octal／binary
  float / float·dec      float        float·1e  exponent
  float·inf float·nan    infinity／NaN
  bool · date · time · null`,
  Json: `── KIND 標籤（JSON／JSONC）──────────────────────
容器（label·notation）：
  table          object { … }        table·multi  multiline object
  inline         inline object
  array·inline   inline array        array·multi  multiline array

純量（label·notation）：
  str            string              null
  int            integer
  float          float               float·1e  exponent
  bool`,
  Yaml: `── KIND 標籤（YAML）──────────────────────────────
容器（label·notation）：
  table·block    block mapping       table·flow  flow mapping { … }
  array·block    block sequence      array·flow  flow sequence [ … ]
  （opaque 節點 — anchors／aliases／merge／tags — 唯讀）

純量（label·notation）：
  str            plain string        str·'…'  single-quoted
  str·"…"        double-quoted       str·|    literal block
  str·>          folded block
  int            decimal integer     int·0x int·0o  hex／octal
  float          float               float·1e  exponent
  float·inf float·nan    infinity／NaN
  bool · null`,
};
