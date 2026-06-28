//! Phorge language server (`phg lsp`) — Item D, `docs/specs/2026-06-28-lsp-design.md`.
//!
//! A minimal LSP over stdio so editors show Phorge diagnostics inline. **Hand-rolled** JSON-RPC in
//! `std`: an LSP server is not a security-critical primitive, so the dependency policy excludes
//! `tower-lsp`/`lsp-server`/`serde`. This module owns a tiny total JSON parser (for inbound request
//! bodies), the `Content-Length` framing, the server loop, and the diagnostic mapping. It is internal
//! tooling — **off the byte-identity spine** (it never touches the three execution backends), so it
//! carries no `run`/`runvm`/PHP parity risk. Diagnostics reuse the *exact* checker the CLI runs, so
//! editor squiggles equal `phg check`.
//!
//! Capabilities: `publishDiagnostics` (full document sync, token-width ranges), hover,
//! go-to-definition, completion, and document symbols — top-level *and* local/parameter resolution
//! (the v2 query layer lives in `scope.rs` + `symbols.rs`, all front-end-only). Member completion and
//! lambda/match-pattern binders remain a documented follow-up.

mod json;
mod scope;
mod symbols;
#[cfg(test)]
mod tests;

use crate::diagnostic::Diagnostic;
use crate::lexer::lex;
use crate::parser::Parser;
use json::Json;
use std::collections::HashMap;
use std::io::{self, BufRead, Write};

/// The server state: the open documents (URI → full text) and the shutdown flag.
#[derive(Default)]
pub struct Server {
    docs: HashMap<String, String>,
    /// Set by `shutdown`; `exit` then returns the process code (0 if shutdown was requested).
    shutting_down: bool,
}

/// An outbound LSP message body (a JSON string), produced by [`Server::handle`].
type Out = String;

impl Server {
    /// Handle one decoded inbound message, returning zero or more outbound message bodies
    /// (responses keyed by `id`, and/or `publishDiagnostics` notifications). Pure w.r.t. I/O so the
    /// tests can drive it directly without a real editor.
    pub fn handle(&mut self, msg: &Json) -> Vec<Out> {
        let method = msg.get("method").and_then(Json::as_str).unwrap_or("");
        let id = msg.get("id");
        match method {
            "initialize" => vec![response(id, INITIALIZE_RESULT)],
            // Notifications (no `id`): no response, but `didOpen`/`didChange` recompute diagnostics.
            "initialized" | "$/setTrace" => vec![],
            "textDocument/hover" => vec![response(id, &self.hover(msg))],
            "textDocument/definition" => vec![response(id, &self.definition(msg))],
            "textDocument/completion" => vec![response(id, &self.completion(msg))],
            "textDocument/documentSymbol" => vec![response(id, &self.document_symbols(msg))],
            "textDocument/didOpen" => self.on_open(msg),
            "textDocument/didChange" => self.on_change(msg),
            "textDocument/didClose" => {
                if let Some(uri) = doc_uri(msg) {
                    self.docs.remove(&uri);
                }
                vec![]
            }
            "shutdown" => {
                self.shutting_down = true;
                vec![response(id, "null")]
            }
            // An unknown *request* (has an id) gets a MethodNotFound error; an unknown notification is
            // ignored (the LSP spec requires servers to tolerate unknown notifications).
            _ => {
                if let Some(id) = id {
                    vec![error_response(id, -32601, "method not found")]
                } else {
                    vec![]
                }
            }
        }
    }

    /// `true` once `shutdown` has been received — `exit` after this is a clean (code 0) stop.
    pub fn shutting_down(&self) -> bool {
        self.shutting_down
    }

    fn on_open(&mut self, msg: &Json) -> Vec<Out> {
        let Some(td) = msg.get("params").and_then(|p| p.get("textDocument")) else {
            return vec![];
        };
        let (Some(uri), Some(text)) = (
            td.get("uri").and_then(Json::as_str),
            td.get("text").and_then(Json::as_str),
        ) else {
            return vec![];
        };
        self.docs.insert(uri.to_string(), text.to_string());
        vec![self.publish(uri)]
    }

    fn on_change(&mut self, msg: &Json) -> Vec<Out> {
        let Some(uri) = doc_uri(msg) else {
            return vec![];
        };
        // Full document sync: the last content change carries the whole new text.
        let text = msg
            .get("params")
            .and_then(|p| p.get("contentChanges"))
            .and_then(Json::as_array)
            .and_then(|cs| cs.last())
            .and_then(|c| c.get("text"))
            .and_then(Json::as_str);
        if let Some(text) = text {
            self.docs.insert(uri.clone(), text.to_string());
        }
        vec![self.publish(&uri)]
    }

