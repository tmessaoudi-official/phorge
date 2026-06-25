//! M-Lift L1 — a std-only PHP lexer for the **Tier-1** token set (typed function/class/enum/`match`
//! and ordinary control flow + expressions). Not a full PHP lexer: it covers the constructs the
//! demo-angle lifter handles and treats anything outside Tier-1 as a lex error rather than guessing.
//!
//! Output is a flat `Vec<PTokenSpanned>` ending in [`PTok::Eof`]; the L2 parser distinguishes
//! keywords from identifiers (both arrive as [`PTok::Ident`]) and consumes the stream.

/// A PHP token (Tier-1 subset). Keywords are not pre-classified — they arrive as [`PTok::Ident`]
/// and the parser matches the string, mirroring how Phorge's own lexer hands `match`/`if`/… to its
/// parser. Variables carry their name **without** the leading `$`.
#[derive(Debug, Clone, PartialEq)]
pub enum PTok {
    /// `<?php` (the only open tag this tier accepts; short tags are out of scope).
    OpenTag,
    /// `?>` close tag (rare in pure-PHP files; tolerated).
    CloseTag,
    /// A bare identifier: a keyword (`function`, `class`, `return`, `int`, …) or a name. The parser
    /// decides which by string.
    Ident(String),
    /// `$name` — a variable, stored without the `$`.
    Var(String),
    Int(i64),
    Float(f64),
    /// A string literal's **decoded** contents (escapes resolved). Emitted for single-quoted strings
    /// and for double-quoted strings that contain **no** interpolation — i.e. a safe literal.
    Str(String),
    /// A double-quoted string that DOES interpolate (`"hi $name"`, `"{$x}"`). Carries the **raw,
    /// undecoded** inner text. The L2 parser rejects it loudly as Tier-2 rather than silently lifting
    /// `$name` as literal text — honoring the never-guess contract. (Tier-2 will parse the raw form.)
    InterpStr(String),
    // ── punctuation ──
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semi,
    Colon,
    /// `::` (static/`const` access — `Limits::MAX`).
    DoubleColon,
    /// `->` (instance member).
    Arrow,
    /// `?->` (null-safe member).
    NullArrow,
    /// `=>` (array/`match` arm).
    FatArrow,
    Question,
    /// `??` null-coalesce.
    Coalesce,
    // ── operators ──
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Dot,
    /// `++` increment.
    Inc,
    /// `--` decrement.
    Dec,
    /// Compound assignments `+= -= *= /= %= .= ??=` (Tier-1: Phorge supports these natively, so they
    /// round-trip). The variant names mirror the operator.
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    DotEq,
    CoalesceEq,
    Assign,
    EqEq,
    EqEqEq,
    NotEq,
    NotEqEq,
    Lt,
    Gt,
    Le,
    Ge,
    AndAnd,
    OrOr,
    Not,
    /// End of input.
    Eof,
}

/// A token plus its 1-based source line (for lift diagnostics / `// lifted (verify)` placement).
#[derive(Debug, Clone, PartialEq)]
pub struct PTokenSpanned {
    pub tok: PTok,
    pub line: usize,
}

