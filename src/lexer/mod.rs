//! Hand-written lexer: source `&str` → `Vec<Token>`. Iterative (no recursion), so unlike the
//! parser/checker it never contributes to the recursion-depth budget those stages guard. Faults
//! surface as a unified `diagnostic::Diagnostic` (`Stage::Lex`) carrying line/col.

use crate::diagnostic::{Diagnostic, Stage};
use crate::token::{Comment, CommentKind, Span, StrSeg, Token, TokenKind};

pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: u32,
    col: u32,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Lexer {
            src: src.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn peek2(&self) -> Option<u8> {
        self.src.get(self.pos + 1).copied()
    }

    fn peek3(&self) -> Option<u8> {
        self.src.get(self.pos + 2).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.pos += 1;
        if b == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(b)
    }

    fn skip_whitespace(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' {
                self.bump();
            } else {
                break;
            }
        }
    }

    fn scan_number(&mut self, start: usize, line: u32, col: u32) -> Result<Token, Diagnostic> {
        // Base-prefixed integers `0x` / `0b` / `0o` (Rust-style; a bare leading `0` stays decimal —
        // implicit octal is a PHP footgun we deliberately drop). Underscore separators are allowed.
        if self.peek() == Some(b'0') {
            if let Some(radix) = match self.src.get(self.pos + 1) {
                Some(b'x' | b'X') => Some(16u32),
                Some(b'b' | b'B') => Some(2),
                Some(b'o' | b'O') => Some(8),
                _ => None,
            } {
                self.bump();
                self.bump(); // consume the `0x` / `0b` / `0o` prefix
                let mut digits = String::new();
                while let Some(c) = self.peek() {
                    if c == b'_' {
                        self.bump();
                    } else if (c as char).is_digit(radix) {
                        digits.push(c as char);
                        self.bump();
                    } else {
                        break;
                    }
                }
                if digits.is_empty() {
                    return Err(Diagnostic::new(
                        Stage::Lex,
                        "expected a digit after the numeric base prefix",
                        line,
                        col,
                    ));
                }
                let i = i64::from_str_radix(&digits, radix).map_err(|_| {
                    Diagnostic::new(Stage::Lex, "integer literal out of range", line, col)
                })?;
                return Ok(Token {
                    kind: TokenKind::Int(i),
                    span: Span {
                        start,
                        len: self.pos - start,
                        line,
                        col,
                    },
                });
            }
        }
        // Decimal — digits with optional `_` separators, optional fraction, optional exponent.
        while matches!(self.peek(), Some(b) if b.is_ascii_digit() || b == b'_') {
            self.bump();
        }
        let mut is_float = false;
        if self.peek() == Some(b'.') && matches!(self.peek2(), Some(d) if d.is_ascii_digit()) {
            is_float = true;
            self.bump(); // consume '.'
            while matches!(self.peek(), Some(b) if b.is_ascii_digit() || b == b'_') {
                self.bump();
            }
        }
        // Exponent `e`/`E`, only when followed by an optional sign and at least one digit (so `3em`
        // is `3` then the identifier `em`, not a malformed exponent).
        if matches!(self.peek(), Some(b'e' | b'E')) {
            let exp_ok = match self.src.get(self.pos + 1) {
                Some(d) if d.is_ascii_digit() => true,
                Some(b'+' | b'-') => {
                    matches!(self.src.get(self.pos + 2), Some(d) if d.is_ascii_digit())
                }
                _ => false,
            };
            if exp_ok {
                is_float = true;
                self.bump(); // 'e'
                if matches!(self.peek(), Some(b'+' | b'-')) {
                    self.bump();
                }
                while matches!(self.peek(), Some(b) if b.is_ascii_digit() || b == b'_') {
                    self.bump();
                }
            }
        }
        // `decimal` suffix `…d` (M-NUM S1): a numeric literal immediately followed by `d` that is NOT
        // continued by another identifier char (so `3d` is a decimal but `3days` is `3` + `days`). The
        // scale comes from the literal TEXT (trailing zeros preserved): `1.50d` ⇒ scale 2. An
        // exponent literal (`1e3d`) is rejected — `e`-exponent on a decimal is out of scope this slice.
        if self.peek() == Some(b'd')
            && !matches!(self.src.get(self.pos + 1), Some(c) if c.is_ascii_alphanumeric() || *c == b'_')
        {
            let raw_dec = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
            self.bump(); // consume the `d`
            if is_float {
                // `is_float` here means a fraction OR an exponent was scanned. A `.` fraction is fine
                // for a decimal; an `e`-exponent is rejected this slice.
                let stripped: String = raw_dec.chars().filter(|c| *c != '_').collect();
                if stripped.contains(['e', 'E']) {
                    return Err(Diagnostic::new(
                        Stage::Lex,
                        "an exponent is not allowed on a `decimal` literal (`1e3d`) — write the digits out",
                        line,
                        col,
                    )
                    .with_code("E-DECIMAL-LITERAL"));
                }
            }
            let (unscaled, scale) = parse_decimal_literal(raw_dec).ok_or_else(|| {
                Diagnostic::new(
                    Stage::Lex,
                    "`decimal` literal is out of range (exceeds i128)",
                    line,
                    col,
                )
                .with_code("E-DECIMAL-LITERAL")
            })?;
            return Ok(Token {
                kind: TokenKind::Decimal(unscaled, scale),
                span: Span {
                    start,
                    len: self.pos - start,
                    line,
                    col,
                },
            });
        }
        let raw = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
        // Strip `_` separators before parsing; the literal's value (not its surface form) is what
        // reaches the AST, so every base/format collapses to the same Int/Float — byte-identical.
        let text: String = raw.chars().filter(|c| *c != '_').collect();
        let kind = if is_float {
            let f: f64 = text.parse().map_err(|_| {
                Diagnostic::new(Stage::Lex, "float literal out of range", line, col)
            })?;
            if !f.is_finite() {
                return Err(Diagnostic::new(
                    Stage::Lex,
                    "float literal out of range",
                    line,
                    col,
                ));
            }
            TokenKind::Float(f)
        } else {
            let i: i64 = text.parse().map_err(|_| {
                Diagnostic::new(Stage::Lex, "integer literal out of range", line, col)
            })?;
            TokenKind::Int(i)
        };
        Ok(Token {
            kind,
            span: Span {
                start,
                len: self.pos - start,
                line,
                col,
            },
        })
    }

    fn skip_line_comment(&mut self) {
        while let Some(b) = self.peek() {
            if b == b'\n' {
                break;
            }
            self.bump();
        }
    }

    fn skip_block_comment(&mut self) -> Result<(), Diagnostic> {
        let (sl, sc) = (self.line, self.col);
        self.bump();
        self.bump(); // consume /*
        loop {
            match self.peek() {
                None => {
                    return Err(Diagnostic::new(
                        Stage::Lex,
                        "unterminated block comment",
                        sl,
                        sc,
                    ))
                }
                Some(b'*') if self.peek2() == Some(b'/') => {
                    self.bump();
                    self.bump();
                    return Ok(());
                }
                _ => {
                    self.bump();
                }
            }
        }
    }

    fn scan_string(&mut self, start: usize, line: u32, col: u32) -> Result<Token, Diagnostic> {
        self.bump(); // opening quote
                     // Single pass that BOTH expands escapes AND splits interpolation — only here do
                     // we know whether a `{` is a real interpolation brace or a `\{` literal escape (a
                     // parser-side split on the escape-expanded value couldn't tell `\{` from `\\{`).
                     // Escapes expand into whichever buffer is active: the literal run, or — when
                     // inside `{…}` — the interpolation's inner source (so an escaped quote `\"` in
                     // `"{m[\"k\"]}"` becomes a real `"` in the re-lexed expression, matching the old
                     // expand-then-split pipeline). A bare unescaped `{` opens an interpolation, the
                     // first unescaped `}` closes it (no nesting — matching the prior splitter). Source
                     // is valid UTF-8, so the `from_utf8` calls cannot fail.
        let mut segs: Vec<StrSeg> = Vec::new();
        let mut lit: Vec<u8> = Vec::new();
        let mut interp: Option<Vec<u8>> = None; // `Some` while inside `{…}`
        let mut interp_start: usize = 0; // absolute byte offset of the active interpolation's content
        loop {
            // Snapshot before consuming, so an invalid escape reports the backslash's column.
            let (el, ec) = (self.line, self.col);
            match self.bump() {
                None => {
                    return Err(Diagnostic::new(
                        Stage::Lex,
                        "unterminated string",
                        line,
                        col,
                    ))
                }
                Some(b'"') => {
                    if interp.is_some() {
                        return Err(Diagnostic::new(
                            Stage::Lex,
                            "unterminated interpolation '{' in string",
                            line,
                            col,
                        ));
                    }
                    break;
                }
                // A bare `{` outside an interpolation opens one (flush the pending literal first); a
                // `{` already inside one is just a character of the inner expression source.
                Some(b'{') if interp.is_none() => {
                    if !lit.is_empty() {
                        segs.push(StrSeg::Lit(
                            String::from_utf8(std::mem::take(&mut lit)).expect("valid UTF-8"),
                        ));
                    }
                    interp = Some(Vec::new());
                    // `self.pos` is now just past the opening `{` → the first byte of the inner source.
                    interp_start = self.pos;
                }
                // The first unescaped `}` closes the interpolation; a `}` outside one is an error
                // (write `\}` for a literal brace).
                Some(b'}') if interp.is_some() => {
                    segs.push(StrSeg::Interp(
                        String::from_utf8(interp.take().expect("inside interp"))
                            .expect("valid UTF-8"),
                        interp_start,
                    ));
                }
                Some(b'}') => return Err(Diagnostic::new(
                    Stage::Lex,
                    "unexpected '}' in string (no matching '{'; write `\\}` for a literal brace)",
                    el,
                    ec,
                )),
                Some(b'\\') => {
                    // Expand the escape into the active buffer (interpolation inner if inside one,
                    // else the literal run).
                    let buf = interp.as_mut().unwrap_or(&mut lit);
                    match self.bump() {
                        Some(b'n') => buf.push(b'\n'),
                        Some(b't') => buf.push(b'\t'),
                        Some(b'r') => buf.push(b'\r'),
                        Some(b'\\') => buf.push(b'\\'),
                        Some(b'"') => buf.push(b'"'),
                        // `\{` / `\}` — a literal brace (Phase 1 string slice): emitted as a byte into
                        // the active buffer, so it never opens/closes an interpolation.
                        Some(b'{') => buf.push(b'{'),
                        Some(b'}') => buf.push(b'}'),
                        // `\u{HEX}` — a Unicode escape: 1–6 hex digits → UTF-8 bytes at lex time.
                        Some(b'u') => self.scan_unicode_escape(buf, el, ec)?,
                        Some(other) => {
                            return Err(Diagnostic::new(
                                Stage::Lex,
                                format!("invalid escape \\{}", other as char),
                                el,
                                ec,
                            ))
                        }
                        None => {
                            return Err(Diagnostic::new(
                                Stage::Lex,
                                "unterminated string",
                                line,
                                col,
                            ))
                        }
                    }
                }
                Some(other) => interp.as_mut().unwrap_or(&mut lit).push(other),
            }
        }
        if !lit.is_empty() {
            segs.push(StrSeg::Lit(
                String::from_utf8(lit).expect("string literal run is valid UTF-8"),
            ));
        }
        Ok(Token {
            kind: TokenKind::Str(segs),
            span: Span {
                start,
                len: self.pos - start,
                line,
                col,
            },
        })
    }

    /// A-62: scan a `"""…"""` multi-line **text block**. The opening `"""` is followed by a
    /// mandatory newline (the opening line carries no content); the closing delimiter is a line whose
    /// only non-whitespace content is `"""`. Incidental indentation is stripped (Java JEP-378: the
    /// minimal leading-whitespace over every non-blank content line **and** the closing line), and
    /// each line's trailing whitespace is trimmed. Interpolation (`{expr}`) and escapes work exactly
    /// as in `"…"` — implemented by dedenting, escaping bare `"`, then routing the body through
    /// [`Self::scan_string`] (no duplicated escape/interpolation logic), yielding the same
    /// `TokenKind::Str(segs)` a single-line string would.
    fn scan_text_block(&mut self, start: usize, line: u32, col: u32) -> Result<Token, Diagnostic> {
        self.bump();
        self.bump();
        self.bump(); // opening `"""`
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r')) {
            self.bump();
        }
        if self.bump() != Some(b'\n') {
            return Err(Diagnostic::new(
                Stage::Lex,
                "a `\"\"\"` text block must be followed by a newline (the opening line carries no content)",
                line,
                col,
            ));
        }
        // Collect raw content lines (each incl. its trailing `\n`) until the closing-delimiter line.
        let mut body: Vec<u8> = Vec::new();
        let closing_indent;
        loop {
            let mark = self.pos;
            let mut indent = 0usize;
            while matches!(self.peek(), Some(b' ' | b'\t')) {
                self.bump();
                indent += 1;
            }
            if self.peek() == Some(b'"') && self.peek2() == Some(b'"') && self.peek3() == Some(b'"')
            {
                self.bump();
                self.bump();
                self.bump();
                closing_indent = indent;
                break;
            }
            if self.peek().is_none() {
                return Err(Diagnostic::new(
                    Stage::Lex,
                    "unterminated text block (no closing `\"\"\"`)",
                    line,
                    col,
                ));
            }
            body.extend_from_slice(&self.src[mark..self.pos]); // the indent we consumed
            loop {
                match self.bump() {
                    Some(b'\n') => {
                        body.push(b'\n');
                        break;
                    }
                    Some(c) => body.push(c),
                    None => {
                        return Err(Diagnostic::new(
                            Stage::Lex,
                            "unterminated text block (no closing `\"\"\"`)",
                            line,
                            col,
                        ))
                    }
                }
            }
        }
        // The newline separating the last content line from the closing delimiter is not content.
        if body.last() == Some(&b'\n') {
            body.pop();
        }
        let dedented = dedent_block(&body, closing_indent);
        // Escape bare `"` (a `\X` pair is copied verbatim) so the dedented body is safe to wrap in
        // `"…"` and route through `scan_string`, which owns all escape + interpolation handling.
        // Build the wrapped buffer as BYTES (a `u8 as char` cast would corrupt multi-byte UTF-8).
        let mut wrapped: Vec<u8> = Vec::with_capacity(dedented.len() + 2);
        wrapped.push(b'"');
        let mut k = 0;
        while k < dedented.len() {
            let c = dedented[k];
            if c == b'\\' && k + 1 < dedented.len() {
                wrapped.push(b'\\');
                wrapped.push(dedented[k + 1]);
                k += 2;
                continue;
            }
            if c == b'"' {
                wrapped.push(b'\\');
                wrapped.push(b'"');
                k += 1;
                continue;
            }
            wrapped.push(c);
            k += 1;
        }
        wrapped.push(b'"');
        let wrapped = String::from_utf8(wrapped).expect("text block body is valid UTF-8");
        let mut sub = Lexer::new(&wrapped);
        let tok = sub.scan_string(0, line, col).map_err(|e| {
            Diagnostic::new(
                Stage::Lex,
                format!("in text block: {}", e.message),
                line,
                col,
            )
        })?;
        let mut segs = match tok.kind {
            TokenKind::Str(s) => s,
            _ => unreachable!("scan_string yields a Str token"),
        };
        // Make interpolation offsets file-unique: the sub-lexer numbered them from 0 within the
        // wrapped body; shift by this block's source start (the block occupies a unique source range,
        // and the dedented body is no longer than the source, so `start + off` stays within it).
        for seg in &mut segs {
            if let StrSeg::Interp(_, off) = seg {
                *off += start;
            }
        }
        Ok(Token {
            kind: TokenKind::Str(segs),
            span: Span {
                start,
                len: self.pos - start,
                line,
                col,
            },
        })
    }

    /// Scan a raw string body (`r#*"` already consumed): copy bytes verbatim — no escapes, no
    /// interpolation — until a closing `"` followed by `hashes` `#`s. A `"` with the wrong number of
    /// trailing `#`s is a literal `"`, so any content (including `"#`) is expressible by choosing a
    /// longer `#`-run. Produces a single `StrSeg::Lit` (Phase 1 string slice).
    fn scan_raw_string(
        &mut self,
        start: usize,
        line: u32,
        col: u32,
        hashes: usize,
    ) -> Result<Token, Diagnostic> {
        let mut body: Vec<u8> = Vec::new();
        loop {
            match self.peek() {
                None => {
                    return Err(Diagnostic::new(
                        Stage::Lex,
                        "unterminated raw string",
                        line,
                        col,
                    ))
                }
                Some(b'"') => {
                    // A closing delimiter iff the `"` is followed by exactly `hashes` `#`s.
                    let closes = (0..hashes).all(|i| self.src.get(self.pos + 1 + i) == Some(&b'#'));
                    if closes {
                        self.bump(); // `"`
                        for _ in 0..hashes {
                            self.bump(); // each closing `#`
                        }
                        break;
                    }
                    body.push(b'"');
                    self.bump();
                }
                Some(c) => {
                    body.push(c);
                    self.bump();
                }
            }
        }
        let value = String::from_utf8(body).expect("raw string body is valid UTF-8");
        Ok(Token {
            kind: TokenKind::Str(vec![StrSeg::Lit(value)]),
            span: Span {
                start,
                len: self.pos - start,
                line,
                col,
            },
        })
    }

    /// Expand a `\u{HEX}` escape (the `\u` is already consumed): `{`, then 1–6 hex digits, then `}`,
    /// naming a Unicode codepoint whose UTF-8 bytes are appended to `bytes`. `(el, ec)` is the
    /// position of the opening backslash, for error reporting (Phase 1 string slice).
    fn scan_unicode_escape(
        &mut self,
        bytes: &mut Vec<u8>,
        el: u32,
        ec: u32,
    ) -> Result<(), Diagnostic> {
        if self.bump() != Some(b'{') {
            return Err(Diagnostic::new(
                Stage::Lex,
                "expected `{` after `\\u` (e.g. `\\u{1F600}`)",
                el,
                ec,
            ));
        }
        let mut hex = String::new();
        loop {
            match self.bump() {
                Some(b'}') => break,
                Some(c) if c.is_ascii_hexdigit() => hex.push(c as char),
                Some(c) => {
                    return Err(Diagnostic::new(
                        Stage::Lex,
                        format!("invalid hex digit `{}` in `\\u{{…}}`", c as char),
                        el,
                        ec,
                    ))
                }
                None => {
                    return Err(Diagnostic::new(
                        Stage::Lex,
                        "unterminated `\\u{…}` escape",
                        el,
                        ec,
                    ))
                }
            }
        }
        if hex.is_empty() || hex.len() > 6 {
            return Err(Diagnostic::new(
                Stage::Lex,
                "`\\u{…}` takes 1–6 hex digits",
                el,
                ec,
            ));
        }
        let cp = u32::from_str_radix(&hex, 16).expect("digits validated as hex above");
        match char::from_u32(cp) {
            Some(ch) => {
                let mut buf = [0u8; 4];
                bytes.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                Ok(())
            }
            None => Err(Diagnostic::new(
                Stage::Lex,
                format!("`\\u{{{hex}}}` is not a valid Unicode codepoint"),
                el,
                ec,
            )),
        }
    }

    /// Scan an `html"…"` literal (the `html` prefix is already consumed). The body is captured
    /// exactly like [`Self::scan_string`] — same escapes (`\n \t \r \\ \"`), multi-byte UTF-8 and
    /// raw newlines copied verbatim, so an `html"…"` literal spans lines for free — and `{`/`}` are
    /// preserved verbatim: the interpolation split *and* the desugar into `Core.Html` kernel calls
    /// happen in the parser/checker, not here. The only difference from `scan_string` is the token
    /// kind, which routes the body to the html desugarer instead of the plain-string one.
    fn scan_html(&mut self, start: usize, line: u32, col: u32) -> Result<Token, Diagnostic> {
        self.bump(); // opening quote
        let mut bytes: Vec<u8> = Vec::new();
        loop {
            let (el, ec) = (self.line, self.col);
            match self.bump() {
                None => {
                    return Err(Diagnostic::new(
                        Stage::Lex,
                        "unterminated html literal",
                        line,
                        col,
                    ))
                }
                Some(b'"') => break,
                Some(b'\\') => match self.bump() {
                    Some(b'n') => bytes.push(b'\n'),
                    Some(b't') => bytes.push(b'\t'),
                    Some(b'r') => bytes.push(b'\r'),
                    Some(b'\\') => bytes.push(b'\\'),
                    Some(b'"') => bytes.push(b'"'),
                    Some(other) => {
                        return Err(Diagnostic::new(
                            Stage::Lex,
                            format!("invalid escape \\{}", other as char),
                            el,
                            ec,
                        ))
                    }
                    None => {
                        return Err(Diagnostic::new(
                            Stage::Lex,
                            "unterminated html literal",
                            line,
                            col,
                        ))
                    }
                },
                Some(other) => bytes.push(other),
            }
        }
        let value = String::from_utf8(bytes).expect("source html body is valid UTF-8");
        Ok(Token {
            kind: TokenKind::Html(value),
            span: Span {
                start,
                len: self.pos - start,
                line,
                col,
            },
        })
    }

    /// Scan a `b"…"` byte-string literal (the `b` prefix is already consumed). Unlike `scan_string`
    /// there is NO interpolation — `{`/`}` are literal bytes. Escapes are `\n \t \r \\ \"` plus
    /// `\xHH` (two hex digits → one arbitrary octet), so a literal can hold non-UTF-8 Bytes.
    fn scan_bytes(&mut self, start: usize, line: u32, col: u32) -> Result<Token, Diagnostic> {
        self.bump(); // opening quote
        let mut bytes: Vec<u8> = Vec::new();
        loop {
            let (el, ec) = (self.line, self.col);
            match self.bump() {
                None => {
                    return Err(Diagnostic::new(
                        Stage::Lex,
                        "unterminated byte string",
                        line,
                        col,
                    ))
                }
                Some(b'"') => break,
                Some(b'\\') => match self.bump() {
                    Some(b'n') => bytes.push(b'\n'),
                    Some(b't') => bytes.push(b'\t'),
                    Some(b'r') => bytes.push(b'\r'),
                    Some(b'\\') => bytes.push(b'\\'),
                    Some(b'"') => bytes.push(b'"'),
                    Some(b'x') => {
                        let hi = self.hex_digit(el, ec)?;
                        let lo = self.hex_digit(el, ec)?;
                        bytes.push(hi << 4 | lo);
                    }
                    Some(other) => {
                        return Err(Diagnostic::new(
                            Stage::Lex,
                            format!("invalid escape \\{}", other as char),
                            el,
                            ec,
                        ))
                    }
                    None => {
                        return Err(Diagnostic::new(
                            Stage::Lex,
                            "unterminated byte string",
                            line,
                            col,
                        ))
                    }
                },
                Some(other) => bytes.push(other),
            }
        }
        Ok(Token {
            kind: TokenKind::Bytes(bytes),
            span: Span {
                start,
                len: self.pos - start,
                line,
                col,
            },
        })
    }

    /// Consume one hex digit for a `\xHH` byte escape, or error at the offending position.
    fn hex_digit(&mut self, el: u32, ec: u32) -> Result<u8, Diagnostic> {
        match self.bump() {
            Some(c) if c.is_ascii_hexdigit() => Ok((c as char).to_digit(16).unwrap() as u8),
            _ => Err(Diagnostic::new(
                Stage::Lex,
                "invalid \\xHH byte escape (expected two hex digits)",
                el,
                ec,
            )),
        }
    }

    // NOTE: identifiers are ASCII-only by design for v0.1 (scan_ident uses
    // is_ascii_alphabetic / is_ascii_alphanumeric). Unicode identifiers are out of scope.
    fn scan_ident(&mut self, start: usize, line: u32, col: u32) -> Token {
        while matches!(self.peek(), Some(b) if b == b'_' || b.is_ascii_alphanumeric()) {
            self.bump();
        }
        let text = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
        let kind = keyword(text).unwrap_or_else(|| TokenKind::Ident(text.to_string()));
        Token {
            kind,
            span: Span {
                start,
                len: self.pos - start,
                line,
                col,
            },
        }
    }

    /// Decode the full UTF-8 char beginning at the current position. The source is always
    /// valid UTF-8 (it came from `&str`), so a char boundary is guaranteed at `self.pos`.
    /// Used only on the error path so diagnostics show the real char, not a mojibake byte.
    fn current_char(&self) -> char {
        std::str::from_utf8(&self.src[self.pos..])
            .ok()
            .and_then(|s| s.chars().next())
            .unwrap_or(char::REPLACEMENT_CHARACTER)
    }
}