    /// Resolve a `textDocument/{hover,definition,completion}` request to the buffer text, the cursor's
    /// byte offset, the identifier name under the cursor (if any), and the parsed program. `None` if
    /// the document/position can't be located or the buffer doesn't parse.
    fn symbol_at(
        &self,
        msg: &Json,
    ) -> Option<(String, usize, Option<String>, crate::ast::Program)> {
        let uri = doc_uri(msg)?;
        let text = self.docs.get(&uri)?;
        let pos = msg.get("params")?.get("position")?;
        let line = num(pos.get("line"))?;
        let character = num(pos.get("character"))?;
        let offset = symbols::offset_at(text, line, character)?;
        let name = symbols::ident_at(text, offset);
        let tokens = crate::lexer::lex(text).ok()?;
        let program = Parser::new(tokens).parse_program().ok()?;
        Some((text.clone(), offset, name, program))
    }

    /// Resolve the identifier under the cursor to its declaration span: a top-level symbol first, then
    /// a local binding (parameter / `var` / `for` var / `if`-let / `catch` / destructure) of the
    /// enclosing callable. `is_local` distinguishes the two for hover rendering.
    fn resolve_decl(
        offset: usize,
        name: &str,
        program: &crate::ast::Program,
    ) -> Option<(crate::token::Span, bool)> {
        if let Some((_kind, span)) = symbols::definition_of(program, name) {
            return Some((span, false));
        }
        // No top-level match → a local of the enclosing callable (scoped + nearest-preceding).
        scope::local_definition(program, name, offset).map(|sp| (sp, true))
    }

    /// `textDocument/hover` — show the declaration signature of the symbol under the cursor (top-level
    /// or local), or `null`.
    fn hover(&self, msg: &Json) -> String {
        let Some((text, offset, Some(name), program)) = self.symbol_at(msg) else {
            return "null".to_string();
        };
        match Self::resolve_decl(offset, &name, &program) {
            Some((span, is_local)) => {
                let sig = if is_local {
                    symbols::local_signature_text(&text, span)
                } else {
                    symbols::signature_text(&text, span)
                };
                format!(
                    "{{\"contents\":{{\"kind\":\"markdown\",\"value\":\"```phorge\\n{}\\n```\"}}}}",
                    escape(&sig)
                )
            }
            None => "null".to_string(),
        }
    }

    /// `textDocument/definition` — the `Location` of the symbol's declaration (top-level or local), or
    /// `null`. The range spans the declaration's name token (true end-position).
    fn definition(&self, msg: &Json) -> String {
        let Some(uri) = doc_uri(msg) else {
            return "null".to_string();
        };
        let Some((text, offset, Some(name), program)) = self.symbol_at(msg) else {
            return "null".to_string();
        };
        match Self::resolve_decl(offset, &name, &program) {
            Some((span, _)) => {
                let (sl, sc) = scope::position_at(&text, span.start);
                let (el, ec) = scope::position_at(&text, span.start + span.len);
                format!(
                    "{{\"uri\":\"{}\",\"range\":{}}}",
                    escape(&uri),
                    range_json(sl, sc, el, ec)
                )
            }
            None => "null".to_string(),
        }
    }

    /// `textDocument/completion` — top-level declaration names, the enclosing callable's in-scope
    /// locals/params, and the language keywords. No member completion yet (needs the resolved-type
    /// index); documented in the LSP design.
    fn completion(&self, msg: &Json) -> String {
        let Some((_text, offset, _name, program)) = self.symbol_at(msg) else {
            return "{\"isIncomplete\":false,\"items\":[]}".to_string();
        };
        let mut items: Vec<String> = Vec::new();
        // Top-level declarations (deduped names already unique per the checker).
        for (name, kind) in symbols::top_level_symbols(&program) {
            items.push(completion_item(&name, kind, "phorge symbol"));
        }
        // In-scope locals/params of the enclosing callable.
        let mut seen_local: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (name, _span) in scope::enclosing_bindings(&program, offset) {
            if seen_local.insert(name.clone()) {
                items.push(completion_item(&name, 6 /* Variable */, "local"));
            }
        }
        // Language keywords (CompletionItemKind 14 = Keyword).
        for kw in KEYWORDS {
            items.push(completion_item(kw, 14, "keyword"));
        }
        format!("{{\"isIncomplete\":false,\"items\":[{}]}}", items.join(","))
    }

