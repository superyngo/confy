use confy_core::schema::SchemaSource;
use confy_tui::tui::schema_io::resolve_schema_source;
use std::fs;

#[test]
fn resolves_a_local_relative_path_against_the_open_files_directory() {
    let dir = std::env::temp_dir().join("confy_schema_io_test");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("s.json"), r#"{"type":"object"}"#).unwrap();
    let source = SchemaSource::Local("./s.json".into());
    let result = resolve_schema_source(&source, &dir);
    assert_eq!(result, Ok(r#"{"type":"object"}"#.to_string()));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_missing_local_file_is_a_soft_error_not_a_panic() {
    let dir = std::env::temp_dir().join("confy_schema_io_test_missing");
    let source = SchemaSource::Local("./nope.json".into());
    let result = resolve_schema_source(&source, &dir);
    assert!(result.is_err());
}

// ---- SchemaSource::Url (Task 11, 2026-08-11 audit remediation) ----
//
// No mock-HTTP-server crate exists anywhere in the workspace (checked via
// `cargo tree --dev`) — a hand-rolled `TcpListener` + raw HTTP/1.1 response
// writer matches the project's low-dependency ethos better than adding one
// for a single test file.

use std::io::{Read, Write};
use std::net::TcpListener;

/// Bind an ephemeral local port, accept exactly one connection on a
/// background thread, discard the request, and write `response` verbatim
/// (caller supplies full status line + headers + body). Returns the URL to
/// fetch. The thread is detached — it exits the moment the one connection is
/// served, so nothing lingers past the test.
fn spawn_one_shot_http_server(response: String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            // Drain (and discard) the request so the client's write doesn't
            // block on a full TCP buffer before we respond.
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    format!("http://127.0.0.1:{port}/schema.json")
}

fn http_ok(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

#[test]
fn url_schema_source_fetches_over_http() {
    let body = r#"{"type":"object","properties":{"port":{"type":"integer"}}}"#;
    let url = spawn_one_shot_http_server(http_ok(body));
    let source = SchemaSource::Url(url);
    let result = resolve_schema_source(&source, std::env::temp_dir().as_path());
    assert_eq!(result, Ok(body.to_string()));
}

#[test]
fn url_schema_source_non_200_is_a_soft_error() {
    let response =
        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string();
    let url = spawn_one_shot_http_server(response);
    let source = SchemaSource::Url(url);
    let result = resolve_schema_source(&source, std::env::temp_dir().as_path());
    assert!(
        result.is_err(),
        "a 404 must resolve as a soft error, not Ok"
    );
}

#[test]
fn url_schema_source_connection_refused_is_a_soft_error_not_a_panic() {
    // Bind to grab a real free port, then drop the listener immediately —
    // nothing is listening there anymore, so the fetch hits connection-refused.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let source = SchemaSource::Url(format!("http://127.0.0.1:{port}/schema.json"));
    let result = resolve_schema_source(&source, std::env::temp_dir().as_path());
    assert!(
        result.is_err(),
        "connection-refused must resolve as a soft error, not Ok"
    );
}

#[test]
fn url_schema_source_malformed_json_body_does_not_panic() {
    // resolve_schema_source is a pure fetch — it hands back whatever text the
    // server sent without parsing it as JSON (schema parsing happens later,
    // in confy-core). A malformed body must still resolve to Ok(raw text),
    // not panic, matching the fetch layer's actual contract.
    let body = "not json at all {{{";
    let url = spawn_one_shot_http_server(http_ok(body));
    let source = SchemaSource::Url(url);
    let result = resolve_schema_source(&source, std::env::temp_dir().as_path());
    assert_eq!(result, Ok(body.to_string()));
}
