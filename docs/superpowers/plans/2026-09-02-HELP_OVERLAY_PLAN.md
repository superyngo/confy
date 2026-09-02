
# Help overlay keymap alignment & visual polish

## Context

The `?` Help overlay's keymap content is inconsistent between TUI and Web: the TUI packs two key/description pairs per line in an ad-hoc four-column layout, the Web cheatsheet strings multiple bindings together with `·`, neither is a clean two-column (keys left, description right) layout, the TUI popup has no padding between its border/title and the first content row, and some zh-TW strings use looser wording (e.g. "多行對話框"/"強制對話框" for what is actually the external editor). This plan restructures both surfaces' Help keymap content into a shared, grouped, two-column model (Navigation / Selection / Edit / File & App, keys joined with `/` within a row), aligns identical bindings to identical wording between TUI and Web, fixes the missing TUI padding, reflows the Kind-legend glossary into one tag/description pair per row on both surfaces, and expands `docs/reference/KEYMAP.md` to also document the Help-overlay and inline/external-editor parity (not just raw key bindings). No new automated drift test is added (explicit user decision) — the existing `keys.rs` keymap-doc tests are untouched and still verify `map_key`/`resolveKeyIntent` against `KEYMAP.md`.

## Approach

### 1. i18n: add the shared row/section catalog, remove the old TUI blobs, reflow the Web legend

Edit `i18n/en.json` and `i18n/zh-TW.json` (flat `key -> string` maps, both files must stay in sync — `tr`/`t` fall back to `en` then to the raw key, so an accidental omission never panics but must not happen here).

**Remove** these 3 keys from both files (their content moves into code-composed sections in step 2; grep confirms the only other references are `crates/confy-tui/src/tui/keys.rs` (updated in step 2) and historical `CHANGELOG.md` prose, which is not touched):
- `tui.help.toml`
- `tui.help.json`
- `tui.help.yaml`

**Add** the following 119 keys to *both* `i18n/en.json` (values below) and `i18n/zh-TW.json` (zh values below), preserving each file's existing indentation/key order convention (append near the other `tui.help.*`/`web.help.*`/`help.*` keys). Insert alphabetically-adjacent to existing `tui.*`/`web.*` blocks is not required — flat file, any position is fine as long as both files gain the identical key set.

Add to `i18n/en.json`:
```json
{
  "help.section.nav": "Navigation",
  "help.section.select": "Selection",
  "help.section.edit": "Edit",
  "help.section.file": "File & App",
  "tui.help.section.keysign": "Key sign (first 3 chars)",
  "tui.help.section.containers": "Containers",
  "tui.help.section.scalars": "Scalars [type:format]",
  "tui.help.section.legend": "Kind legend",
  "help.row.move_cursor": "Move cursor",
  "help.row.first_last_row": "First/last row",
  "help.row.page": "Page up/down",
  "help.row.expand_collapse_level": "Expand/collapse one level (subtree / ascend)",
  "help.row.collapse_expand_all": "Collapse/expand all",
  "help.row.space_toggle": "Toggle branch · open leaf detail",
  "help.row.detail": "Detail panel (any node)",
  "help.row.toggle_select": "Toggle select",
  "help.row.range_select": "Range select",
  "help.row.fuzzy_filter": "Fuzzy filter",
  "help.row.type_filter": "Type filter (checkbox menu)",
  "help.row.clear_esc": "Clear filter/selection · cancel · close overlay",
  "help.row.edit": "Edit (inline or editor)",
  "help.row.force_editor": "Force editor (any node)",
  "help.row.rename": "Rename key (inline, works for all node types)",
  "help.row.add_node": "Add node",
  "help.row.delete": "Delete",
  "help.row.copy_cut_paste": "Copy/cut/paste",
  "help.row.nudge": "±1 number",
  "help.row.undo_redo": "Undo/redo",
  "help.row.convert": "Convert document to another format (Root node)",
  "help.row.save": "Save",
  "help.row.open": "Open",
  "help.row.action_menu": "Action menu",
  "help.row.lang_picker": "Language picker",
  "help.row.help": "Show this help",
  "help.row.diag": "Diagnostics overlay (developer-facing event ring)",
  "help.row.quit": "Quit (prompts if dirty)",
  "help.row.remark.toml": "Remark toggle (node ↔ comment)",
  "help.row.remark.json": "Remark toggle (comment out with //)",
  "help.row.remark.yaml": "Remark toggle (comment out with #)",
  "help.row.kind_switch.toml": "Kind switch (scalar type / table & array notation)",
  "help.row.kind_switch.json": "Kind switch (object/array inline↔multiline, float plain↔exponent)",
  "help.row.kind_switch.yaml": "Kind switch (map/seq block↔flow, string style, int radix, float plain↔exp)",
  "tui.help.row.filter_lock": "Lock in filtered list",
  "tui.help.row.convert_jsonc_toggle": "Convert Path step: toggle .json/.jsonc extension",
  "web.help.row.save_vscode": "Save (shared with VS Code)",
  "web.help.row.undo_redo_vscode": "Undo/redo (shared with VS Code — the workbench owns the stack)",
  "web.help.row.save_as_convert": "Save As / Convert…",
  "web.help.note.vscode": "Save As / Convert, Help, About, and language live in the tab's \"…\" More Actions menu. The title-bar \"Reopen as Text Editor\" / \"Open Text Editor to the Side\" buttons swap/split to the raw text view; while a side-by-side text edit doesn't parse, the tree dims and pauses until the text parses again.",
  "web.help.section.pointer": "Pointer",
  "web.help.row.pointer_select": "Select",
  "web.help.row.pointer_range": "Range-select",
  "web.help.row.pointer_multi": "Multi-select",
  "web.help.row.pointer_drag": "Marquee / move",
  "tui.help.legend.toml.keysign.1": "(B)|bare key",
  "tui.help.legend.toml.keysign.2": "(Q)|quoted key",
  "tui.help.legend.toml.keysign.3": "(D)|dotted key",
  "tui.help.legend.toml.keysign.4": "(-)|no key",
  "tui.help.legend.toml.containers.1": "[G]|root/file node",
  "tui.help.legend.toml.containers.2": "[C]|comment node",
  "tui.help.legend.toml.containers.3": "[A/I]|inline array",
  "tui.help.legend.toml.containers.4": "[A/M]|multiline array",
  "tui.help.legend.toml.containers.5": "[A/T]|array-of-tables",
  "tui.help.legend.toml.containers.6": "[T/I]|inline table",
  "tui.help.legend.toml.containers.7": "[T/S]|table scope (standard [header])",
  "tui.help.legend.toml.containers.8": "[T/D]|dotted-key table (a.b.c = …)",
  "tui.help.legend.toml.scalars.1": "[S:str ]|basic string",
  "tui.help.legend.toml.scalars.2": "[S:mstr]|multiline basic string",
  "tui.help.legend.toml.scalars.3": "[S:lit ]|literal string",
  "tui.help.legend.toml.scalars.4": "[S:mlit]|multiline literal string",
  "tui.help.legend.toml.scalars.5": "[I:dec ]|decimal integer",
  "tui.help.legend.toml.scalars.6": "[I:hex ]|hex integer",
  "tui.help.legend.toml.scalars.7": "[I:oct ]|octal integer",
  "tui.help.legend.toml.scalars.8": "[I:bin ]|binary integer",
  "tui.help.legend.toml.scalars.9": "[F:flt ]|float",
  "tui.help.legend.toml.scalars.10": "[F:inf ]|infinity",
  "tui.help.legend.toml.scalars.11": "[F:nan ]|NaN",
  "tui.help.legend.toml.scalars.12": "[B:bool]|boolean",
  "tui.help.legend.toml.scalars.13": "[D:odt ]|offset datetime",
  "tui.help.legend.toml.scalars.14": "[D:ldt ]|local datetime",
  "tui.help.legend.toml.scalars.15": "[D:ldat]|local date",
  "tui.help.legend.toml.scalars.16": "[D:ltim]|local time",
  "tui.help.legend.json.keysign.1": "(Q)|quoted key",
  "tui.help.legend.json.keysign.2": "(-)|no key",
  "tui.help.legend.json.containers.1": "[G]|root/file node",
  "tui.help.legend.json.containers.2": "[C]|comment node (// line editable; /* */ block read-only)",
  "tui.help.legend.json.containers.3": "[A/I]|inline array",
  "tui.help.legend.json.containers.4": "[A/M]|multiline array",
  "tui.help.legend.json.containers.5": "[T/I]|inline object",
  "tui.help.legend.json.containers.6": "[T/M]|multiline object",
  "tui.help.legend.json.scalars.1": "[S:str ]|string",
  "tui.help.legend.json.scalars.2": "[S:null]|null",
  "tui.help.legend.json.scalars.3": "[I:dec ]|integer",
  "tui.help.legend.json.scalars.4": "[F:flt ]|float",
  "tui.help.legend.json.scalars.5": "[F:exp ]|exponent float",
  "tui.help.legend.json.scalars.6": "[B:bool]|boolean",
  "tui.help.legend.yaml.keysign.1": "(B)|bare key",
  "tui.help.legend.yaml.keysign.2": "(Q)|quoted key",
  "tui.help.legend.yaml.keysign.3": "(-)|no key",
  "tui.help.legend.yaml.containers.1": "[G]|root/file node",
  "tui.help.legend.yaml.containers.2": "[C]|comment node",
  "tui.help.legend.yaml.containers.3": "[A/B]|block sequence",
  "tui.help.legend.yaml.containers.4": "[A/F]|flow sequence",
  "tui.help.legend.yaml.containers.5": "[T/B]|block mapping",
  "tui.help.legend.yaml.containers.6": "[T/F]|flow mapping",
  "tui.help.legend.yaml.containers.7": "[opaq ]|out-of-subset, read-only (anchors, aliases, merge, tags)",
  "tui.help.legend.yaml.scalars.1": "[S:str ]|plain string",
  "tui.help.legend.yaml.scalars.2": "[S:sq  ]|single-quoted string",
  "tui.help.legend.yaml.scalars.3": "[S:dq  ]|double-quoted",
  "tui.help.legend.yaml.scalars.4": "[S:lit ]|literal block (|)",
  "tui.help.legend.yaml.scalars.5": "[S:fold]|folded block (>)",
  "tui.help.legend.yaml.scalars.6": "[I:dec ]|decimal integer",
  "tui.help.legend.yaml.scalars.7": "[I:hex ]|hex integer",
  "tui.help.legend.yaml.scalars.8": "[I:oct ]|octal integer",
  "tui.help.legend.yaml.scalars.9": "[F:flt ]|float",
  "tui.help.legend.yaml.scalars.10": "[F:exp ]|exponent float",
  "tui.help.legend.yaml.scalars.11": "[F:inf ]|infinity",
  "tui.help.legend.yaml.scalars.12": "[F:nan ]|NaN",
  "tui.help.legend.yaml.scalars.13": "[B:bool]|boolean",
  "tui.help.legend.yaml.scalars.14": "[S:null]|null"
}
```

