//! LSP tests (Item D) — drive `Server::handle` and the JSON parser directly, without a real editor.
//! The server is off the byte-identity spine, so these cover framing/dispatch/mapping; the diagnostic
//! *content* is already covered by the checker tests.

use super::json::Json;
use super::*;

// ── JSON parser ──────────────────────────────────────────────────────────────────────────────

#[test]
fn json_parses_objects_arrays_and_escapes() {
    let v = Json::parse(r#"{"a":1,"b":[true,null,"x\ny"],"c":{"d":"e"}}"#).unwrap();
    assert_eq!(v.get("a"), Some(&Json::Num(1.0)));
    assert_eq!(v.get("b").and_then(Json::as_array).map(<[_]>::len), Some(3));
    assert_eq!(
        v.get("b").and_then(Json::as_array).unwrap()[2].as_str(),
        Some("x\ny")
    );
    assert_eq!(
        v.get("c").and_then(|c| c.get("d")).and_then(Json::as_str),
        Some("e")
    );
}

#[test]
fn json_rejects_garbage_and_trailing_junk() {
    assert!(Json::parse("{").is_none());
    assert!(Json::parse(r#"{"a":1} trailing"#).is_none());
    assert!(Json::parse("nope").is_none());
}

#[test]
fn json_parses_unicode_escape() {
    assert_eq!(Json::parse(r#""Aé""#).unwrap().as_str(), Some("Aé"));
}

// ── lifecycle ──────────────────────────────────────────────────────────────────────────────

#[test]
fn initialize_advertises_capabilities() {
    let mut s = Server::default();
    let msg = Json::parse(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#).unwrap();
    let out = s.handle(&msg);
    assert_eq!(out.len(), 1);
    assert!(out[0].contains("\"id\":1"));
    assert!(out[0].contains("textDocumentSync"));
}

#[test]
fn shutdown_sets_flag_and_responds_null() {
    let mut s = Server::default();
    assert!(!s.shutting_down());
    let msg = Json::parse(r#"{"id":9,"method":"shutdown"}"#).unwrap();
    let out = s.handle(&msg);
    assert!(s.shutting_down());
    assert!(out[0].contains("\"result\":null"));
}

#[test]
fn unknown_request_is_method_not_found_but_unknown_notification_is_ignored() {
    let mut s = Server::default();
    let req = Json::parse(r#"{"id":3,"method":"textDocument/bogus"}"#).unwrap();
    assert!(s.handle(&req)[0].contains("-32601"));
    let notif = Json::parse(r#"{"method":"$/somethingNew"}"#).unwrap();
    assert!(s.handle(&notif).is_empty());
}

// ── diagnostics ──────────────────────────────────────────────────────────────────────────────

fn did_open(uri: &str, text: &str) -> Json {
    // The text is embedded as a JSON string — escape quotes/newlines.
    let escaped = super::escape(text);
    Json::parse(&format!(
        r#"{{"method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{uri}","text":"{escaped}"}}}}}}"#
    ))
    .unwrap()
}

#[test]
fn open_clean_program_publishes_empty_diagnostics() {
    let mut s = Server::default();
    let out = s.handle(&did_open(
        "file:///a.phg",
        "package Main; function main() -> void { }",
    ));
    assert_eq!(out.len(), 1);
    assert!(out[0].contains("publishDiagnostics"));
    assert!(out[0].contains("\"diagnostics\":[]"));
    assert!(out[0].contains("file:///a.phg"));
}

#[test]
fn open_program_with_error_publishes_a_diagnostic_with_code_and_range() {
    let mut s = Server::default();
    let out = s.handle(&did_open(
        "file:///b.phg",
        "package Main; function main() -> void { var x = nope; }",
    ));
    let body = &out[0];
    assert!(body.contains("publishDiagnostics"));
    assert!(body.contains("E-UNKNOWN-IDENT"), "{body}");
    assert!(body.contains("\"severity\":1"));
    assert!(body.contains("\"range\""));
}

#[test]
fn change_then_close_updates_then_clears_document() {
    let mut s = Server::default();
    s.handle(&did_open(
        "file:///c.phg",
        "package Main; function main() -> void { }",
    ));
    // didChange (full sync) introducing an error.
    let change = Json::parse(
        r#"{"method":"textDocument/didChange","params":{"textDocument":{"uri":"file:///c.phg"},"contentChanges":[{"text":"package Main; function main() -> void { var x = nope; }"}]}}"#,
    )
    .unwrap();
    assert!(s.handle(&change)[0].contains("E-UNKNOWN-IDENT"));
    // didClose removes it (no panic; no diagnostics emitted).
    let close = Json::parse(
        r#"{"method":"textDocument/didClose","params":{"textDocument":{"uri":"file:///c.phg"}}}"#,
    )
    .unwrap();
    assert!(s.handle(&close).is_empty());
}

#[test]
fn warning_maps_to_severity_2() {
    // A force-unwrap on an optional fires W-FORCE-UNWRAP (a non-fatal lint) — severity 2.
    let mut s = Server::default();
    let out = s.handle(&did_open(
        "file:///w.phg",
        "package Main; function f(int? o) -> int { return o!; } function main() -> void { }",
    ));
    let body = &out[0];
    assert!(body.contains("W-FORCE-UNWRAP"), "{body}");
    assert!(body.contains("\"severity\":2"));
}