/// Parse the numeric TEXT of a `…d` decimal literal (the part before the `d`, e.g. `"1.50"`,
/// `"100"`, `"1_000.5"`) into `(unscaled, scale)` (M-NUM S1). Underscore separators are stripped (a
/// source literal may use them, unlike the runtime `Decimal.of` grammar). The scale is the count of
/// fractional digits, so trailing zeros are preserved. Returns `None` if the unscaled value overflows
/// `i128` (a compile-time error, not a runtime fault). No sign handling — the lexer scans the
/// magnitude; a leading `-` is the unary-minus operator on the literal.
fn parse_decimal_literal(text: &str) -> Option<(i128, u8)> {
    let cleaned: String = text.chars().filter(|c| *c != '_').collect();
    let (int_part, frac_part) = match cleaned.split_once('.') {
        Some((i, f)) => (i, f),
        None => (cleaned.as_str(), ""),
    };
    let scale = u8::try_from(frac_part.len()).ok()?;
    let combined = format!("{int_part}{frac_part}");
    let unscaled: i128 = combined.parse().ok()?;
    Some((unscaled, scale))
}

fn keyword(s: &str) -> Option<TokenKind> {
    use TokenKind::*;
    Some(match s {
        "function" => Function,
        "class" => Class,
        "enum" => Enum,
        "constructor" => Constructor,
        "trait" => Trait,
        "const" => Const,
        "open" => Open,
        "abstract" => Abstract,
        "public" => Public,
        "private" => Private,
        "protected" => Protected,
        "internal" => Internal,
        "return" => Return,
        "if" => If,
        "else" => Else,
        "for" => For,
        "while" => While,
        "do" => Do,
        "break" => Break,
        "continue" => Continue,
        "in" => In,
        "match" => Match,
        "import" => Import,
        "package" => Package,
        "this" => This,
        "true" => True,
        "false" => False,
        "null" => Null,
        "new" => New,
        "instanceof" => Instanceof,
        "interface" => Interface,
        "implements" => Implements,
        "extends" => Extends,
        // `var` is a CONTEXTUAL keyword (like `foreach`/`as`/`when`): it stays an ordinary identifier
        // in the token stream and is recognized as the inference-binding keyword only at a
        // declaration/binding start by the parser (`Parser::at_var_decl`). This frees `var` to be a
        // value / parameter / field name (it maps to a legal PHP `$var` / `->var`).
        "mutable" => Mutable,
        "static" => Static,
        "with" => With,
        "type" => TypeKw,
        "throw" => Throw,
        "try" => Try,
        "catch" => Catch,
        "finally" => Finally,
        "throws" => Throws,
        _ => return None,
    })
}