Add to `i18n/zh-TW.json` (same 119 keys, zh values — `bare key`/`quoted key`/`dotted key`/technical labels intentionally stay untranslated, matching the existing convention already used throughout `tui.help.*`):
```json
{
  "help.section.nav": "導覽",
  "help.section.select": "選取",
  "help.section.edit": "編輯",
  "help.section.file": "檔案與應用程式",
  "tui.help.section.keysign": "Key 符號（前 3 字元）",
  "tui.help.section.containers": "容器類型",
  "tui.help.section.scalars": "純量類型 [type:format]",
  "tui.help.section.legend": "Kind 圖例",
  "help.row.move_cursor": "移動游標",
  "help.row.first_last_row": "第一列／最後一列",
  "help.row.page": "上下翻頁",
  "help.row.expand_collapse_level": "展開／摺疊一層（子樹／收合）",
  "help.row.collapse_expand_all": "全部摺疊／展開",
  "help.row.space_toggle": "切換分支展開；葉節點則開啟詳細資訊",
  "help.row.detail": "詳細資訊面板（任何節點）",
  "help.row.toggle_select": "切換選取",
  "help.row.range_select": "範圍選取",
  "help.row.fuzzy_filter": "模糊篩選",
  "help.row.type_filter": "類型篩選（核取方塊選單）",
  "help.row.clear_esc": "清除篩選／選取；取消；關閉彈出視窗",
  "help.row.edit": "編輯（inline 或編輯器）",
  "help.row.force_editor": "強制開啟編輯器（任何節點）",
  "help.row.rename": "重新命名 key（inline，適用於所有節點類型）",
  "help.row.add_node": "新增節點",
  "help.row.delete": "刪除",
  "help.row.copy_cut_paste": "複製／剪下／貼上",
  "help.row.nudge": "數字 ±1",
  "help.row.undo_redo": "復原／重做",
  "help.row.convert": "將文件轉換為其他格式（Root 節點）",
  "help.row.save": "儲存",
  "help.row.open": "開啟",
  "help.row.action_menu": "動作選單",
  "help.row.lang_picker": "語言選擇器",
  "help.row.help": "顯示此說明",
  "help.row.diag": "診斷疊層（開發者事件紀錄）",
  "help.row.quit": "離開（若有未儲存變更會提示）",
  "help.row.remark.toml": "切換 remark（節點 ↔ comment）",
  "help.row.remark.json": "切換 remark（以 // 註解）",
  "help.row.remark.yaml": "切換 remark（以 # 註解）",
  "help.row.kind_switch.toml": "切換 kind（純量類型／table 與 array 記法）",
  "help.row.kind_switch.json": "切換 kind（object/array inline↔multiline、float plain↔exponent）",
  "help.row.kind_switch.yaml": "切換 kind（map/seq block↔flow、字串樣式、整數進位制、float plain↔exp）",
  "tui.help.row.filter_lock": "鎖定篩選結果",
  "tui.help.row.convert_jsonc_toggle": "轉換格式 Path 步驟：切換 .json/.jsonc 副檔名",
  "web.help.row.save_vscode": "儲存（與 VS Code 共用）",
  "web.help.row.undo_redo_vscode": "復原／重做（與 VS Code 共用 — workbench 掌管復原堆疊）",
  "web.help.row.save_as_convert": "另存新檔／轉換格式…",
  "web.help.note.vscode": "另存新檔／轉換格式、說明、關於、語言選擇都在分頁的「…」更多動作選單中。標題列的「以文字編輯器重新開啟」／「在旁開啟文字編輯器」按鈕會切換／並排顯示原始文字檢視；並排的文字編輯若無法解析，樹狀畫面會變暗並暫停，直到文字再次可解析為止。",
  "web.help.section.pointer": "指標裝置",
  "web.help.row.pointer_select": "選取",
  "web.help.row.pointer_range": "範圍選取",
  "web.help.row.pointer_multi": "多選",
  "web.help.row.pointer_drag": "套索選取／拖曳移動",
  "tui.help.legend.toml.keysign.1": "(B)|bare key",
  "tui.help.legend.toml.keysign.2": "(Q)|quoted key",
  "tui.help.legend.toml.keysign.3": "(D)|dotted key",
  "tui.help.legend.toml.keysign.4": "(-)|無 key",
  "tui.help.legend.toml.containers.1": "[G]|root/file 節點",
  "tui.help.legend.toml.containers.2": "[C]|comment 節點",
  "tui.help.legend.toml.containers.3": "[A/I]|inline array",
  "tui.help.legend.toml.containers.4": "[A/M]|multiline array",
  "tui.help.legend.toml.containers.5": "[A/T]|array-of-tables",
  "tui.help.legend.toml.containers.6": "[T/I]|inline table",
  "tui.help.legend.toml.containers.7": "[T/S]|table scope（標準 [header]）",
  "tui.help.legend.toml.containers.8": "[T/D]|dotted-key table（a.b.c = …）",
  "tui.help.legend.toml.scalars.1": "[S:str ]|basic string",
  "tui.help.legend.toml.scalars.2": "[S:mstr]|multiline basic string",
  "tui.help.legend.toml.scalars.3": "[S:lit ]|literal string",
  "tui.help.legend.toml.scalars.4": "[S:mlit]|multiline literal string",
  "tui.help.legend.toml.scalars.5": "[I:dec ]|decimal integer",
  "tui.help.legend.toml.scalars.6": "[I:hex ]|hex integer",
  "tui.help.legend.toml.scalars.7": "[I:oct ]|octal integer",
  "tui.help.legend.toml.scalars.8": "[I:bin ]|binary integer",
  "tui.help.legend.toml.scalars.9": "[F:flt ]|float",
  "tui.help.legend.toml.scalars.10": "[F:inf ]|infinity",
  "tui.help.legend.toml.scalars.11": "[F:nan ]|NaN",
  "tui.help.legend.toml.scalars.12": "[B:bool]|boolean",
  "tui.help.legend.toml.scalars.13": "[D:odt ]|offset datetime",
  "tui.help.legend.toml.scalars.14": "[D:ldt ]|local datetime",
  "tui.help.legend.toml.scalars.15": "[D:ldat]|local date",
  "tui.help.legend.toml.scalars.16": "[D:ltim]|local time",
  "tui.help.legend.json.keysign.1": "(Q)|quoted key",
  "tui.help.legend.json.keysign.2": "(-)|無 key",
  "tui.help.legend.json.containers.1": "[G]|root/file 節點",
  "tui.help.legend.json.containers.2": "[C]|comment 節點（// 單行可編輯；/* */ 區塊為唯讀）",
  "tui.help.legend.json.containers.3": "[A/I]|inline array",
  "tui.help.legend.json.containers.4": "[A/M]|multiline array",
  "tui.help.legend.json.containers.5": "[T/I]|inline object",
  "tui.help.legend.json.containers.6": "[T/M]|multiline object",
  "tui.help.legend.json.scalars.1": "[S:str ]|string",
  "tui.help.legend.json.scalars.2": "[S:null]|null",
  "tui.help.legend.json.scalars.3": "[I:dec ]|integer",
  "tui.help.legend.json.scalars.4": "[F:flt ]|float",
  "tui.help.legend.json.scalars.5": "[F:exp ]|exponent float",
  "tui.help.legend.json.scalars.6": "[B:bool]|boolean",
  "tui.help.legend.yaml.keysign.1": "(B)|bare key",
  "tui.help.legend.yaml.keysign.2": "(Q)|quoted key",
  "tui.help.legend.yaml.keysign.3": "(-)|無 key",
  "tui.help.legend.yaml.containers.1": "[G]|root/file 節點",
  "tui.help.legend.yaml.containers.2": "[C]|comment 節點",
  "tui.help.legend.yaml.containers.3": "[A/B]|block sequence",
  "tui.help.legend.yaml.containers.4": "[A/F]|flow sequence",
  "tui.help.legend.yaml.containers.5": "[T/B]|block mapping",
  "tui.help.legend.yaml.containers.6": "[T/F]|flow mapping",
  "tui.help.legend.yaml.containers.7": "[opaq ]|子集之外，唯讀（anchors、aliases、merge、tags）",
  "tui.help.legend.yaml.scalars.1": "[S:str ]|plain string",
  "tui.help.legend.yaml.scalars.2": "[S:sq  ]|single-quoted string",
  "tui.help.legend.yaml.scalars.3": "[S:dq  ]|double-quoted",
  "tui.help.legend.yaml.scalars.4": "[S:lit ]|literal block (|)",
  "tui.help.legend.yaml.scalars.5": "[S:fold]|folded block (>)",
  "tui.help.legend.yaml.scalars.6": "[I:dec ]|decimal integer",
  "tui.help.legend.yaml.scalars.7": "[I:hex ]|hex integer",
  "tui.help.legend.yaml.scalars.8": "[I:oct ]|octal integer",
  "tui.help.legend.yaml.scalars.9": "[F:flt ]|float",
  "tui.help.legend.yaml.scalars.10": "[F:exp ]|exponent float",
  "tui.help.legend.yaml.scalars.11": "[F:inf ]|infinity",
  "tui.help.legend.yaml.scalars.12": "[F:nan ]|NaN",
  "tui.help.legend.yaml.scalars.13": "[B:bool]|boolean",
  "tui.help.legend.yaml.scalars.14": "[S:null]|null"
}
```