/// Lex PHP source into Tier-1 tokens. Returns `lift lex error: …` (with a line) on a character or
/// construct outside the supported set, so the lifter can report rather than silently misread.
pub fn lex_php(src: &str) -> Result<Vec<PTokenSpanned>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0usize;
    let mut line = 1usize;
    let mut out: Vec<PTokenSpanned> = Vec::new();

    let push = |out: &mut Vec<PTokenSpanned>, tok: PTok, line: usize| {
        out.push(PTokenSpanned { tok, line });
    };

    while i < chars.len() {
        let c = chars[i];
        // Whitespace (track newlines).
        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        // Open/close tags.
        if c == '<' && chars[i..].starts_with(&['<', '?', 'p', 'h', 'p']) {
            push(&mut out, PTok::OpenTag, line);
            i += 5;
            continue;
        }
        if c == '?' && i + 1 < chars.len() && chars[i + 1] == '>' {
            push(&mut out, PTok::CloseTag, line);
            i += 2;
            continue;
        }
        // Comments: `//`, `#` (line) and `/* … */` (block).
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '#' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                if chars[i] == '\n' {
                    line += 1;
                }
                i += 1;
            }
            if i + 1 >= chars.len() {
                return Err(format!(
                    "lift lex error: unterminated block comment (line {line})"
                ));
            }
            i += 2; // consume `*/`
            continue;
        }
        // Variable `$name`.
        if c == '$' {
            i += 1;
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            if i == start {
                return Err(format!(
                    "lift lex error: `$` not followed by a name (line {line})"
                ));
            }
            let name: String = chars[start..i].iter().collect();
            push(&mut out, PTok::Var(name), line);
            continue;
        }
        // Identifier / keyword (a leading letter or `_`).
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            push(&mut out, PTok::Ident(word), line);
            continue;
        }
        // Number (int or float). No leading-sign handling — `-` is a separate operator token.
        if c.is_ascii_digit() {
            let start = i;
            let mut is_float = false;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            if i < chars.len()
                && chars[i] == '.'
                && i + 1 < chars.len()
                && chars[i + 1].is_ascii_digit()
            {
                is_float = true;
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let text: String = chars[start..i].iter().collect();
            if is_float {
                let v: f64 = text
                    .parse()
                    .map_err(|_| format!("lift lex error: bad float `{text}` (line {line})"))?;
                push(&mut out, PTok::Float(v), line);
            } else {
                let v: i64 = text
                    .parse()
                    .map_err(|_| format!("lift lex error: bad int `{text}` (line {line})"))?;
                push(&mut out, PTok::Int(v), line);
            }
            continue;
        }
        // String literal — single or double quote. Tier-1 decodes basic escapes; a `$` inside a
        // double-quoted string (interpolation) is kept literal here and the *parser* decides whether
        // the construct is liftable (interpolation is Tier-2).
        if c == '"' || c == '\'' {
            let quote = c;
            i += 1;
            let raw_start = i;
            let mut s = String::new();
            // Interpolation flag: only a double-quoted string interpolates, and only when a `$` is
            // followed by a variable-name start (`[A-Za-z_]`) or a complex form (`{$…}`/`${…}`). A
            // lone `$5`/`$ ` is literal in PHP, so it does NOT flag (avoids false-positive rejection).
            let mut interpolated = false;
            while i < chars.len() && chars[i] != quote {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    let e = chars[i + 1];
                    let decoded = match e {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        '\\' => '\\',
                        '"' => '"',
                        '\'' => '\'',
                        '0' => '\0',
                        // Unknown escape: keep the backslash literally (PHP single-quote semantics).
                        _ => {
                            s.push('\\');
                            s.push(e);
                            i += 2;
                            continue;
                        }
                    };
                    s.push(decoded);
                    i += 2;
                    continue;
                }
                if quote == '"' {
                    let next = chars.get(i + 1).copied();
                    let dollar_var = chars[i] == '$'
                        && next.is_some_and(|n| n.is_alphabetic() || n == '_' || n == '{');
                    let complex = chars[i] == '{' && next == Some('$');
                    if dollar_var || complex {
                        interpolated = true;
                    }
                }
                if chars[i] == '\n' {
                    line += 1;
                }
                s.push(chars[i]);
                i += 1;
            }
            if i >= chars.len() {
                return Err(format!("lift lex error: unterminated string (line {line})"));
            }
            let raw: String = chars[raw_start..i].iter().collect();
            i += 1; // consume closing quote
            if interpolated {
                push(&mut out, PTok::InterpStr(raw), line);
            } else {
                push(&mut out, PTok::Str(s), line);
            }
            continue;
        }
        // Multi-char then single-char operators/punctuation. Longest match first.
        let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
        let three: String = chars[i..(i + 3).min(chars.len())].iter().collect();
        if three == "===" {
            push(&mut out, PTok::EqEqEq, line);
            i += 3;
            continue;
        }
        if three == "!==" {
            push(&mut out, PTok::NotEqEq, line);
            i += 3;
            continue;
        }
        if three == "?->" {
            push(&mut out, PTok::NullArrow, line);
            i += 3;
            continue;
        }
        if three == "??=" {
            push(&mut out, PTok::CoalesceEq, line);
            i += 3;
            continue;
        }
        let two_tok = match two.as_str() {
            "==" => Some(PTok::EqEq),
            "!=" => Some(PTok::NotEq),
            "<=" => Some(PTok::Le),
            ">=" => Some(PTok::Ge),
            "&&" => Some(PTok::AndAnd),
            "||" => Some(PTok::OrOr),
            "->" => Some(PTok::Arrow),
            "=>" => Some(PTok::FatArrow),
            "::" => Some(PTok::DoubleColon),
            "??" => Some(PTok::Coalesce),
            "++" => Some(PTok::Inc),
            "--" => Some(PTok::Dec),
            "+=" => Some(PTok::PlusEq),
            "-=" => Some(PTok::MinusEq),
            "*=" => Some(PTok::StarEq),
            "/=" => Some(PTok::SlashEq),
            "%=" => Some(PTok::PercentEq),
            ".=" => Some(PTok::DotEq),
            _ => None,
        };
        if let Some(t) = two_tok {
            push(&mut out, t, line);
            i += 2;
            continue;
        }
        let one_tok = match c {
            '(' => PTok::LParen,
            ')' => PTok::RParen,
            '{' => PTok::LBrace,
            '}' => PTok::RBrace,
            '[' => PTok::LBracket,
            ']' => PTok::RBracket,
            ',' => PTok::Comma,
            ';' => PTok::Semi,
            ':' => PTok::Colon,
            '?' => PTok::Question,
            '+' => PTok::Plus,
            '-' => PTok::Minus,
            '*' => PTok::Star,
            '/' => PTok::Slash,
            '%' => PTok::Percent,
            '.' => PTok::Dot,
            '=' => PTok::Assign,
            '<' => PTok::Lt,
            '>' => PTok::Gt,
            '!' => PTok::Not,
            _ => {
                return Err(format!(
                    "lift lex error: unexpected character `{c}` (line {line})"
                ))
            }
        };
        push(&mut out, one_tok, line);
        i += 1;
    }
    push(&mut out, PTok::Eof, line);
    Ok(out)
}
