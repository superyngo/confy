// T8 (R14/R15/R29): the Raw breadcrumb jump's byte-offset conversion, kept
// pure and DOM-free so it is directly importable by a spec without going
// through ui.ts's wasm-dependent module graph. The path→node lookup that
// used to live here is gone (2026-09-14): it walked `session.outline()`,
// which omits Comment nodes by design, so a jump to a comment row was a
// silent no-op — core answers it directly now (`Session::span_of`).

// Core's `text_range`s are UTF-8 byte offsets over `serialize()`; DOM
// APIs (`Range`, `<textarea>.setSelectionRange`) index by UTF-16 code unit.
// A document with any non-ASCII byte before the target makes a naive
// `slice(byteOffset)` land mid-character — measured and quantified in the
// design record's F5/T2.3 (CJK+emoji fixture, drift === 8).
export function byteToCodeUnit(text: string, byteOffset: number): number {
  let bytes = 0;
  for (let i = 0; i < text.length; i++) {
    if (bytes >= byteOffset) return i;
    const code = text.charCodeAt(i);
    if (code < 0x80) bytes += 1;
    else if (code < 0x800) bytes += 2;
    else if (code >= 0xd800 && code <= 0xdbff) {
      bytes += 4; // astral code point: one UTF-16 surrogate pair, 4 UTF-8 bytes
      i++; // consume the low surrogate too (the `for` loop's own ++ finishes the pair)
    } else bytes += 3;
  }
  return text.length;
}