**Edit in place** (content-only, no key changes) the 6 existing `web.help.legend.{toml,json,yaml}` keys in both files, reflowing every line that packs two label/description pairs into one line so each pair is on its own line (mechanical split only — no wording changes except the one pre-existing zh-TW TOML line noted below that was missing a separator and gets one added for consistency with its own English counterpart). Replace each value with (newlines shown as line breaks inside the JSON string, i.e. `\n`):

`i18n/en.json` → `web.help.legend.toml`:
```
── KIND badge (TOML) ──────────────────────────────
Containers (label·notation):
  table·scope    standard [header] table
  table·dotted   dotted-key table (a.b.c = …)
  inline         inline table { … }
  array·inline  inline array
  array·multi  multiline array
  AoT            array-of-tables  [[…]]

Scalars (label·notation):
  str  basic string
  str·"…"  (quoted)
  str·'…'        literal string
  str·"""  multiline basic
  str·'''  multiline literal
  int            decimal integer
  int·0x int·0o int·0b   hex / octal / binary
  float / float·dec  float
  float·1e  exponent
  float·inf float·nan    infinity / NaN
  bool · date · time · null
```

`i18n/zh-TW.json` → `web.help.legend.toml`:
```
── KIND 標籤（TOML）──────────────────────────────
容器（label·notation）：
  table·scope    標準 [header] table
  table·dotted   dotted-key table（a.b.c = …）
  inline         inline table { … }
  array·inline  inline array
  array·multi  multiline array
  AoT            array-of-tables  [[…]]

純量（label·notation）：
  str            basic string
  str·"…"（quoted）
  str·'…'        literal string
  str·"""  multiline basic
  str·'''  multiline literal
  int            decimal integer
  int·0x int·0o int·0b   hex／octal／binary
  float / float·dec  float
  float·1e  exponent
  float·inf float·nan    infinity／NaN
  bool · date · time · null
```

