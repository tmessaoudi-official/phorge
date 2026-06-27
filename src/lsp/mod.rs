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
//! v1 capabilities: `publishDiagnostics` (full document sync). Hover + go-to-definition are a
//! follow-up slice (they need a position→symbol index over the checker's resolved tables).

mod json;
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

    /// Build a `textDocument/publishDiagnostics` notification for `uri` from the current buffer.
    fn publish(&self, uri: &str) -> Out {
        let text = self.docs.get(uri).map(String::as_str).unwrap_or("");
        let diags = diagnostics_for(text);
        let items: Vec<String> = diags.iter().map(lsp_diagnostic_json).collect();
        format!(
            "{{\"jsonrpc\":\"2.0\",\"method\":\"textDocument/publishDiagnostics\",\"params\":{{\"uri\":\"{}\",\"diagnostics\":[{}]}}}}",
            escape(uri),
            items.join(",")
        )
    }
}

/// The advertised server capabilities (v1): full-text sync (`1`) — the client sends the whole document
/// on each change — and diagnostics via push. Hover/definition are added when that slice lands.
const INITIALIZE_RESULT: &str =
    "{\"capabilities\":{\"textDocumentSync\":1},\"serverInfo\":{\"name\":\"phorge-lsp\"}}";

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
/// positions are 0-based. v1 highlights a single character at the caret (the flattened `Diagnostic`
/// drops the span length — a v2 refinement threads it through for a full range). `code` carries the
/// stable `E-…`/`W-…` code (resolvable via `phg explain`); a `hint` is appended to the message.
fn lsp_diagnostic_json(d: &Diagnostic) -> String {
    let line = d.line.saturating_sub(1);
    let col = d.col.saturating_sub(1);
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
        "{{\"range\":{{\"start\":{{\"line\":{line},\"character\":{col}}},\"end\":{{\"line\":{line},\"character\":{}}}}},\"severity\":{severity},\"code\":{code},\"source\":\"phorge\",\"message\":\"{}\"}}",
        col + 1,
        escape(&message)
    )
}

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
