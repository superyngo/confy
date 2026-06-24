// Functional-path verification (Stage 2). Drives the wasm `ConfySession` through
// the same command channel the Web UI uses, proving the Intent→SessionSnapshot
// contract + the async external-edit handshake work end-to-end. No browser —
// the user manually tests the rendered UI (per the no-pty-TUI-testing policy);
// this proves the API surface the UI sits on.
import init, { ConfySession } from "./pkg/confy_ffi.js";
import { readFileSync } from "node:fs";
import assert from "node:assert";

const wasm = readFileSync("./pkg/confy_ffi_bg.wasm");
await init(wasm);

function unit(name) {
  return name; // bare string
}
function tuple(name, v) {
  return { [name]: v };
}
// serde-wasm-bindgen emits `undefined` (not `null`) for Option::None.
const isNull = (v) => v == null; // loose: catches null and undefined

let failures = 0;
function check(name, cond, extra = "") {
  if (cond) {
    console.log(`  ✓ ${name}`);
  } else {
    console.log(`  ✗ ${name} ${extra}`);
    failures++;
  }
}

// ---- 1. Load + snapshot ----
const src = `[server]\nhost = "localhost"\nport = 8080\n`;
const s = new ConfySession(src, "toml");
let snap = s.snapshot();
check("loads TOML, format=Toml", snap.doc_format === "Toml");
check("starts in Normal mode", snap.mode === "Normal");
check("root + server branch visible", snap.rows.length === 2, JSON.stringify(snap.rows.map(r => r.key)));

// ---- 2. Navigate into the branch + expand ----
snap = s.dispatch(unit("CursorDown")); // onto [server]
check("cursor on server", snap.cursor[0]?.Key === "server", JSON.stringify(snap.cursor));
snap = s.dispatch(unit("ToggleExpand")); // expand server
check("expanded shows host+port", snap.rows.length === 4, "len=" + snap.rows.length);
check("cursor row flagged", snap.rows.some(r => r.is_cursor && r.key === "server"));
// Kind-badge wire: each row carries a type_label, branches a child_count.
const serverRow = snap.rows.find(r => r.key === "server");
check("branch row carries type_label=table", serverRow?.type_label === "table", serverRow?.type_label);
check("branch row carries child_count", serverRow?.child_count === 2, "n=" + serverRow?.child_count);
check("leaf row carries scalar type_label", snap.rows.find(r => r.key === "port")?.type_label === "integer");

// ---- 3. Navigate to leaf `port` and inline-edit ----
snap = s.dispatch(unit("CursorDown")); // host
snap = s.dispatch(unit("CursorDown")); // port
check("cursor on port", snap.rows.find(r => r.is_cursor)?.key === "port");

// ---- 4. Nudge the integer ----
snap = s.dispatch(tuple("Nudge", 10));
const portVal = snap.rows.find(r => r.key === "port").value;
check("nudge +10 → 8090", portVal === "8090", portVal);
check("doc now dirty", snap.is_dirty === true);

// ---- 5. Inline edit: BeginEdit on a single-line scalar routes inline ----
snap = s.dispatch(unit("BeginEdit"));
check("scalar routes inline (no external_edit)", isNull(snap.external_edit));
check("enters Edit mode", typeof snap.mode === "object" && "Edit" in snap.mode, JSON.stringify(snap.mode));

// type "9999": clear isn't available; just type digits (appends)
snap = s.dispatch(tuple("EditChar", "9"));
snap = s.dispatch(tuple("EditChar", "9"));
const editMode = snap.mode.Edit;
check("edit buffer captures chars", editMode.buffer.includes("9"), editMode.buffer);

// cancel the edit — doc should be unchanged from nudged state
snap = s.dispatch(unit("EditCancel"));
check("edit cancelled → Normal", snap.mode === "Normal");

// ---- 6. Multiline external-edit handshake (the async §8.2 path) ----
// Reload a doc with a multiline string.
const src2 = `notes = """\nhello\n"""\n`;
const s2 = new ConfySession(src2, "toml");
s2.dispatch(unit("CursorDown")); // onto notes
let snap2 = s2.dispatch(unit("BeginEdit"));
check("multiline routes external", snap2.external_edit !== null && snap2.external_edit.kind?.Value !== undefined, JSON.stringify(snap2.external_edit));
const ext = snap2.external_edit;
const extPath = ext.kind.Value.path;
check("external initial contains hello", ext.initial.includes("hello"), ext.initial);

// Host edits asynchronously, returns edited text via ApplyReplace.
const edited = 'notes = """\nWORLD\n"""\n';
snap2 = s2.dispatch(tuple("ApplyReplace", { path: extPath, text: edited }));
check("apply succeeds (no error)", isNull(snap2.error), snap2.error);
check("pending cleared", isNull(snap2.external_edit));
const out = s2.serialize();
check("doc reflects WORLD", out.includes("WORLD"), out);
check("old hello gone", !out.includes("hello"), out);