/// A-62: strip incidental indentation from a text-block body (raw lines joined by `\n`, no trailing
/// newline) per Java JEP-378: the common prefix is the minimal leading-whitespace over every
/// non-blank line **and** the closing delimiter's indentation; each line is then left-stripped by
/// that amount and right-stripped of trailing whitespace. Blank lines collapse to empty.
fn dedent_block(body: &[u8], closing_indent: usize) -> Vec<u8> {
    let is_ws = |c: u8| c == b' ' || c == b'\t';
    let lines: Vec<&[u8]> = body.split(|&c| c == b'\n').collect();
    let mut min = closing_indent;
    for ln in &lines {
        if ln.iter().all(|&c| is_ws(c)) {
            continue; // blank line — excluded from the minimum
        }
        let ind = ln.iter().take_while(|&&c| is_ws(c)).count();
        if ind < min {
            min = ind;
        }
    }
    let mut out = Vec::with_capacity(body.len());
    for (i, ln) in lines.iter().enumerate() {
        if i > 0 {
            out.push(b'\n');
        }
        let strip = ln.iter().take_while(|&&c| is_ws(c)).count().min(min);
        let line = &ln[strip..];
        // right-trim trailing whitespace (incl. a stray `\r` from CRLF input)
        let end = line
            .iter()
            .rposition(|&c| c != b' ' && c != b'\t' && c != b'\r')
            .map_or(0, |p| p + 1);
        out.extend_from_slice(&line[..end]);
    }
    out
}