`i18n/en.json` → `web.help.legend.json`:
```
── KIND badge (JSON / JSONC) ──────────────────────
Containers (label·notation):
  table  object { … }
  table·multi  multiline object
  inline         inline object
  array·inline  inline array
  array·multi  multiline array

Scalars (label·notation):
  str            string              null
  int            integer
  float  float
  float·1e  exponent
  bool
```

`i18n/zh-TW.json` → `web.help.legend.json`:
```
── KIND 標籤（JSON／JSONC）──────────────────────
容器（label·notation）：
  table  object { … }
  table·multi  multiline object
  inline         inline object
  array·inline  inline array
  array·multi  multiline array

純量（label·notation）：
  str            string              null
  int            integer
  float  float
  float·1e  exponent
  bool
```

`i18n/en.json` → `web.help.legend.yaml`:
```
── KIND badge (YAML) ──────────────────────────────
Containers (label·notation):
  table·block  block mapping
  table·flow  flow mapping { … }
  array·block  block sequence
  array·flow  flow sequence [ … ]
  (opaque nodes — anchors/aliases/merge/tags — are read-only)

Scalars (label·notation):
  str  plain string
  str·'…'  single-quoted
  str·"…"  double-quoted
  str·|  literal block
  str·>          folded block
  int  decimal integer
  int·0x int·0o  hex / octal
  float  float
  float·1e  exponent
  float·inf float·nan    infinity / NaN
  bool · null
```

`i18n/zh-TW.json` → `web.help.legend.yaml`:
```
── KIND 標籤（YAML）──────────────────────────────
容器（label·notation）：
  table·block  block mapping
  table·flow  flow mapping { … }
  array·block  block sequence
  array·flow  flow sequence [ … ]
  （opaque 節點 — anchors／aliases／merge／tags — 唯讀）

純量（label·notation）：
  str  plain string
  str·'…'  single-quoted
  str·"…"  double-quoted
  str·|  literal block
  str·>          folded block
  int  decimal integer
  int·0x int·0o  hex／octal
  float  float
  float·1e  exponent
  float·inf float·nan    infinity／NaN
  bool · null
```

`web.help.title` (`"confy web — keys"` / `"confy web — 按鍵"`) is already unreferenced by any `.ts` file (verified: only appears in the two i18n JSON files) — pre-existing dead key, leave it untouched.

### 2. TUI: rewrite `keys::help_text` as a shared Section/Row model with unicode-width-aligned two columns

Edit `crates/confy-tui/src/tui/keys.rs`. Replace the current `help_text` function (lines 106–118) and everything it needs with the following (insert `use unicode_width::UnicodeWidthStr;` at the top of the function bodies that need it, following the exact pattern already used in `crates/confy-tui/src/tui/overlay_lang_picker.rs` and `crates/confy-tui/src/tui/ui.rs::draw_title` — `unicode-width = "0.2"` is already a dependency, no `Cargo.toml` change needed):

