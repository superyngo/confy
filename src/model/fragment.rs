use crate::model::document::MutateError;
use toml_edit::{DocumentMut, Table};

/// Parse a user-edited TOML fragment into a detached table whose entries can be
/// merged into the document. The fragment is parsed as a standalone TOML doc.
pub fn parse_fragment(src: &str) -> Result<Table, MutateError> {
    let doc = src.parse::<DocumentMut>().map_err(|e| MutateError::Fragment(e.to_string()))?;
    Ok(doc.as_table().clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_fragment() {
        let f = parse_fragment("port = 8080\n").unwrap();
        assert_eq!(f.len(), 1);
        assert_eq!(f.iter().next().map(|(k, _)| k).unwrap(), "port");
    }

    #[test]
    fn parses_table_fragment() {
        let f = parse_fragment("[server]\nport = 8080\n").unwrap();
        assert!(f.contains_key("server"));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_fragment("= = nope").is_err());
    }
}
