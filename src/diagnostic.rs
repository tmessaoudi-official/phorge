//! One structured error shape for every pipeline stage (M2 P3.5 Wave 2 Task 2.1).
//!
//! Before this, four near-identical error types existed (`LexError`/`ParseError`/`TypeError`
//! were byte-identical `{message,line,col}` structs; `RuntimeError` carried only a message;
//! the VM and compiler returned a bare `String`). They are now all `Diagnostic`, tagged with
//! the [`Stage`] they came from and rendered uniformly. This single shape is also the seam a
//! future `--json` / LSP layer hangs off (one place to add a serializer).
//!
//! Position is `line`/`col` (1-based; `0` means "unknown"), not the lexer's full
//! [`crate::token::Span`] — no error renderer consumes the span's byte offsets, and every
//! construction site already has a line/col in hand.

use std::fmt;

use crate::token::Span;

/// Which pipeline stage produced a [`Diagnostic`]. Drives the rendered prefix
/// (`"parse error …"`, `"runtime error …"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Lex,
    Parse,
    Type,
    Compile,
    Runtime,
}

impl Stage {
    /// The lowercase word used in the rendered prefix.
    fn label(self) -> &'static str {
        match self {
            Stage::Lex => "lex",
            Stage::Parse => "parse",
            Stage::Type => "type",
            Stage::Compile => "compile",
            Stage::Runtime => "runtime",
        }
    }
}

/// A single error, anywhere in the pipeline. `line == 0` means no position is known (the
/// compiler and the tree-walking interpreter don't track one); `col == 0` with `line > 0`
/// means a line is known but not a column (VM runtime errors, located via `Chunk.lines`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub stage: Stage,
    pub message: String,
    pub line: u32,
    pub col: u32,
    /// Stable machine code (e.g. `E-UNKNOWN-IDENT`) keyed by `phg explain`; `None` if uncoded.
    pub code: Option<&'static str>,
    /// An optional one-line suggestion ("did you mean `…`?").
    pub hint: Option<String>,
}

impl Diagnostic {
    /// Full constructor.
    pub fn new(stage: Stage, message: impl Into<String>, line: u32, col: u32) -> Self {
        Diagnostic {
            stage,
            message: message.into(),
            line,
            col,
            code: None,
            hint: None,
        }
    }

    /// Attach a stable diagnostic code (consumed by `phg explain`).
    #[must_use]
    pub fn with_code(mut self, code: &'static str) -> Self {
        self.code = Some(code);
        self
    }

    /// Attach a one-line hint shown beneath the diagnostic.
    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Render with the offending source line and a caret under the column, plus the code and hint
    /// when present. Falls back to the plain [`Display`] form when no position is known (`line == 0`
    /// — the tree-walking interpreter and the compiler track none).
    pub fn render(&self, src: &str) -> String {
        let mut s = self.to_string();
        if self.line > 0 {
            if let Some(line_text) = src.lines().nth((self.line - 1) as usize) {
                s.push('\n');
                s.push_str(line_text);
                if self.col > 0 {
                    s.push('\n');
                    // Indent the caret to the column, preserving tabs so it lines up regardless of
                    // the terminal's tab width.
                    let pad: String = line_text
                        .chars()
                        .take((self.col - 1) as usize)
                        .map(|c| if c == '\t' { '\t' } else { ' ' })
                        .collect();
                    s.push_str(&pad);
                    s.push('^');
                }
            }
        }
        if let Some(code) = self.code {
            s.push_str(&format!("\n  [{code}]"));
        }
        if let Some(hint) = &self.hint {
            s.push_str(&format!("\n  hint: {hint}"));
        }
        s
    }

    /// Build a front-end diagnostic from a token [`Span`] (uses its `line`/`col`).
    pub fn at(stage: Stage, span: Span, message: impl Into<String>) -> Self {
        Self::new(stage, message, span.line, span.col)
    }

    /// A runtime fault with no known position (the tree-walking interpreter).
    pub fn runtime(message: impl Into<String>) -> Self {
        Self::new(Stage::Runtime, message, 0, 0)
    }

    /// A runtime fault located at a source `line` (the VM, via `Chunk.lines[ip]`).
    pub fn runtime_at_line(message: impl Into<String>, line: u32) -> Self {
        Self::new(Stage::Runtime, message, line, 0)
    }

    /// A compile-time fault with no position (the bytecode compiler tracks none yet).
    pub fn compile(message: impl Into<String>) -> Self {
        Self::new(Stage::Compile, message, 0, 0)
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let stage = self.stage.label();
        if self.line == 0 {
            write!(f, "{stage} error: {}", self.message)
        } else if self.col == 0 {
            write!(f, "{stage} error at {}: {}", self.line, self.message)
        } else {
            write!(
                f,
                "{stage} error at {}:{}: {}",
                self.line, self.col, self.message
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_line_and_col_for_front_end_stages() {
        // lex/parse/type always carry a real line:col — output is unchanged from the
        // pre-Diagnostic format `"<stage> error at L:C: <msg>"`.
        let d = Diagnostic::new(Stage::Parse, "expected ';'", 3, 7);
        assert_eq!(d.to_string(), "parse error at 3:7: expected ';'");
        let t = Diagnostic::new(Stage::Type, "type mismatch", 10, 2);
        assert_eq!(t.to_string(), "type error at 10:2: type mismatch");
    }

    #[test]
    fn renders_line_only_when_col_is_zero() {
        // VM runtime errors know the line (from Chunk.lines) but not a column.
        let d = Diagnostic::runtime_at_line("division by zero", 4);
        assert_eq!(d.to_string(), "runtime error at 4: division by zero");
    }

    #[test]
    fn renders_no_position_when_line_is_zero() {
        // The interpreter and the compiler track no position — output matches the old
        // `"runtime error: …"` / `"compile error: …"` forms exactly.
        assert_eq!(
            Diagnostic::runtime("division by zero").to_string(),
            "runtime error: division by zero"
        );
        assert_eq!(
            Diagnostic::compile("indexing is not supported (M1 surface)").to_string(),
            "compile error: indexing is not supported (M1 surface)"
        );
    }

    #[test]
    fn at_reads_line_and_col_from_span() {
        let span = Span {
            start: 0,
            len: 1,
            line: 5,
            col: 9,
        };
        let d = Diagnostic::at(Stage::Lex, span, "bad token");
        assert_eq!((d.line, d.col), (5, 9));
        assert_eq!(d.to_string(), "lex error at 5:9: bad token");
    }

    #[test]
    fn render_underlines_the_offending_span_and_appends_hint_and_code() {
        let src = "function main() {\n    foo;\n}";
        let d = Diagnostic::new(Stage::Type, "unknown identifier `foo`", 2, 5)
            .with_code("E-UNKNOWN-IDENT")
            .with_hint("did you mean `for`?");
        let r = d.render(src);
        assert!(
            r.starts_with("type error at 2:5: unknown identifier `foo`"),
            "{r}"
        );
        assert!(r.contains("    foo;"), "missing source line:\n{r}");
        assert!(r.contains("    ^"), "missing caret:\n{r}");
        assert!(r.contains("[E-UNKNOWN-IDENT]"), "missing code:\n{r}");
        assert!(
            r.contains("hint: did you mean `for`?"),
            "missing hint:\n{r}"
        );
    }

    #[test]
    fn render_without_position_is_just_the_display_line() {
        let d = Diagnostic::runtime("division by zero");
        assert_eq!(d.render("whatever"), "runtime error: division by zero");
    }
}