// ---- 7. Undo/redo ----
snap2 = s2.dispatch(unit("Undo"));
check("undo restores hello", s2.serialize().includes("hello"));

// ---- 8. Save marks clean ----
snap2 = s2.dispatch(unit("Redo"));
snap2 = s2.dispatch(unit("Save"));
check("Save clears dirty", snap2.is_dirty === false);

// ---- 9. Quit flow: dirty → prompt; y → quit ----
const s3 = new ConfySession("a = 1\n", "toml");
s3.dispatch(unit("CursorDown"));
s3.dispatch(tuple("Nudge", 1));
let snap3 = s3.dispatch(unit("QuitRequested"));
check("dirty quit enters prompt (not quit)", snap3.quit === false && typeof snap3.mode === "object" && "Prompt" in snap3.mode);
snap3 = s3.dispatch(tuple("PromptKey", "y"));
check("y confirms quit", snap3.quit === true);

// ---- 10. JSON backend parity ----
const s4 = new ConfySession('{"a": 1, "b": [1,2,3]}', "json");
const snap4 = s4.snapshot();
check("JSON loads, format=Json", snap4.doc_format === "Json");
check("JSON root has children", snap4.rows.length >= 2);

// ---- 11. Type-filter facet grid projects from core ----
const s5 = new ConfySession('a = 1\nb = "x"\n', "toml");
let snap5 = s5.dispatch(unit("EnterTypeFilter"));
const tf = typeof snap5.mode === "object" && "TypeFilter" in snap5.mode ? snap5.mode.TypeFilter : null;
check("TypeFilter mode carries grid", tf !== null && Array.isArray(tf.rows));
if (tf) {
  const hasHeader = tf.rows.some(r => "Header" in r);
  const cells = tf.rows.flatMap(r => ("Cells" in r ? r.Cells : []));
  check("grid has headers + cells", hasHeader && cells.length > 0);
  check("exactly one cursor cell", cells.filter(c => c.is_cursor).length === 1);
  check("grid starts inactive", tf.active === false);
  // Toggle the cursor cell, reopen, expect active + one On cell.
  s5.dispatch(unit("TypeFilterToggle"));
  snap5 = s5.dispatch(unit("EnterTypeFilter"));
  const tf2 = "TypeFilter" in snap5.mode ? snap5.mode.TypeFilter : null;
  check("grid active after toggle", tf2 && tf2.active === true);
  check("a cell went On", tf2 && tf2.rows.flatMap(r => ("Cells" in r ? r.Cells : [])).some(c => c.state === "On"));
}

// ---- 12. clipboard_count reflects copy ----
const s6 = new ConfySession("a = 1\nb = 2\n", "toml");
check("clipboard empty initially", isNull(s6.snapshot().clipboard_count));
s6.dispatch(unit("ToggleSelect"));
const snap6 = s6.dispatch(unit("CopySelected"));
check("clipboard_count set after copy", snap6.clipboard_count === 1, String(snap6.clipboard_count));

// ---- 13. SetCursor: pointer-addressed cursor placement (Web UI) ----
const s7 = new ConfySession("a = 1\nb = 2\nc = 3\n", "toml");
const cPath = s7.snapshot().rows.find(r => r.key === "c").path;
let snap7 = s7.dispatch(tuple("SetCursor", cPath));
check("SetCursor lands on c", snap7.rows.find(r => r.is_cursor)?.key === "c", JSON.stringify(snap7.cursor));
// An off-tree path is ignored (cursor unchanged).
snap7 = s7.dispatch(tuple("SetCursor", [{ Key: "nope" }]));
check("SetCursor ignores off-tree path", snap7.rows.find(r => r.is_cursor)?.key === "c");

// ---- 14. CommitEdit + CommitKind: pointer inline-edit / kind switch (Web UI) ----
const s8 = new ConfySession("port = 8080\nname = \"x\"\n", "toml");
const portPath = s8.snapshot().rows.find(r => r.key === "port").path;
s8.dispatch(tuple("SetCursor", portPath));
s8.dispatch({ CommitEdit: { value: "9090", name: null } });
check("CommitEdit replaces value", s8.serialize().includes("port = 9090"), s8.serialize());
// Rename: value omitted (null) keeps it.
const namePath = s8.snapshot().rows.find(r => r.key === "name").path;
s8.dispatch(tuple("SetCursor", namePath));
s8.dispatch({ CommitEdit: { value: null, name: "label" } });
check("CommitEdit renames key, keeps value", s8.serialize().includes('label = "x"'), s8.serialize());
// Kind switch via kind_options → CommitKind.
s8.dispatch(tuple("SetCursor", s8.snapshot().rows.find(r => r.key === "port").path));
const opts = s8.kind_options(s8.snapshot().rows.find(r => r.key === "port").path);
const hex = opts.find(o => o.target === "IntHex");
check("kind_options offers IntHex for an integer", !!hex, JSON.stringify(opts));
if (hex) {
  s8.dispatch({ CommitKind: { path: s8.snapshot().rows.find(r => r.key === "port").path, target: "IntHex" } });
  check("CommitKind switches integer to hex", /port = 0x/.test(s8.serialize()), s8.serialize());
}

