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

console.log(failures === 0 ? "\nALL FUNCTIONAL CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
