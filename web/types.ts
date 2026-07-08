// Hand-written TypeScript view of the confy-core serde contract (PORTING §8).
// These types mirror the Rust `Serialize`/`Deserialize` representations exactly;
// `serde-wasm-bindgen` is the wire format. The `serde_roundtrip` + `dispatch`
// native tests pin the Rust side; this file is the canonical JS side.
//
// Enum representation is serde's default externally-tagged form:
//   unit variant    -> "CursorDown"          (a bare string)
//   tuple variant   -> { "Nudge": 5 }
//   struct variant  -> { "ApplyReplace": { path, text } }

// ---- Path / segments ----
export type Seg = { Key: string } | { Index: number };
export type Path = Seg[];

// ---- Leaf model types (mirror model::node / model::document) ----
// Mirrors confy-core `model::node::ScalarType` (serde external-tag = variant name).
export type ScalarType =
  | "String"
  | "Integer"
  | "Float"
  | "Bool"
  | "Null"
  | "OffsetDatetime"
  | "LocalDatetime"
  | "LocalDate"
  | "LocalTime";

// The full Format enum (TOML+JSON+YAML). Unknown strings are tolerated.
export type Format = string;

export type DocFormat = "Toml" | "Json" | "Yaml";

// ---- View row (session::view::ViewRow) ----
export interface ViewRow {
  path: Path;
  depth: number;
  is_branch: boolean;
  key: string;
  // serde `Option`s arrive as `undefined` (serde-wasm-bindgen), never `null`.
  value: string | undefined;
  scalar_type: ScalarType | undefined;
  format: Format;
  // Node-kind label ("table"/"array"/"inline"/"array-of-tables"/"string"/…)
  // for the per-row kind badge.
  type_label: string;
  // Immediate child count (drives the branch item-count badge; 0 for scalars).
  child_count: number;
  trailing_comment: string | undefined;
  // Key-sign label ("bare"/"quoted"/"dotted"/"none") for the structured "Sign" field.
  key_sign?: string;
  read_only: boolean;
  selected: boolean;
  is_cursor: boolean;
}

// ---- Mode projection (session::view::ModeView) ----
export type PromptView =
  | "ConfirmQuit"
  | "Collision"
  | "TypeChange"
  | "ArrayUpgrade"
  | "JsoncUpgrade";

export type EditField = "Value" | "Name";
export type ConvertStep = "Format" | "Path" | "Confirm";

export interface KindOptionView {
  label: string;
  target: string; // KindTarget enum tag (opaque to the UI; sent back verbatim)
}

export interface EditView {
  field: EditField;
  buffer: string;
  cursor: number;
  key: string;
  is_element: boolean;
  is_comment: boolean;
  rename_only: boolean;
}

export interface ConvertView {
  step: ConvertStep;
  cursor: number;
  options: DocFormat[];
  target: DocFormat;
  path: string;
  path_cursor: number;
  warnings: string[];
}

// ---- Type-filter facet grid (session::view::TypeFilterView) ----
export type CheckState = "On" | "Partial" | "Off";

export interface TypeFilterCellView {
  label: string;
  state: CheckState;
  is_cursor: boolean;
}

export type TypeFilterRow =
  | { Header: string }
  | { Cells: TypeFilterCellView[] };

export interface TypeFilterView {
  rows: TypeFilterRow[];
  cursor_row: number;
  cursor_col: number;
  active: boolean;
}

export type ModeView =
  | "Normal"
  | { Prompt: { kind: PromptView } }
  | { Filter: { text: string; cursor: number } }
  | "FilterResults"
  | { TypeFilter: TypeFilterView }
  | { KindSwitch: { cursor: number; options: KindOptionView[] } }
  | { Convert: ConvertView }
  | "Detail"
  | "Help"
  | { Edit: EditView };

// ---- External edit handshake (session::view::ExternalEdit, §8.2) ----
export type ExternalEditKind =
  | { Value: { path: Path } }
  | { Comment: { path: Path } };

