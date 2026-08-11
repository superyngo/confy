//! Per-format schema-hint detection — pure, no I/O. Three ecosystem
//! conventions, one per format (spec §1):
//! - JSON/JSONC: a root-level `"$schema"` string member.
//! - YAML: a leading `# yaml-language-server: $schema=<path-or-url>` modeline.
//! - TOML: a first-line `#:schema <path-or-url>` comment (Taplo convention).

use super::types::SchemaSource;
use crate::model::document::DocFormat;

pub fn detect_hint(text: &str, format: DocFormat) -> Option<SchemaSource> {
    match format {
        DocFormat::Json => detect_json(text),
        DocFormat::Yaml => detect_yaml(text),
        DocFormat::Toml => detect_toml(text),
    }
}

fn to_source(raw: &str) -> Option<SchemaSource> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.starts_with("http://") || raw.starts_with("https://") {
        Some(SchemaSource::Url(raw.to_string()))
    } else {
        Some(SchemaSource::Local(raw.to_string()))
    }
}

fn detect_json(text: &str) -> Option<SchemaSource> {
    // Parse-then-lookup rather than regex: `$schema` is a root member of a
    // JSON *value*, and a naive text scan would false-positive on a nested
    // `"$schema"` string value elsewhere in the document. JSONC `//`/`/* */`
    // comments would break `serde_json::from_str`, but a root-level
    // `"$schema"` key is legal even in strict JSON, so this degrades to
    // `None` (not a panic/error) on a JSONC file with comments before the
    // key — acceptable: JSONC's `//`/`/* */` upgrade is orthogonal to schema
    // detection, and a load failure here is never fatal (spec §1: "never a
    // hard-fail").
    let parsed: serde_json::Value = serde_json::from_str(text).ok()?;
    let schema = parsed.get("$schema")?.as_str()?;
    to_source(schema)
}

fn detect_yaml(text: &str) -> Option<SchemaSource> {
    // "Leading" = the modeline must appear before any non-comment,
    // non-blank line (a real document line breaks the leading-comment run).
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('#') {
            if let Some(eq) = rest.trim_start().strip_prefix("yaml-language-server:") {
                if let Some(schema) = eq.trim_start().strip_prefix("$schema=") {
                    return to_source(schema.trim());
                }
            }
            continue; // some other leading comment — keep scanning
        }
        return None; // first non-comment, non-blank line — stop
    }
    None
}

fn detect_toml(text: &str) -> Option<SchemaSource> {
    let first_line = text.lines().next()?;
    let rest = first_line.strip_prefix("#:schema")?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    to_source(rest)
}