    /// `textDocument/documentSymbol` — a hierarchical outline of the buffer: every top-level item, with
    /// classes/enums/interfaces/traits carrying their members/variants as children.
    fn document_symbols(&self, msg: &Json) -> String {
        let Some(uri) = doc_uri(msg) else {
            return "[]".to_string();
        };
        let Some(text) = self.docs.get(&uri) else {
            return "[]".to_string();
        };
        let Ok(tokens) = crate::lexer::lex(text) else {
            return "[]".to_string();
        };
        let Ok(program) = Parser::new(tokens).parse_program() else {
            return "[]".to_string();
        };
        symbols::document_symbols_json(text, &program)
    }

    /// Build a `textDocument/publishDiagnostics` notification for `uri` from the current buffer.
    fn publish(&self, uri: &str) -> Out {
        let text = self.docs.get(uri).map(String::as_str).unwrap_or("");
        let diags = diagnostics_for(text);
        let items: Vec<String> = diags.iter().map(|d| lsp_diagnostic_json(d, text)).collect();
        format!(
            "{{\"jsonrpc\":\"2.0\",\"method\":\"textDocument/publishDiagnostics\",\"params\":{{\"uri\":\"{}\",\"diagnostics\":[{}]}}}}",
            escape(uri),
            items.join(",")
        )
    }
}

/// The advertised server capabilities: full-text sync (`1`) — the client sends the whole document on
/// each change — push diagnostics, hover, go-to-definition, completion, and document symbols (v2).
const INITIALIZE_RESULT: &str =
    "{\"capabilities\":{\"textDocumentSync\":1,\"hoverProvider\":true,\"definitionProvider\":true,\"completionProvider\":{\"triggerCharacters\":[\".\"]},\"documentSymbolProvider\":true},\"serverInfo\":{\"name\":\"phorge-lsp\"}}";

/// Compute the diagnostics for a document buffer — the **same** pipeline `phg check` runs (lex →
/// parse → `check`), so editor diagnostics equal the CLI's. A lex or parse error is a single
/// diagnostic at its span; otherwise the checker's errors (or, when clean, its non-fatal warnings).
fn diagnostics_for(text: &str) -> Vec<Diagnostic> {
    let tokens = match lex(text) {
        Ok(t) => t,
        Err(d) => return vec![d],
    };
    let program = match Parser::new(tokens).parse_program() {
        Ok(p) => p,
        Err(d) => return vec![d],
    };
    match crate::checker::check(&program) {
        Ok(warnings) => warnings,
        Err(errors) => errors,
    }
}

/// Map a Phorge `Diagnostic` to an LSP `Diagnostic` JSON object. Phorge `line`/`col` are 1-based; LSP
/// positions are 0-based. v2 spans the offending **token** (the `Diagnostic` struct is span-less, so
/// the end is re-derived from `text`: the token starting at the caret gives its `Span.len`; absent
/// that, a 1-char caret). `code` carries the stable `E-…`/`W-…` code (resolvable via `phg explain`); a
/// `hint` is appended to the message.
fn lsp_diagnostic_json(d: &Diagnostic, text: &str) -> String {
    let line = d.line.saturating_sub(1);
    let col = d.col.saturating_sub(1);
    // True end-range: the token at the caret widens the highlight to the whole identifier/keyword.
    let (end_line, end_col) = symbols::offset_at(text, line, col)
        .and_then(|off| symbols::token_span_at(text, off))
        .map_or((line, col + 1), |sp| {
            scope::position_at(text, sp.start + sp.len)
        });
    // Warnings are stage-independent; the checker returns them via `Ok`, errors via `Err`. Here we
    // only know per-diagnostic intent through its code prefix (`W-` ⇒ warning), falling back to error.
    let severity = match d.code {
        Some(c) if c.starts_with("W-") => 2,
        _ => 1,
    };
    let message = match &d.hint {
        Some(h) => format!("{}\n\nhint: {h}", d.message),
        None => d.message.clone(),
    };
    let code = d
        .code
        .map_or_else(|| String::from("null"), |c| format!("\"{}\"", escape(c)));
    format!(
        "{{\"range\":{},\"severity\":{severity},\"code\":{code},\"source\":\"phorge\",\"message\":\"{}\"}}",
        range_json(line, col, end_line, end_col),
        escape(&message)
    )
}