```rust
struct HelpRow {
    keys: &'static str,
    desc_key: &'static str,
}

struct HelpSection {
    title_key: &'static str,
    rows: Vec<HelpRow>,
}

/// The `?` overlay's keymap content, grouped and two-column (keys left,
/// description right). `r`/`K` descriptions are format-specific.
fn help_sections(format: crate::model::document::DocFormat) -> Vec<HelpSection> {
    use crate::model::document::DocFormat;
    let (remark_key, kind_key) = match format {
        DocFormat::Toml => ("help.row.remark.toml", "help.row.kind_switch.toml"),
        DocFormat::Json => ("help.row.remark.json", "help.row.kind_switch.json"),
        DocFormat::Yaml => ("help.row.remark.yaml", "help.row.kind_switch.yaml"),
    };
    vec![
        HelpSection {
            title_key: "help.section.nav",
            rows: vec![
                HelpRow { keys: "j/k/↑/↓", desc_key: "help.row.move_cursor" },
                HelpRow { keys: "Home/End", desc_key: "help.row.first_last_row" },
                HelpRow { keys: "PgUp/PgDn", desc_key: "help.row.page" },
                HelpRow { keys: "1/2", desc_key: "help.row.expand_collapse_level" },
                HelpRow { keys: "0/9", desc_key: "help.row.collapse_expand_all" },
                HelpRow { keys: "Space", desc_key: "help.row.space_toggle" },
                HelpRow { keys: "Enter/i", desc_key: "help.row.detail" },
            ],
        },
        HelpSection {
            title_key: "help.section.select",
            rows: vec![
                HelpRow { keys: "s", desc_key: "help.row.toggle_select" },
                HelpRow { keys: "Shift+↑/↓", desc_key: "help.row.range_select" },
                HelpRow { keys: "/", desc_key: "help.row.fuzzy_filter" },
                HelpRow { keys: "/…Enter", desc_key: "tui.help.row.filter_lock" },
                HelpRow { keys: "f", desc_key: "help.row.type_filter" },
                HelpRow { keys: "Esc", desc_key: "help.row.clear_esc" },
            ],
        },
        HelpSection {
            title_key: "help.section.edit",
            rows: vec![
                HelpRow { keys: "e", desc_key: "help.row.edit" },
                HelpRow { keys: "E", desc_key: "help.row.force_editor" },
                HelpRow { keys: "F2", desc_key: "help.row.rename" },
                HelpRow { keys: "a", desc_key: "help.row.add_node" },
                HelpRow { keys: "d/Del", desc_key: "help.row.delete" },
                HelpRow { keys: "x/c/v", desc_key: "help.row.copy_cut_paste" },
                HelpRow { keys: "←/→", desc_key: "help.row.nudge" },
                HelpRow { keys: "r", desc_key: remark_key },
                HelpRow { keys: "K", desc_key: kind_key },
                HelpRow { keys: "z/y", desc_key: "help.row.undo_redo" },
                HelpRow { keys: "C", desc_key: "help.row.convert" },
                HelpRow { keys: "Tab", desc_key: "tui.help.row.convert_jsonc_toggle" },
                HelpRow { keys: "l", desc_key: "help.row.lang_picker" },
            ],
        },
        HelpSection {
            title_key: "help.section.file",
            rows: vec![
                HelpRow { keys: "Ctrl+s/w", desc_key: "help.row.save" },
                HelpRow { keys: "m", desc_key: "help.row.action_menu" },
                HelpRow { keys: "~", desc_key: "help.row.diag" },
                HelpRow { keys: "?", desc_key: "help.row.help" },
                HelpRow { keys: "q", desc_key: "help.row.quit" },
            ],
        },
    ]
}

/// Renders `sections` as ` key<pad>  description` lines, one shared column
/// width across all sections computed by *display* width (`unicode-width`,
/// already a workspace dependency) so a zh-TW description never desyncs the
/// key column the way naive `char`-count padding would.
fn render_help_sections(lang: confy_core::session::Lang, sections: &[HelpSection]) -> String {
    use confy_core::session::tr;
    use unicode_width::UnicodeWidthStr;
    let col = sections
        .iter()
        .flat_map(|s| s.rows.iter())
        .map(|r| r.keys.width())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for s in sections {
        out.push_str(&format!(" ── {} ──\n", tr(lang, s.title_key)));
        for r in &s.rows {
            let pad = " ".repeat(col.saturating_sub(r.keys.width()));
            out.push_str(&format!(" {}{}  {}\n", r.keys, pad, tr(lang, r.desc_key)));
        }
        out.push('\n');
    }
    out
}

/// The Kind-legend glossary: 3 groups (`keysign`/`containers`/`scalars`) per
/// `DocFormat`, sourced from `tui.help.legend.<format>.<group>.<n>` catalog
/// entries encoded as `"<tag>|<description>"` (`split_once('|')`). One shared
/// column width across all 3 groups for this format.
fn help_legend_text(
    format: crate::model::document::DocFormat,
    lang: confy_core::session::Lang,
) -> String {
    use crate::model::document::DocFormat;
    use confy_core::session::tr;
    use unicode_width::UnicodeWidthStr;
    let (fmt_str, counts): (&str, [usize; 3]) = match format {
        DocFormat::Toml => ("toml", [4, 8, 16]),
        DocFormat::Json => ("json", [2, 6, 6]),
        DocFormat::Yaml => ("yaml", [3, 7, 14]),
    };
    let groups = ["keysign", "containers", "scalars"];
    let group_title_keys = [
        "tui.help.section.keysign",
        "tui.help.section.containers",
        "tui.help.section.scalars",
    ];
    let mut rows: Vec<(usize, String, String)> = Vec::new();
    for (gi, (grp, count)) in groups.iter().zip(counts.iter()).enumerate() {
        for i in 1..=*count {
            let key = format!("tui.help.legend.{fmt_str}.{grp}.{i}");
            let raw = tr(lang, &key);
            let (tag, desc) = raw.split_once('|').unwrap_or((raw, ""));
            rows.push((gi, tag.to_string(), desc.to_string()));
        }
    }
    let col = rows.iter().map(|(_, t, _)| t.width()).max().unwrap_or(0);
    let mut out = String::new();
    out.push_str(&format!(" ── {} ──\n", tr(lang, "tui.help.section.legend")));
    for (gi, title_key) in group_title_keys.iter().enumerate() {
        out.push_str(&format!(" {}\n", tr(lang, title_key)));
        for (_, tag, desc) in rows.iter().filter(|(g, _, _)| *g == gi) {
            let pad = " ".repeat(col.saturating_sub(tag.width()));
            out.push_str(&format!("   {tag}{pad}  {desc}\n"));
        }
        out.push('\n');
    }
    out
}

/// Keybinding help text, displayed in the `?` overlay. Format-specific: the
/// op list and KIND legend differ per backend. Routed through the `tui.*`/
/// `help.*` catalog (i18n Phase 2+) -- see `docs/reference/KEYMAP.md` §Help
/// overlay parity for the shared row model this composes.
pub fn help_text(
    format: crate::model::document::DocFormat,
    lang: confy_core::session::Lang,
) -> String {
    let sections = help_sections(format);
    let mut out = render_help_sections(lang, &sections);
    out.push_str(&help_legend_text(format, lang));
    out
}
```

Do not change the 3 existing tests `json_help_differs_from_toml`, `yaml_help_differs_from_toml`, `help_text_is_translated_for_zh_tw` (lines 125–160) — every assertion they make (`"//"`, `"[S:null]"`, `!"dotted"`, `!"[A/T]"`, `"[opaq ]"`, `"block"`, `"flow"`, `"[D:odt ]"`, `"Ctrl+s"`) still holds against the new output; verify this in step "Verification" rather than pre-editing them.

### 3. TUI: add popup padding, adjust the two geometry calculations that mirror it

Edit `crates/confy-tui/src/tui/overlay_help.rs`:
- In `draw_help_overlay`, add `.padding(ratatui::widgets::Padding::new(2, 2, 1, 1))` to the `Block` (between `.borders(Borders::ALL)` and `.style(...)`) — left/right 2 cells, top/bottom 1 row, fixing the "no padding between title and content" complaint.
- The popup height calculation `let line_count = wrapped_line_count(&text, popup_width.saturating_sub(2)) as u16; let height = (line_count + 2).min(f.area().height);` must account for the new padding: change `popup_width.saturating_sub(2)` to `popup_width.saturating_sub(6)` (2 border cols + 2+2 padding cols) and `(line_count + 2)` to `(line_count + 4)` (2 border rows + 1+1 padding rows).

Edit `crates/confy-tui/src/tui/mod.rs` (lines ~265–272, the comment-labelled "Mirror draw_help_overlay's geometry" block): change `let inner_w = popup_width.saturating_sub(2);` to `let inner_w = popup_width.saturating_sub(6);` and `let inner_h = size.height.saturating_sub(2);` to `let inner_h = size.height.saturating_sub(4);`, updating the comment to mention the padding too. This keeps the scroll/page clamp math consistent with the new rendered geometry.

### 4. Web: rewrite `help-content.ts` as the same Section/Row model, rendered as a CSS grid

Replace `web/help-content.ts` in full. Delete `HELP_TEXT`, `HELP_TEXT_ZH_TW`, `HELP_TEXT_VSCODE`, `HELP_TEXT_VSCODE_ZH_TW` (confirmed unused outside this file) and keep `helpLineHTML` unchanged (still used for the legend glossary, which keeps its distinct `label·notation` vocabulary — see `docs/reference/KEYMAP.md` §Help overlay parity for why TUI/Web legends are not unified). New file body:

```ts
// Shared Help/About/KIND-legend content for the Help overlay (desktop
// `web/ui.ts`) and the touch edit UI. Row descriptions are sourced from the
// `help.row.*`/`help.section.*` i18n keys shared with the TUI's
// `keys::help_text` (crates/confy-tui/src/tui/keys.rs) so the same binding
// reads identically on both surfaces; `web.help.*` keys are Web-only
// (Pointer section, VS Code variant rows/note). See
// docs/reference/KEYMAP.md §Help overlay parity.
import { escapeHtml } from "./escape.js";
import { t, tArgs, getLang } from "./i18n.js";

interface HelpRow {
  keys: string;
  descKey: string;
}
interface HelpSection {
  titleKey: string;
  rows: HelpRow[];
}

const NAV_SECTION: HelpSection = {
  titleKey: "help.section.nav",
  rows: [
    { keys: "j/k/↑/↓", descKey: "help.row.move_cursor" },
    { keys: "Home/End/g/G", descKey: "help.row.first_last_row" },
    { keys: "PgUp/PgDn", descKey: "help.row.page" },
    { keys: "1/2", descKey: "help.row.expand_collapse_level" },
    { keys: "0/9", descKey: "help.row.collapse_expand_all" },
    { keys: "Space", descKey: "help.row.space_toggle" },
    { keys: "Enter/i", descKey: "help.row.detail" },
  ],
};

const SELECT_SECTION: HelpSection = {
  titleKey: "help.section.select",
  rows: [
    { keys: "s", descKey: "help.row.toggle_select" },
    { keys: "Shift+↑/↓", descKey: "help.row.range_select" },
    { keys: "/", descKey: "help.row.fuzzy_filter" },
    { keys: "f", descKey: "help.row.type_filter" },
    { keys: "Esc", descKey: "help.row.clear_esc" },
  ],
};

function editSection(docFormat: string, variant: "web" | "vscode"): HelpSection {
  const fmt = docFormat.toLowerCase();
  return {
    titleKey: "help.section.edit",
    rows: [
      { keys: "e", descKey: "help.row.edit" },
      { keys: "E", descKey: "help.row.force_editor" },
      { keys: "F2", descKey: "help.row.rename" },
      { keys: "a", descKey: "help.row.add_node" },
      { keys: "d/Delete", descKey: "help.row.delete" },
      { keys: "c/x/v", descKey: "help.row.copy_cut_paste" },
      { keys: "←/→/+/-", descKey: "help.row.nudge" },
      { keys: "r", descKey: `help.row.remark.${fmt}` },
      { keys: "K", descKey: `help.row.kind_switch.${fmt}` },
      {
        keys: "z/y",
        descKey: variant === "vscode" ? "web.help.row.undo_redo_vscode" : "help.row.undo_redo",
      },
      { keys: "C", descKey: "help.row.convert" },
    ],
  };
}

function fileSection(variant: "web" | "vscode"): HelpSection {
  const rows: HelpRow[] = [
    { keys: "Ctrl+s", descKey: variant === "vscode" ? "web.help.row.save_vscode" : "help.row.save" },
  ];
  if (variant === "web") rows.push({ keys: "Ctrl+o", descKey: "help.row.open" });
  if (variant === "vscode") rows.push({ keys: "⇧⌘S / Ctrl+⇧S", descKey: "web.help.row.save_as_convert" });
  rows.push({ keys: "m", descKey: "help.row.action_menu" });
  rows.push({ keys: "?", descKey: "help.row.help" });
  if (variant === "web") rows.push({ keys: "q", descKey: "help.row.quit" });
  return { titleKey: "help.section.file", rows };
}

const POINTER_SECTION: HelpSection = {
  titleKey: "web.help.section.pointer",
  rows: [
    { keys: "click", descKey: "web.help.row.pointer_select" },
    { keys: "Shift+click", descKey: "web.help.row.pointer_range" },
    { keys: "⌘click", descKey: "web.help.row.pointer_multi" },
    { keys: "drag", descKey: "web.help.row.pointer_drag" },
    { keys: "right-click", descKey: "help.row.action_menu" },
  ],
};

function sectionHTML(s: HelpSection): string {
  const title = `<div class="help-sect-title">${escapeHtml(t(s.titleKey))}</div>`;
  const rows = s.rows
    .map(
      (r) =>
        `<div class="help-key">${escapeHtml(r.keys)}</div>` +
        `<div class="help-desc">${escapeHtml(t(r.descKey))}</div>`,
    )
    .join("");
  return title + rows;
}

function sectionsHTML(sections: HelpSection[]): string {
  return `<div class="help-grid">${sections.map(sectionHTML).join("")}</div>`;
}

// One Help line → HTML: the Kind-legend glossary alternates key/description
// (some lines carry two pairs), so wrap every even content segment in a
// .help-key span. Splitting on runs of 2+ spaces with a capture keeps the
// separators, so the alignment survives untouched. Lines without a 2+-space
// split (headings, prose) stay plain; `──` rules get their own .help-sect
// span. Kept distinct from the row-based keymap grid above because the Web
// Kind legend uses its own `label·notation` vocabulary, not the TUI's
// bracket-tag notation (see docs/reference/KEYMAP.md §Help overlay parity).
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

function legendHTML(docFormat: string): string {
  const legend = t(`web.help.legend.${docFormat.toLowerCase()}`);
  return `<div class="help-legend">${legend.split("\n").map(helpLineHTML).join("\n")}</div>`;
}

function vscodeNoteHTML(): string {
  return `<div class="help-note">${escapeHtml(t("web.help.note.vscode"))}</div>`;
}

// Shared Help/About body composition, used by both the desktop overlay
// (`web/ui.ts`) and the touch sheet (`web/touch/app.ts`). Returns HTML ready
// to drop inside the `.help-body` container — the caller must NOT escape it
// again.
//
// `aboutText` is the core-catalog body (`ConfySession.about_text()`, mirrors
// `crates/confy-core/src/session/state.rs::about_text`). One host-owned line
// is appended, mirroring the TUI's `tui.about.language` disclosure: the
// active language code.
export function helpBodyHTML(
  tab: "Help" | "About",
  docFormat: string,
  aboutText: string,
  variant: "web" | "vscode" = "web",
): string {
  if (tab === "About") {
    const body =
      aboutText.replace(/\n+$/, "") + "\n\n" + tArgs("web.about.language", [getLang()]);
    const escaped = escapeHtml(body).replace(
      /(https:\/\/\S+)/,
      '<a href="$1" target="_blank" rel="noopener noreferrer">$1</a>',
    );
    return `<div class="help-about">${escaped}</div>`;
  }
  let out = sectionsHTML([NAV_SECTION, SELECT_SECTION, editSection(docFormat, variant), fileSection(variant)]);
  if (variant === "vscode") out += vscodeNoteHTML();
  out += sectionsHTML([POINTER_SECTION]);
  out += legendHTML(docFormat);
  return out;
}
```

`helpBodyHTML`'s exported signature is unchanged, so its two call sites need only the container-tag edit in step 5, not a signature change.

### 5. Web: swap the Help container from `<pre>` to `<div class="help-body">`

- `web/ui.ts` line 660: change `` `<pre>${body}</pre>` `` to `` `<div class="help-body">${body}</div>` ``.
- `web/touch/app.ts` line 1082: change `` `<pre class="help-body">${body}</pre>` `` to `` `<div class="help-body">${body}</div>` ``.

No other call sites reference these two lines' markup (confirmed by grep — no `.spec.mjs` asserts on `HELP_TEXT`/`helpBodyHTML`/`help-body`/`help-key`/`help-sect` output).

### 6. Web CSS: grid layout + section/row/legend/note styling, replacing the old `#overlay pre` rule

