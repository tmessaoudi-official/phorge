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

// ── hover + go-to-definition ───────────────────────────────────────────────────────────────────

/// A program where `helper` is declared on line 1 (0-based) and called on line 2.
const PROG: &str =
    "package Main;\nfunction helper(int n) -> int { return n; }\nfunction main() -> void { var r = helper(3); }";

/// A `textDocument/<method>` request positioned at (line, character) (0-based) in `file:///x.phg`.
fn req_at(method: &str, line: u32, character: u32) -> Json {
    Json::parse(&format!(
        r#"{{"id":7,"method":"textDocument/{method}","params":{{"textDocument":{{"uri":"file:///x.phg"}},"position":{{"line":{line},"character":{character}}}}}}}"#
    ))
    .unwrap()
}

#[test]
fn initialize_advertises_hover_and_definition() {
    let mut s = Server::default();
    let out = s.handle(&Json::parse(r#"{"id":1,"method":"initialize"}"#).unwrap());
    assert!(out[0].contains("hoverProvider"));
    assert!(out[0].contains("definitionProvider"));
}

#[test]
fn hover_on_a_call_shows_the_declaration_signature() {
    let mut s = Server::default();
    s.handle(&did_open("file:///x.phg", PROG));
    // line 2, char 35 → inside `helper(3)` on the `main` line.
    let out = s.handle(&req_at("hover", 2, 35));
    let body = &out[0];
    assert!(body.contains("function helper(int n) -> int"), "{body}");
}

#[test]
fn definition_jumps_to_the_declaration_line() {
    let mut s = Server::default();
    s.handle(&did_open("file:///x.phg", PROG));
    let out = s.handle(&req_at("definition", 2, 35));
    let body = &out[0];
    // `helper` is declared on source line 2 (1-based) ⇒ LSP line 1 (0-based).
    assert!(body.contains("file:///x.phg"), "{body}");
    assert!(body.contains("\"line\":1"), "{body}");
}

#[test]
fn hover_on_a_non_symbol_is_null() {
    let mut s = Server::default();
    s.handle(&did_open("file:///x.phg", PROG));
    // Column 0 of line 0 is the `package` keyword — not a resolvable top-level symbol reference.
    let out = s.handle(&req_at("hover", 0, 0));
    assert_eq!(out[0], "{\"jsonrpc\":\"2.0\",\"id\":7,\"result\":null}");
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

// ── v2: true end-ranges, locals, completion, document symbols ──────────────────────────────────

#[test]
fn initialize_advertises_completion_and_document_symbols() {
    let mut s = Server::default();
    let out = s.handle(&Json::parse(r#"{"id":1,"method":"initialize"}"#).unwrap());
    assert!(out[0].contains("completionProvider"), "{}", out[0]);
    assert!(out[0].contains("documentSymbolProvider"), "{}", out[0]);
}

#[test]
fn diagnostic_range_spans_the_whole_token() {
    // The unknown ident `nope` is on line 2 (0-based), chars 10..14; v2 widens the range to the token.
    let mut s = Server::default();
    let out = s.handle(&did_open(
        "file:///e.phg",
        "package Main;\nfunction main() -> void {\n  var x = nope;\n}",
    ));
    let body = &out[0];
    assert!(body.contains("E-UNKNOWN-IDENT"), "{body}");
    assert!(
        body.contains(
            "\"start\":{\"line\":2,\"character\":10},\"end\":{\"line\":2,\"character\":14}"
        ),
        "{body}"
    );
}

/// A program with a parameter `count` and a local `total`, used below their declarations.
const LOCALS: &str =
    "package Main;\nfunction f(int count) -> int {\n  var total = count;\n  return total;\n}";

#[test]
fn definition_on_a_param_use_jumps_to_the_param() {
    let mut s = Server::default();
    s.handle(&did_open("file:///x.phg", LOCALS));
    // line 2, char 15 → inside `count` in `var total = count;`.
    let out = s.handle(&req_at("definition", 2, 15));
    let body = &out[0];
    // The param `count` is declared on source line 1 (0-based).
    assert!(body.contains("\"line\":1"), "{body}");
}

#[test]
fn definition_on_a_local_use_jumps_to_the_var_decl() {
    let mut s = Server::default();
    s.handle(&did_open("file:///x.phg", LOCALS));
    // line 3, char 10 → inside `total` in `return total;`.
    let out = s.handle(&req_at("definition", 3, 10));
    let body = &out[0];
    // `var total` is on source line 2 (0-based).
    assert!(body.contains("\"line\":2"), "{body}");
}

#[test]
fn hover_on_a_local_shows_its_declaration() {
    let mut s = Server::default();
    s.handle(&did_open("file:///x.phg", LOCALS));
    let out = s.handle(&req_at("hover", 2, 15)); // on `count`
    let body = &out[0];
    assert!(body.contains("count"), "{body}");
}

#[test]
fn completion_lists_top_level_locals_and_keywords() {
    let mut s = Server::default();
    s.handle(&did_open("file:///x.phg", LOCALS));
    // Completion inside `f`'s body (line 3).
    let out = s.handle(&req_at("completion", 3, 4));
    let body = &out[0];
    assert!(
        body.contains("\"label\":\"f\""),
        "top-level fn missing: {body}"
    );
    assert!(
        body.contains("\"label\":\"count\""),
        "param local missing: {body}"
    );
    assert!(
        body.contains("\"label\":\"total\""),
        "var local missing: {body}"
    );
    assert!(
        body.contains("\"label\":\"return\""),
        "keyword missing: {body}"
    );
}

#[test]
fn document_symbols_outline_nests_class_members() {
    let mut s = Server::default();
    let src = "package Main;\nclass Point {\n  int x;\n  function get() -> int { return this.x; }\n}\nfunction main() -> void { }";
    s.handle(&did_open("file:///x.phg", src));
    let out = s.handle(&req_at("documentSymbol", 0, 0));
    let body = &out[0];
    assert!(body.contains("\"name\":\"Point\",\"kind\":5"), "{body}");
    assert!(body.contains("\"name\":\"x\",\"kind\":8"), "{body}");
    assert!(body.contains("\"name\":\"get\",\"kind\":6"), "{body}");
    assert!(body.contains("\"name\":\"main\",\"kind\":12"), "{body}");
    // `main` is a sibling of `Point`, not nested inside it.
    assert!(body.contains("\"children\":["), "{body}");
}

// ── references / document-highlight / rename / formatting (v3) ─────────────────────────────────

#[test]
fn initialize_advertises_v3_capabilities() {
    let mut s = Server::default();
    let out = s.handle(&Json::parse(r#"{"id":1,"method":"initialize"}"#).unwrap());
    assert!(out[0].contains("referencesProvider"), "{}", out[0]);
    assert!(out[0].contains("documentHighlightProvider"), "{}", out[0]);
    assert!(out[0].contains("renameProvider"), "{}", out[0]);
    assert!(out[0].contains("documentFormattingProvider"), "{}", out[0]);
}

#[test]
fn references_returns_declaration_and_use() {
    let mut s = Server::default();
    s.handle(&did_open("file:///x.phg", PROG));
    // Cursor on the `helper` call (line 2). References include the decl (line 1) + the call (line 2).
    let out = s.handle(&req_at("references", 2, 35));
    let body = &out[0];
    assert!(body.contains("\"line\":1"), "decl missing: {body}");
    assert!(body.contains("\"line\":2"), "use missing: {body}");
    // Two locations.
    assert_eq!(body.matches("\"uri\"").count(), 2, "{body}");
}

#[test]
fn document_highlight_marks_every_occurrence() {
    let mut s = Server::default();
    s.handle(&did_open("file:///x.phg", PROG));
    let out = s.handle(&req_at("documentHighlight", 1, 9)); // on the `helper` declaration name
    let body = &out[0];
    assert!(body.contains("\"kind\":1"), "{body}");
    assert_eq!(body.matches("\"range\"").count(), 2, "{body}");
}

#[test]
fn rename_produces_a_workspace_edit_for_every_occurrence() {
    let mut s = Server::default();
    s.handle(&did_open("file:///x.phg", PROG));
    let req = Json::parse(
        r#"{"id":7,"method":"textDocument/rename","params":{"textDocument":{"uri":"file:///x.phg"},"position":{"line":2,"character":35},"newName":"helper2"}}"#,
    )
    .unwrap();
    let out = s.handle(&req);
    let body = &out[0];
    assert!(body.contains("\"changes\""), "{body}");
    assert!(body.contains("file:///x.phg"), "{body}");
    assert_eq!(
        body.matches("helper2").count(),
        2,
        "both occurrences renamed: {body}"
    );
}

#[test]
fn formatting_returns_a_whole_document_edit() {
    let mut s = Server::default();
    // Deliberately un-canonical spacing so the formatter produces a change.
    s.handle(&did_open(
        "file:///f.phg",
        "package Main;\nfunction main() -> void {var x=1;}",
    ));
    let req = Json::parse(
        r#"{"id":7,"method":"textDocument/formatting","params":{"textDocument":{"uri":"file:///f.phg"}}}"#,
    )
    .unwrap();
    let out = s.handle(&req);
    let body = &out[0];
    assert!(body.contains("\"newText\""), "{body}");
    assert!(body.contains("\"range\""), "{body}");
}

#[test]
fn formatting_unparseable_buffer_yields_no_edit() {
    let mut s = Server::default();
    s.handle(&did_open("file:///g.phg", "package Main; function ((("));
    let req = Json::parse(
        r#"{"id":7,"method":"textDocument/formatting","params":{"textDocument":{"uri":"file:///g.phg"}}}"#,
    )
    .unwrap();
    let out = s.handle(&req);
    assert!(out[0].contains("\"result\":[]"), "{}", out[0]);
}
