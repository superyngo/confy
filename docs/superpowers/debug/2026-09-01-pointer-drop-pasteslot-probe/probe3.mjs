// POST-FIX verification against the rebuilt real wasm core (the same module the
// Web/touch UI loads): for every row and every band, the grip-drag path
// (`MoveSelectionTo` with the slot `pointer_slot` returned — exactly what
// web/dnd.ts and web/touch/app.ts now send) must land in the SAME place as an
// armed cut+paste released at the identical pixel. Plus: inline/flow containers
// must now be reachable as `Into` targets from a pointer.
import init, { ConfySession } from "../../../../web/pkg/confy_ffi.js";
import { readFileSync } from "node:fs";
await init(readFileSync(new URL("../../../../web/pkg/confy_ffi_bg.wasm", import.meta.url)));
const J = (v) => JSON.stringify(v);
const DOC = "a = 1\n[b]\nc = 2\nd = 3\n[e]\nf = 4\n";
const A = [{ Key: "a" }];
const mk = (t = DOC, f = "toml") => { const s = new ConfySession(t, f); s.dispatch("ExpandAll"); return s; };
const label = (p) => (p.length ? p.map((x) => x.Key ?? `#${x.Index}`).join(".") : "<root>");

let bad = 0;
console.log("row    band  slot                                 drag == paste?");
for (const r of mk().snapshot().rows) {
  if (!r.path.length || J(r.path) === J(A)) continue;
  for (const rel of [0.1, 0.3, 0.5, 0.7, 0.9]) {
    const slot = mk().pointer_slot(r.path, rel);

    const drag = mk();
    const dsnap = drag.dispatch({ MoveSelectionTo: { sources: [A], slot, cut: true } });

    const paste = mk();
    paste.dispatch({ SetSelection: { paths: [A] } });
    paste.dispatch("CutSelected");
    paste.dispatch({ SetPasteSlot: slot });
    const psnap = paste.dispatch("Paste");

    const same = drag.serialize() === paste.serialize();
    const sameErr = J(dsnap.notice?.severity) === J(psnap.notice?.severity);
    if (!same || !sameErr) bad++;
    console.log(
      `${label(r.path).padEnd(6)} ${rel}   ${J(slot).padEnd(36)} ${same && sameErr ? "✓" : "✗ MISMATCH"}` +
        (same ? "" : `\n   drag:  ${drag.serialize().replace(/\n/g, "⏎")}\n   paste: ${paste.serialize().replace(/\n/g, "⏎")}`),
    );
  }
}
console.log(bad === 0 ? "\n✓ drag and paste agree at EVERY band" : `\n✗ ${bad} mismatches`);

console.log("\n---- the exact reported bug: drop into the gap under expanded [b] ----");
{
  const slot = mk().pointer_slot([{ Key: "b" }], 0.9);
  const s = mk();
  const snap = s.dispatch({ MoveSelectionTo: { sources: [A], slot, cut: true } });
  console.log("slot:", J(slot), "| notice:", J(snap.notice));
  console.log(s.serialize());
}

console.log("---- inline / flow containers are now Into-targetable by pointer ----");
{
  const t = mk("t = { x = 1 }\narr = [ 1, 2 ]\nk = 9\n");
  for (const p of [[{ Key: "t" }], [{ Key: "arr" }]]) {
    console.log(`${label(p)}:`, [0.3, 0.5, 0.7].map((r) => J(t.pointer_slot(p, r))).join(" "));
  }
  const y = mk("t: {x: 1}\nseq: [1, 2]\nk: 9\n", "yaml");
  for (const p of [[{ Key: "t" }], [{ Key: "seq" }]]) {
    console.log(`yaml ${label(p)}:`, [0.3, 0.5, 0.7].map((r) => J(y.pointer_slot(p, r))).join(" "));
  }
  // and a real drag onto that inline table's mid-band
  const d = mk("t = { x = 1 }\nk = 9\n");
  const slot = d.pointer_slot([{ Key: "t" }], 0.5);
  const snap = d.dispatch({ MoveSelectionTo: { sources: [[{ Key: "k" }]], slot, cut: true } });
  console.log("drag k onto t mid-band:", J(slot), "| notice:", J(snap.notice));
  console.log(d.serialize());
}

console.log("---- regression: cut:false (copy-drag) still copies ----");
{
  const d = mk();
  const slot = d.pointer_slot([{ Key: "e" }], 0.5);
  d.dispatch({ MoveSelectionTo: { sources: [A], slot, cut: false } });
  console.log(J(slot), "\n" + d.serialize());
}