Edit `web/style.css`. The existing block (current lines 475–480):
```css
#overlay h3 { margin: 0 0 10px; font-size: 15px; }
#overlay pre { font-family: var(--mono); font-size: 12.5px; white-space: pre-wrap; margin: 0; }
/* Help body: key/shortcut column stands out from its description. */
#overlay pre .help-key { color: var(--accent); font-weight: 600; }
#overlay pre .help-sect { opacity: 0.6; }
#overlay pre a { color: var(--accent); }
```
becomes (the generic `#overlay pre` rule only ever styled Help's `<pre>`, confirmed by grep — Prompt/KindSwitch render their own markup, not `<pre>`; safe to replace outright):
```css
#overlay h3 { margin: 0 0 10px; font-size: 15px; }
/* Help/About body: two-column keymap grid + legend/about prose. Padding
   between the tab bar and the first row fixes the missing top gap. */
#overlay .help-body { padding-top: 4px; }
#overlay .help-grid {
  display: grid; grid-template-columns: max-content 1fr;
  column-gap: 14px; row-gap: 5px; align-items: baseline;
  font-size: 12.5px; margin-bottom: 14px;
}
#overlay .help-sect-title {
  grid-column: 1 / -1; margin-top: 10px;
  font-size: 11px; font-weight: 700; text-transform: uppercase; letter-spacing: .04em;
  color: var(--muted); opacity: .8;
}
#overlay .help-sect-title:first-child { margin-top: 0; }
#overlay .help-grid .help-key { font-family: var(--mono); color: var(--accent); font-weight: 600; white-space: nowrap; }
#overlay .help-grid .help-desc { font-size: 12.5px; }
#overlay .help-legend { font-family: var(--mono); font-size: 12.5px; white-space: pre-wrap; margin: 0 0 10px; }
#overlay .help-legend .help-key { color: var(--accent); font-weight: 600; }
#overlay .help-legend .help-sect { opacity: 0.6; }
#overlay .help-note { font-size: 12.5px; color: var(--muted); margin-bottom: 10px; }
#overlay .help-about { font-family: var(--mono); font-size: 12.5px; white-space: pre-wrap; }
#overlay .help-about a { color: var(--accent); }
```

Edit `web/touch/style.css`. The existing block (current lines 648–660):
```css
.help-tabs { display: flex; gap: 8px; margin-bottom: 10px; }
/* Help/About body: wrap long lines instead of scrolling sideways — the sheet
   already scrolls on the y axis (.sheet-body overflow-y:auto); a bare <pre>'s
   default white-space:pre forces overflow-x too (any axis other than
   `visible` makes the other axis compute to `auto`), so this is what actually
   keeps overflow to y only. overflow-wrap covers tokens with no break point
   (e.g. a long URL) that pre-wrap alone wouldn't break. */
.help-body {
  white-space: pre-wrap; overflow-wrap: anywhere; word-break: break-word;
}
/* Help body: key/shortcut column stands out from its description. */
.help-body .help-key { color: var(--accent); font-weight: 600; }
.help-body .help-sect { opacity: 0.6; }
```
becomes:
```css
.help-tabs { display: flex; gap: 8px; margin-bottom: 10px; }
/* Help/About body: two-column keymap grid + legend/about prose, wrapped
   instead of scrolling sideways (touch sheet only scrolls on the y axis). */
.help-body { padding-top: 4px; }
.help-grid {
  display: grid; grid-template-columns: max-content 1fr;
  column-gap: 12px; row-gap: 5px; align-items: baseline;
  font-size: 13px; margin-bottom: 14px;
}
.help-sect-title {
  grid-column: 1 / -1; margin-top: 10px;
  font-size: 11px; font-weight: 700; text-transform: uppercase; letter-spacing: .04em;
  color: var(--muted); opacity: .8;
}
.help-sect-title:first-child { margin-top: 0; }
.help-grid .help-key { font-family: var(--mono); color: var(--accent); font-weight: 600; white-space: nowrap; }
.help-legend, .help-about {
  font-family: var(--mono); font-size: 13px;
  white-space: pre-wrap; overflow-wrap: anywhere; word-break: break-word;
}
.help-legend { margin-bottom: 10px; }
.help-legend .help-key { color: var(--accent); font-weight: 600; }
.help-legend .help-sect { opacity: 0.6; }
.help-note { font-size: 13px; color: var(--muted); margin-bottom: 10px; }
```

Do not hand-edit `web/dist/style.css` or `web/ui.js` — both are build output regenerated by `npm run build` (step covered in Verification).

### 7. Expand `docs/reference/KEYMAP.md` with Help-overlay and editor parity

Insert two new `##` sections into `docs/reference/KEYMAP.md` immediately before the existing `## Related documents` section (currently the last section, starting at line 159), leaving every earlier section (including `## Implementation differences`, lines 128–157) unmodified:

```markdown
## Help overlay parity

The `?` overlay's keymap content (not the raw bindings above, which the machine-checked
table already covers) is unified across TUI and Web as a shared Section/Row model, so the
same binding reads identically on both surfaces:

- **Shared rows.** `help.section.*` (4 group headers: Navigation, Selection, Edit, File &
  App) and `help.row.*` (one key per described action) live in `i18n/{en,zh-TW}.json` and
  are read by both `crates/confy-tui/src/tui/keys.rs::help_sections` and
  `web/help-content.ts`'s `NAV_SECTION`/`SELECT_SECTION`/`editSection`/`fileSection`. `r`
  (Remark) and `K` (Kind switch) are the only rows whose *description* is per-`DocFormat`
  (`help.row.remark.<toml|json|yaml>`, `help.row.kind_switch.<toml|json|yaml>`) — both
  surfaces already receive the open document's format and pick the same key.
- **Per-surface rows.** A row's *keys* column may still differ where the binding itself
  differs (see the main table above): e.g. `first_last_row` is `Home/End` on the TUI and
  `Home/End/g/G` on the Web; `nudge` is `←/→` on the TUI and `←/→/+/-` on the Web; `save` is
  `Ctrl+s/w` on the TUI (vim alias) and `Ctrl+s` on the Web. Rows with no Web affordance
  (`tui.help.row.filter_lock`, `tui.help.row.convert_jsonc_toggle`, `l` Language picker, `~`
  Diagnostics) or no TUI affordance (`Ctrl+o` Open, the Pointer section) are prefixed
  `tui.help.*`/`web.help.*` instead of the shared `help.*` and only appear in that surface's
  render.
- **VS Code variant.** `web/help-content.ts`'s `variant: "web" | "vscode"` parameter drops
  the `Ctrl+o`/`q` rows (no in-app Open/Quit under VS Code), swaps `save`/`undo_redo` for
  `web.help.row.save_vscode`/`web.help.row.undo_redo_vscode` (both note the shared VS Code
  stack), adds a `⇧⌘S / Ctrl+⇧S` → `web.help.row.save_as_convert` row, and appends the
  `web.help.note.vscode` prose block — mirroring the same variant split the TUI has no
  equivalent of (VS Code hosts confy inside its own editor tab).
- **Kind legend is intentionally not unified.** The TUI's Kind badge uses bracket tags
  (`[T/S]`, `[S:mstr]`, …, `tui.help.legend.<format>.<keysign|containers|scalars>.<n>`,
  encoded `"<tag>|<description>"`); the Web's Kind badge uses a `label·notation` scheme
  (`web.help.legend.<format>`, one pre-formatted string per format). These are two different
  visual notations for two different UI widgets (TUI's monospace bracket tag vs. Web's
  colored badge pill) — reformatted to one tag/pair per row on each surface for readability,
  but not merged, because the tag vocabularies themselves are not the same.
- **Layout.** TUI renders the whole overlay as one `ratatui::widgets::Paragraph` inside a
  padded `Block` (`Padding::new(2, 2, 1, 1)`), with column alignment computed at render time
  via `unicode_width::UnicodeWidthStr` (display-width, not `char` count, so a zh-TW row never
  desyncs the column under CJK's double-width glyphs — the same technique already used by
  `overlay_lang_picker.rs` and `ui.rs::draw_title`). Web renders the keymap rows as a CSS
  grid (`.help-grid { grid-template-columns: max-content 1fr }`) and the legend/About prose
  as `white-space: pre-wrap` blocks, inside a `<div class="help-body">` (desktop `#overlay`
  and the touch bottom sheet both use the same class).

