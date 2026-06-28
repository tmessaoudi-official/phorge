//! `phg fmt` — the canonical-form source formatter (GA rock 2 tooling). A full-surface,
//! comment-preserving AST printer (`printer`) plus the `format` entry that lexes → parses → prints.
//!
//! The formatter's one hard rule is **meaning preservation**: it prints from the parsed AST (not by
//! re-spacing tokens), so `parse(fmt(x)) ≡ parse(x)` holds by construction, and the printer's matches
//! are exhaustive so it can never silently mis-handle a node. It is also **idempotent**
//! (`fmt(fmt(x)) == fmt(x)`) and **never touches an unparseable file** (a parse error is returned, the
//! source left alone).

mod printer;

#[cfg(test)]
mod tests;

use crate::diagnostic::Diagnostic;
use crate::lexer::lex_with_comments;
use crate::parser::Parser;

/// Format Phorj source to canonical form. Returns the formatted text, or a `Diagnostic` if the
/// source does not lex/parse (the caller must NOT write the file in that case — a formatter never
/// corrupts broken source).
pub fn format(src: &str) -> Result<String, Diagnostic> {
    let (tokens, comments) = lex_with_comments(src)?;
    let program = Parser::new(tokens).parse_program()?;
    printer::format_program(&program, &comments).map_err(|m| {
        // The printer only errors on AST a parsed program cannot contain — treat as an internal
        // formatter bug, surfaced (never silently) as a Parse-stage diagnostic.
        Diagnostic::new(crate::diagnostic::Stage::Parse, m, 1, 1)
    })
}