// ---- 15. SetSelection: pointer multi-select (Web UI, Phase 2) ----
const s9 = new ConfySession("a = 1\nb = 2\nc = 3\n", "toml");
const path9 = (k) => s9.snapshot().rows.find(r => r.key === k).path;
let snap9 = s9.dispatch({ SetSelection: { paths: [path9("a"), path9("c")] } });
check("SetSelection selects a + c", snap9.rows.filter(r => r.selected).map(r => r.key).join(",") === "a,c",
  snap9.rows.filter(r => r.selected).map(r => r.key).join(","));
check("SetSelection cursor follows focal (c)", snap9.rows.find(r => r.is_cursor)?.key === "c");
// A fresh SetSelection replaces (not unions); off-tree paths drop.
snap9 = s9.dispatch({ SetSelection: { paths: [path9("b"), [{ Key: "nope" }]] } });
check("SetSelection replaces + drops off-tree", snap9.rows.filter(r => r.selected).map(r => r.key).join(",") === "b",
  snap9.rows.filter(r => r.selected).map(r => r.key).join(","));

// ---- 16. MoveSelectionTo: pointer drag-reparent (Web UI, Phase 2) ----
const s10 = new ConfySession("a = 1\n[t]\nx = 2\n", "toml");
const aPath = s10.snapshot().rows.find(r => r.key === "a").path;
const tPath = s10.snapshot().rows.find(r => r.key === "t").path;
const snap10 = s10.dispatch({ MoveSelectionTo: { sources: [aPath], target: tPath, index: 0 } });
check("MoveSelectionTo succeeds (no error)", isNull(snap10.error), String(snap10.error));
check("MoveSelectionTo reparents a under [t]", s10.serialize().indexOf("a = 1") > s10.serialize().indexOf("[t]"), s10.serialize());
// Drop into own subtree is rejected, document untouched.
const s11 = new ConfySession("[t]\nx = 2\n", "toml");
const before11 = s11.serialize();
const tP = s11.snapshot().rows.find(r => r.key === "t").path;
const snap11 = s11.dispatch({ MoveSelectionTo: { sources: [tP], target: [...tP, { Key: "x" }], index: 0 } });
check("MoveSelectionTo rejects self-subtree drop", !isNull(snap11.error));
check("MoveSelectionTo leaves doc untouched on reject", s11.serialize() === before11);

// ---- 17. SetFilter: pointer live-search (Web UI, Phase 4) ----
const s12 = new ConfySession("alpha = 1\nbeta = 2\n", "toml");
let snap12 = s12.dispatch({ SetFilter: "alph" });
check("SetFilter enters FilterResults", snap12.mode === "FilterResults", JSON.stringify(snap12.mode));
check("SetFilter narrows to alpha", snap12.rows.some(r => r.key === "alpha") && !snap12.rows.some(r => r.key === "beta"),
  JSON.stringify(snap12.rows.map(r => r.key)));
snap12 = s12.dispatch({ SetFilter: "" });
check("SetFilter clears back to Normal + all rows", snap12.mode === "Normal" && snap12.rows.some(r => r.key === "beta"),
  JSON.stringify(snap12.mode));
// Search matches a value, not just keys (bug #1).
const s12b = new ConfySession("host = \"localhost\"\nport = 8080\n", "toml");
const snap12b = s12b.dispatch({ SetFilter: "localhost" });
check("SetFilter matches a value (not just keys)",
  snap12b.rows.some(r => r.key === "host") && !snap12b.rows.some(r => r.key === "port"),
  JSON.stringify(snap12b.rows.map(r => r.key)));

// ---- 18. SetConvertFormat + SetConvertPath → ConvertRun (Web UI, Phase 4) ----
const s13 = new ConfySession("a = 1\n", "toml");
s13.dispatch(tuple("SetCursor", [])); // root node
s13.dispatch(unit("OpenConvert"));
let snap13 = s13.dispatch(tuple("SetConvertFormat", "Json"));
const cv = "Convert" in snap13.mode ? snap13.mode.Convert : null;
check("SetConvertFormat sets target=Json + seeds .json path", cv && cv.target === "Json" && cv.path.endsWith(".json"),
  JSON.stringify(cv));
s13.dispatch(tuple("SetConvertPath", "custom.json"));
snap13 = s13.dispatch(unit("ConvertRun"));
check("ConvertRun writes to the chosen path", snap13.convert_write && snap13.convert_write[0] === "custom.json",
  JSON.stringify(snap13.convert_write));
check("converted JSON text contains \"a\"", snap13.convert_write && snap13.convert_write[1].includes('"a"'),
  snap13.convert_write && snap13.convert_write[1]);

console.log(failures === 0 ? "\nALL FUNCTIONAL CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
