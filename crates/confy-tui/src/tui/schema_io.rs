//! Host-side schema-source resolution for the TUI: a local hint/override
//! resolves against the open file's directory (spec §1); a URL hint fetches
//! over HTTP with a blocking client (confy-tui has no other networking —
//! this is the one new capability the schema feature adds to this crate).

use confy_core::schema::SchemaSource;
use std::path::Path;

pub fn resolve_schema_source(
    source: &SchemaSource,
    open_file_dir: &Path,
) -> Result<String, String> {
    match source {
        SchemaSource::Local(rel) => {
            let path = open_file_dir.join(rel);
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
        }
        SchemaSource::Url(url) => ureq::get(url)
            .call()
            .map_err(|e| format!("{url}: {e}"))?
            .into_string()
            .map_err(|e| format!("{url}: {e}")),
    }
}