/// An LSP `Range` JSON object from 0-based start/end `(line, character)` pairs.
fn range_json(sl: u32, sc: u32, el: u32, ec: u32) -> String {
    format!(
        "{{\"start\":{{\"line\":{sl},\"character\":{sc}}},\"end\":{{\"line\":{el},\"character\":{ec}}}}}"
    )
}

/// A `CompletionItem` JSON object: a label, an LSP `CompletionItemKind`, and a short detail string.
fn completion_item(label: &str, kind: u32, detail: &str) -> String {
    format!(
        "{{\"label\":\"{}\",\"kind\":{kind},\"detail\":\"{}\"}}",
        escape(label),
        escape(detail)
    )
}

/// The Phorge keyword set surfaced in completion (CompletionItemKind 14 = Keyword). Not exhaustive of
/// every contextual word, but the structural keywords a user types most.
const KEYWORDS: &[&str] = &[
    "package",
    "import",
    "function",
    "class",
    "enum",
    "interface",
    "trait",
    "type",
    "return",
    "if",
    "else",
    "for",
    "while",
    "do",
    "in",
    "match",
    "var",
    "mutable",
    "static",
    "const",
    "open",
    "abstract",
    "public",
    "private",
    "protected",
    "new",
    "this",
    "true",
    "false",
    "null",
    "throw",
    "try",
    "catch",
    "finally",
    "break",
    "continue",
    "instanceof",
    "use",
    "extends",
    "implements",
    "as",
    "test",
    "never",
];

/// Build a JSON-RPC success response for request `id` with a raw `result` JSON fragment.
fn response(id: Option<&Json>, result: &str) -> Out {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{result}}}",
        id_json(id)
    )
}

/// Build a JSON-RPC error response.
fn error_response(id: &Json, code: i64, message: &str) -> Out {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{},\"error\":{{\"code\":{code},\"message\":\"{}\"}}}}",
        id_json(Some(id)),
        escape(message)
    )
}

/// Render an `id` (a number or string) back into the response; `null` if absent.
fn id_json(id: Option<&Json>) -> String {
    match id {
        Some(Json::Num(n)) => format!("{}", *n as i64),
        Some(Json::Str(s)) => format!("\"{}\"", escape(s)),
        _ => "null".to_string(),
    }
}

/// Read a JSON number as a `u32` (LSP positions are non-negative integers).
fn num(j: Option<&Json>) -> Option<u32> {
    match j {
        Some(Json::Num(n)) if *n >= 0.0 => Some(*n as u32),
        _ => None,
    }
}

/// The document URI of a `textDocument/...` notification (`params.textDocument.uri`).
fn doc_uri(msg: &Json) -> Option<String> {
    msg.get("params")
        .and_then(|p| p.get("textDocument"))
        .and_then(|td| td.get("uri"))
        .and_then(Json::as_str)
        .map(str::to_string)
}

/// Minimal JSON string escaping for outbound message bodies (a local copy — `diagnostic::json_escape`
/// is private). Covers the control + quote/backslash set LSP message text needs.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Run the stdio JSON-RPC loop: read `Content-Length`-framed messages from stdin, dispatch each, write
/// framed responses/notifications to stdout. Returns the process exit code (`exit` notification).
pub fn run() -> io::Result<i32> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let stdout = io::stdout();
    let mut server = Server::default();
    loop {
        let Some(body) = read_message(&mut reader)? else {
            // EOF without `exit` — treat as a (non-clean) stop.
            return Ok(1);
        };
        // `exit` ends the loop: code 0 iff `shutdown` preceded it (LSP spec).
        if let Some(msg) = Json::parse(&body) {
            if msg.get("method").and_then(Json::as_str) == Some("exit") {
                return Ok(i32::from(!server.shutting_down()));
            }
            for out in server.handle(&msg) {
                write_message(&stdout, &out)?;
            }
        }
        // A body that fails to parse is ignored (a robust server tolerates garbage frames).
    }
}

/// Read one `Content-Length`-framed message body, or `None` at EOF.
fn read_message(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut length: Option<usize> = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None); // EOF
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break; // end of headers
        }
        if let Some(v) = trimmed.strip_prefix("Content-Length:") {
            length = v.trim().parse::<usize>().ok();
        }
    }
    let Some(len) = length else {
        return Ok(Some(String::new())); // no length ⇒ empty body, ignored upstream
    };
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
}

/// Write one `Content-Length`-framed message body to stdout, flushing.
fn write_message(stdout: &io::Stdout, body: &str) -> io::Result<()> {
    let mut w = stdout.lock();
    write!(w, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    w.flush()
}