export interface ExternalEdit {
  initial: string;
  kind: ExternalEditKind;
}

// ---- Full-state snapshot (session::view::SessionSnapshot) ----
export interface SessionSnapshot {
  doc_format: DocFormat;
  is_dirty: boolean;
  mode: ModeView;
  rows: ViewRow[];
  cursor: Path;
  // serde `Option`s arrive as `undefined` (serde-wasm-bindgen), never `null`.
  status: string | undefined;
  error: string | undefined;
  detail_text: string | undefined;
  external_edit: ExternalEdit | undefined;
  convert_write: [string, string] | undefined; // [output_path, text]
  clipboard_count: number | undefined; // Some(n) when the clipboard holds n fragments
  clipboard_cut: boolean; // true = cut (move); false = copy
  clipboard_paths: Path[]; // source node paths captured in the clipboard
  quit: boolean;
}

// ---- Intent (session::intent::Intent) ----
// Helpers below build the externally-tagged objects so UI code never hand-spells
// a variant name and stays in sync with Rust.
export type Intent =
  // Navigation
  | "CursorDown" | "CursorUp" | "CursorHome" | "CursorEnd"
  | { PageUp: number } | { PageDown: number }
  | "ToggleExpand" | "CollapseAll" | "ExpandAll" | "ExpandLevel" | "CollapseLevel"
  // Pointer (Web UI)
  | { SetCursor: Path }
  | { CommitEdit: { value: string | null; name: string | null } }
  | { CommitKind: { path: Path; target: string } }
  | { SetSelection: { paths: Path[] } }
  | { SetTrailing: { path: Path; comment: string | null } }
  | { MoveSelectionTo: { sources: Path[]; target: Path; index: number } }
  // Selection
  | "ToggleSelect" | "ExtendSelectUp" | "ExtendSelectDown"
  // Filter
  | "EnterFilter" | "CommitFilter" | "ExitFilter" | "ExitFilterResults"
  | { SetFilter: string }
  | { FilterChar: string }
  | "FilterBackspace" | "FilterDelete"
  | "FilterCursorLeft" | "FilterCursorRight" | "FilterCursorHome" | "FilterCursorEnd"
  // Type filter
  | "EnterTypeFilter" | "CommitTypeFilter" | "ExitTypeFilter"
  | { TypeFilterMove: [number, number] }
  | "TypeFilterToggle"
  // Kind switch
  | "OpenKindSwitch" | { KindSwitchMove: number } | "KindSwitchCommit" | "ExitKindSwitch"
  // Convert
  | "OpenConvert" | { ConvertMove: number } | "ConvertPickFormat"
  | { SetConvertFormat: DocFormat }
  | { SetConvertPath: string }
  | { ConvertPathChar: string }
  | "ConvertPathBackspace" | "ConvertPathDelete"
  | "ConvertPathLeft" | "ConvertPathRight" | "ConvertPathHome" | "ConvertPathEnd"
  | "ConvertRun" | "ConvertConfirm" | "ExitConvert"
  // Detail
  | "ToggleDetail" | "ExitDetail"
  // Help
  | "EnterHelp" | "ExitHelp"
  // Inline edit
  | "BeginEdit" | "BeginRename" | "EditToggleField"
  | { EditChar: string }
  | "EditBackspace" | "EditDelete"
  | "EditCursorLeft" | "EditCursorRight" | "EditCursorHome" | "EditCursorEnd"
  | "EditCommit" | "EditCancel"
  // External edit resolution (host → core)
  | { ApplyReplace: { path: Path; text: string } }
  | { ApplyEditComment: { path: Path; text: string } }
  // Mutations
  | { Nudge: number }
  | "AddNode" | "AddChild" | "AddSibling" | "DeleteSelected" | "CopySelected" | "CutSelected" | "Paste" | "Remark"
  // Undo / Redo
  | "Undo" | "Redo"
  // Lifecycle
  | "Escape" | { PromptKey: string } | "QuitRequested" | "Save";