/// Tokenize `src`. Comments are **discarded** (the parser never sees them). Use
/// [`lex_with_comments`] when a tool (the formatter `phg fmt`) needs the trivia.
pub fn lex(src: &str) -> Result<Vec<Token>, Diagnostic> {
    lex_inner(src, &mut Vec::new())
}

/// Like [`lex`], but also returns every comment captured in source order (the `phg fmt`
/// side-channel — F1). The token stream is identical to [`lex`]'s; comments are collected, not
/// emitted as tokens, so the parser/AST are unchanged.
pub fn lex_with_comments(src: &str) -> Result<(Vec<Token>, Vec<Comment>), Diagnostic> {
    let mut comments = Vec::new();
    let tokens = lex_inner(src, &mut comments)?;
    Ok((tokens, comments))
}

/// True when only whitespace precedes byte offset `start` on its source line (an "own-line"
/// comment) — i.e. everything back to the previous newline (or start of file) is blank.
fn at_line_start(src: &[u8], start: usize) -> bool {
    src[..start]
        .iter()
        .rev()
        .take_while(|&&b| b != b'\n')
        .all(|&b| b == b' ' || b == b'\t' || b == b'\r')
}

fn lex_inner(src: &str, comments: &mut Vec<Comment>) -> Result<Vec<Token>, Diagnostic> {
    let mut lx = Lexer::new(src);
    let mut out = Vec::new();
    loop {
        lx.skip_whitespace();
        let line = lx.line;
        let col = lx.col;
        let start = lx.pos;
        match lx.peek() {
            None => {
                out.push(Token {
                    kind: TokenKind::Eof,
                    span: Span {
                        start,
                        len: 0,
                        line,
                        col,
                    },
                });
                return Ok(out);
            }
            Some(b) => {
                if b == b'/' && lx.peek2() == Some(b'/') {
                    let own_line = at_line_start(lx.src, start);
                    lx.skip_line_comment();
                    // The raw `// …` text, trailing whitespace trimmed (the line comment stops at the
                    // newline, which is not consumed).
                    let text = String::from_utf8_lossy(&lx.src[start..lx.pos])
                        .trim_end()
                        .to_string();
                    comments.push(Comment {
                        span: Span {
                            start,
                            len: lx.pos - start,
                            line,
                            col,
                        },
                        text,
                        kind: CommentKind::Line,
                        own_line,
                    });
                    continue;
                }
                if b == b'/' && lx.peek2() == Some(b'*') {
                    let own_line = at_line_start(lx.src, start);
                    lx.skip_block_comment()?;
                    // `start..lx.pos` spans the whole `/* … */` including the closing delimiter.
                    let text = String::from_utf8_lossy(&lx.src[start..lx.pos]).to_string();
                    comments.push(Comment {
                        span: Span {
                            start,
                            len: lx.pos - start,
                            line,
                            col,
                        },
                        text,
                        kind: CommentKind::Block,
                        own_line,
                    });
                    continue;
                }

                // `html"…"` literal — must precede the identifier scan (a bare `html` is a valid
                // identifier, and the module qualifier in `html.text(…)`). Only the exact `html"`
                // sequence triggers it: `Html.` / `htmlx` / a bare `html` are ordinary idents.
                if b == b'h' && lx.src[lx.pos..].starts_with(b"html\"") {
                    for _ in 0..4 {
                        lx.bump(); // consume the `html` prefix
                    }
                    let t = lx.scan_html(start, line, col)?;
                    out.push(t);
                    continue;
                }

                // `r"…"` / `r#"…"#` raw string — literal bytes, NO escapes, NO interpolation (for
                // JSON, regex, templates). Rust-style `#`-run delimiter so embedded `"` is
                // expressible. Triggered only by `r` + zero-or-more `#` + `"`; a bare `r` / `rx` is
                // an ordinary identifier (must precede the identifier scan).
                if b == b'r' {
                    let after = &lx.src[lx.pos + 1..];
                    let hashes = after.iter().take_while(|&&c| c == b'#').count();
                    if after.get(hashes) == Some(&b'"') {
                        lx.bump(); // `r`
                        for _ in 0..hashes {
                            lx.bump(); // each `#`
                        }
                        lx.bump(); // opening `"`
                        let t = lx.scan_raw_string(start, line, col, hashes)?;
                        out.push(t);
                        continue;
                    }
                }

                // `b"…"` byte-string literal — must precede the identifier scan (a bare `b` is a
                // valid identifier start). Only the exact `b"` digraph triggers it.
                if b == b'b' && lx.peek2() == Some(b'"') {
                    lx.bump(); // consume the `b` prefix
                    let t = lx.scan_bytes(start, line, col)?;
                    out.push(t);
                    continue;
                }

                // A-62: `"""` opens a multi-line, auto-dedented **text block** (must precede the
                // single-`"` string check). The opening `"""` must be followed by a newline.
                if b == b'"' && lx.peek2() == Some(b'"') && lx.peek3() == Some(b'"') {
                    let t = lx.scan_text_block(start, line, col)?;
                    out.push(t);
                    continue;
                }
                if b == b'"' {
                    let t = lx.scan_string(start, line, col)?;
                    out.push(t);
                    continue;
                }

                if b.is_ascii_digit() {
                    let t = lx.scan_number(start, line, col)?;
                    out.push(t);
                    continue;
                }

                if b == b'_' || b.is_ascii_alphabetic() {
                    let t = lx.scan_ident(start, line, col);
                    out.push(t);
                    continue;
                }

                // Range operators: longest-match `..=` (3) and `..` (2) ahead of `.` (1). A number
                // like `0..3` already lexes `0` as `Int(0)` — `scan_number`'s float branch needs a
                // *digit* after the dot, and here the next char is another `.`.
                if b == b'.' && lx.peek2() == Some(b'.') {
                    let (kind, len) = if lx.peek3() == Some(b'=') {
                        (TokenKind::DotDotEq, 3)
                    } else {
                        (TokenKind::DotDot, 2)
                    };
                    for _ in 0..len {
                        lx.bump();
                    }
                    out.push(Token {
                        kind,
                        span: Span {
                            start,
                            len,
                            line,
                            col,
                        },
                    });
                    continue;
                }

                // `??=` (3) null-coalesce-assign — longest-match ahead of the two-char `??`,
                // mirroring the `..=`/`..` range block above (M-mut.2).
                if b == b'?' && lx.peek2() == Some(b'?') && lx.peek3() == Some(b'=') {
                    for _ in 0..3 {
                        lx.bump();
                    }
                    out.push(Token {
                        kind: TokenKind::QuestionQuestionEq,
                        span: Span {
                            start,
                            len: 3,
                            line,
                            col,
                        },
                    });
                    continue;
                }

                // two-char operators take priority
                let two = |k: TokenKind| Token {
                    kind: k,
                    span: Span {
                        start,
                        len: 2,
                        line,
                        col,
                    },
                };
                let p2 = lx.peek2();
                let matched_two = match (b, p2) {
                    (b'=', Some(b'=')) => Some(TokenKind::EqEq),
                    (b'!', Some(b'=')) => Some(TokenKind::NotEq),
                    (b'<', Some(b'=')) => Some(TokenKind::Le),
                    (b'>', Some(b'=')) => Some(TokenKind::Ge),
                    (b'-', Some(b'>')) => Some(TokenKind::Arrow),
                    (b'=', Some(b'>')) => Some(TokenKind::FatArrow),
                    (b'|', Some(b'>')) => Some(TokenKind::Pipe),
                    (b'&', Some(b'&')) => Some(TokenKind::AndAnd),
                    (b'|', Some(b'|')) => Some(TokenKind::OrOr),
                    (b'?', Some(b'?')) => Some(TokenKind::QuestionQuestion),
                    (b'?', Some(b'.')) => Some(TokenKind::QuestionDot),
                    // compound-assign + increment/decrement (M-mut.2). `-=`/`--`/`->` and
                    // `/=` (not a `//`/`/*` comment, handled earlier) all reach here distinctly.
                    (b'+', Some(b'=')) => Some(TokenKind::PlusEq),
                    (b'-', Some(b'=')) => Some(TokenKind::MinusEq),
                    (b'*', Some(b'=')) => Some(TokenKind::StarEq),
                    // `**` power operator (Phase 1 operators slice). Distinct p2 from `*=`, so order
                    // is irrelevant; `**=` (power-assign) is out of scope and lexes as `StarStar` `Eq`.
                    (b'*', Some(b'*')) => Some(TokenKind::StarStar),
                    (b'/', Some(b'=')) => Some(TokenKind::SlashEq),
                    (b'%', Some(b'=')) => Some(TokenKind::PercentEq),
                    (b'+', Some(b'+')) => Some(TokenKind::PlusPlus),
                    (b'-', Some(b'-')) => Some(TokenKind::MinusMinus),
                    // `<<` bitwise shift-left (primitives P2). `<=` is claimed above; a bare `<` falls
                    // through to the single-char dispatch. There is deliberately no `>>` token (it
                    // would break nested generics) — shift-right is two `Gt` handled in the parser.
                    (b'<', Some(b'<')) => Some(TokenKind::Shl),
                    // `#[` opens a PHP-8-style attribute group (M6 W2). A bare `#` has no other use
                    // (raw strings `r#"…"#` are lexed in the string path above), so this is the only
                    // place `#` is accepted; a lone `#` falls through to the unexpected-char error.
                    (b'#', Some(b'[')) => Some(TokenKind::HashBracket),
                    _ => None,
                };
                if let Some(k) = matched_two {
                    lx.bump();
                    lx.bump();
                    out.push(two(k));
                    continue;
                }

                let single = |k: TokenKind| Token {
                    kind: k,
                    span: Span {
                        start,
                        len: 1,
                        line,
                        col,
                    },
                };
                let kind = match b {
                    b'.' => Some(TokenKind::Dot),
                    b';' => Some(TokenKind::Semicolon),
                    b',' => Some(TokenKind::Comma),
                    b':' => Some(TokenKind::Colon),
                    b'?' => Some(TokenKind::Question),
                    b'(' => Some(TokenKind::LParen),
                    b')' => Some(TokenKind::RParen),
                    b'{' => Some(TokenKind::LBrace),
                    b'}' => Some(TokenKind::RBrace),
                    b'[' => Some(TokenKind::LBracket),
                    b']' => Some(TokenKind::RBracket),
                    b'+' => Some(TokenKind::Plus),
                    b'-' => Some(TokenKind::Minus),
                    b'*' => Some(TokenKind::Star),
                    b'/' => Some(TokenKind::Slash),
                    b'%' => Some(TokenKind::Percent),
                    b'<' => Some(TokenKind::Lt),
                    b'>' => Some(TokenKind::Gt),
                    b'=' => Some(TokenKind::Eq),
                    b'!' => Some(TokenKind::Bang),
                    // A lone `|` is the union-type separator (`A | B`, M-RT S4). `|>` and `||` are
                    // claimed by the two-char dispatch above, so reaching here means a single `|`.
                    b'|' => Some(TokenKind::Bar),
                    // A lone `&` is the intersection-type separator (`A & B`, M-RT S5) or bitwise-AND
                    // in expression position. `&&` is claimed by the two-char dispatch above.
                    b'&' => Some(TokenKind::Amp),
                    // `^` bitwise XOR, `~` unary bitwise NOT (primitives P2; expression-only).
                    b'^' => Some(TokenKind::Caret),
                    b'~' => Some(TokenKind::Tilde),
                    _ => None,
                };
                match kind {
                    Some(k) => {
                        lx.bump();
                        out.push(single(k));
                    }
                    None => {
                        // Decode the full char (handles multi-byte UTF-8) for the message.
                        let ch = lx.current_char();
                        return Err(Diagnostic::new(
                            Stage::Lex,
                            format!("unexpected character {ch:?}"),
                            line,
                            col,
                        ));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