## Editor (inline/external) parity

Expands the "Inline edit." / "Popup / external editor." bullets under
"Implementation differences" above with the exact per-surface presentation, referenced here
because the Help overlay's `e`/`E`/`Enter`/`i` rows describe this behavior:

| Concern | TUI | Desktop Web | Touch |
| --- | --- | --- | --- |
| Inline edit (scalar leaf) | Core edit buffer driven keystroke-by-keystroke (`EditChar`/`EditCursor*`/`EditDelete`) | Real `<input class="cell-input">`; browser owns cursor/selection/delete; single `CommitEdit` on Enter/blur | Same as desktop web |
| Force editor (`E`, any node) / editor for multiline string or comment (`e`) | Suspends the alternate screen, spawns `$EDITOR` on a scratch file | Opens `#ext-modal` | Opens the `.ext-sheet` bottom sheet |
| Handshake | All three surfaces drive the same `snap.external_edit` async request/response — one core intent, three presentations | | |
| Clipboard-armed guard | Core's `begin_external_edit` refuses while clipboard is armed; TUI additionally raises `core.clipboard.action-locked` before dispatching | Relies on core alone | Same TUI-style extra notice as the TUI (`openExternalEdit`) |
| Detail panel (`Enter`/`i`) | `Mode::Detail` full-screen popup | `#overlay`/aside detail pane | Bottom sheet |

The Help overlay's `help.row.edit` ("Edit (inline or editor)") and `help.row.force_editor`
("Force editor (any node)") rows are intentionally surface-agnostic wording for exactly this
reason: "editor" means `$EDITOR` on the TUI and an in-app modal/sheet on Web, and the row
text does not claim otherwise.

```

Do not otherwise alter `## Implementation differences` (still authoritative for the trigger
logic / `edit_target_kind()` details) — the new `## Editor (inline/external) parity` section
adds the presentation/table summary, it does not replace it.

## Critical files & anchors

- `crates/confy-tui/src/tui/keys.rs:106-118` — current `help_text`, replaced whole per step 2; `unicode_width::UnicodeWidthStr` import pattern to copy is in `overlay_lang_picker.rs:21-22`.
- `crates/confy-tui/src/tui/overlay_help.rs:35-42` — `Block`/height calc needing `Padding` + the `+2`→`+4`/`sub(2)`→`sub(6)` adjustment.
- `crates/confy-tui/src/tui/mod.rs:265-272` — the mirrored geometry calc for scroll clamping, must move in lockstep with overlay_help.rs's.
- `web/help-content.ts` — full-file replacement per step 4.
- `web/style.css:475-480` and `web/touch/style.css:648-660` — exact blocks replaced per step 6.

## Verification

1. `cargo test -p confy-tui keys::` (from repo root) — the 3 pre-existing `help_text` tests (`json_help_differs_from_toml`, `yaml_help_differs_from_toml`, `help_text_is_translated_for_zh_tw`) and the 4 `keymap_doc_*` tests must all still pass unmodified; if any of the 3 `help_text` assertions fail, the substring it checks for must be re-verified against the new row/legend content (the row/legend text above was designed to keep every assertion true — treat a failure as a transcription slip in step 1/2, not a reason to weaken the assertion).
2. `cargo build -p confy-tui` — confirms `Padding` import (from the existing `ratatui::widgets::*` glob) and the new `keys.rs` helpers compile.
3. Manual TUI check: run the TUI binary (`cargo run -p confy-tui -- <some.toml>`), press `?` — confirm: a visible blank row between the border/title and the first `── Navigation ──` line (padding fix), the key column vertically aligned down through all 4 keymap sections and the legend, `Tab` flips to About and back, then switch language (`l`) to zh-TW and reopen `?` — confirm the key column is still aligned (this is the unicode-width regression check) and `多行對話框`/`強制對話框` no longer appear anywhere (replaced by `編輯器`/`強制開啟編輯器`).
4. `cd web && npm run typecheck` — confirms `help-content.ts`'s rewrite type-checks against `web/ui.ts`/`web/touch/app.ts`'s unchanged `helpBodyHTML(...)` call signatures.
5. `cd web && npm test` — the existing `.spec.mjs` suite (including `keymap-parity.spec.mjs` and `toast-migration.spec.mjs`) must still pass; neither touches Help-overlay content and both should be unaffected.
6. `cd web && npm run build` — regenerates `web/dist/*` and `web/ui.js`/`.js.map`; confirms the new `help-content.ts` bundles cleanly.
7. Manual Web check: `cd web && npm run serve` (or the existing dev workflow), open the app, open Help (`?`/info button) — confirm the keymap renders as an aligned two-column grid grouped into Navigation/Selection/Edit/File & App/Pointer, the Kind legend below it, switch to zh-TW and confirm the grid stays aligned (CSS grid, not manual padding, so this should hold trivially) and `多行對話框` no longer appears; then verify the VS Code variant note/rows only by reading `helpBodyHTML(..., "vscode")`'s output in a quick `node -e` smoke check (a full VS Code host isn't required for this) — confirm it omits `Ctrl+o`/`q` and includes the `⇧⌘S / Ctrl+⇧S` row and the VS Code note.

## Assumptions & contingencies

- Web's two-column rendering uses CSS Grid (`display:grid; grid-template-columns:max-content 1fr`) rather than pre-formatted monospace text — this was the user's explicit choice between the two options presented earlier in this session.
- No new automated TUI/Web drift test is added for Help-overlay content — explicit user decision (item 1 in the approval message). If the 3 existing `keys.rs` help-text tests above turn out to need adjustment because a transcribed row/legend string doesn't exactly match what step 1/2 produced, adjust the *test's expected substring* only if the substring itself was checking incidental old wording (e.g. it happened to also match new wording) — do not delete or weaken a test to make it pass around a real content bug.
- The Kind-legend glossary is intentionally kept as two separate, un-unified vocabularies (TUI bracket tags vs. Web `label·notation`) per the scoping decision in step 7 — if this turns out to be wrong (i.e., the user actually wants the legend content itself unified, not just each side's layout polished), that is a larger follow-up requiring new translation/notation-mapping decisions, not a same-plan fix.
</content>
