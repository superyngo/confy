---
status: implemented
---

# JSON Schema validation uses the `jsonschema` crate, not a hand-rolled validator

confy's other backends (TOML/JSON/YAML parsing) are deliberately hand-rolled to get
byte-identical lossless round-tripping — a house style a reader would expect Schema
support to follow too. We didn't: a hand-rolled subset validator (type/enum/const/
required/bounds/pattern, no `$ref`/`allOf`/`oneOf`/`anyOf`/`if-then-else`) would silently
fail on the majority of real-world schemas (SchemaStore, code-generated schemas) that
lean on composition and `$ref`, undermining the feature's actual value. The `jsonschema`
crate gives full draft 2020-12 compliance for one new leaf dependency (confy-core already
depends on `serde_json`, which `jsonschema` builds on) and is confirmed
`wasm32-unknown-unknown`-compatible once its optional `reqwest` remote-fetch feature is
disabled — a non-issue for confy, which stays fs-free and has hosts resolve `$ref`
fetches itself. Round-tripping fidelity (confy's reason to hand-roll elsewhere) doesn't
apply here: a schema is read-only input to a validator, never edited or re-serialized by
confy.

## Considered options

- **Hand-rolled subset validator** — rejected: coverage gap on real schemas (see above)
  defeats the point of adding Schema support at all.
- **`jsonschema` crate** — chosen: full spec compliance, proven, wasm-viable with remote
  fetch disabled.
