use super::*;
use crate::token::TokenKind;

fn kinds(src: &str) -> Vec<TokenKind> {
    lex(src).unwrap().into_iter().map(|t| t.kind).collect()
}

#[test]
fn empty_and_whitespace_yield_eof_only() {
    assert_eq!(kinds(""), vec![TokenKind::Eof]);
    assert_eq!(kinds("   \n\t \r\n"), vec![TokenKind::Eof]);
}

#[test]
fn span_tracks_across_newlines() {
    // "a\nbb": ident "a" on line 1, ident "bb" on line 2 at col 1.
    let toks = lex("a\nbb").unwrap();
    // toks[0] = a, toks[1] = bb, toks[2] = Eof
    assert_eq!(toks[0].span.line, 1);
    assert_eq!(toks[0].span.col, 1);
    assert_eq!(toks[0].span.start, 0);
    assert_eq!(toks[0].span.len, 1);

    assert_eq!(toks[1].span.line, 2);
    assert_eq!(toks[1].span.col, 1);
    assert_eq!(toks[1].span.start, 2); // byte 0='a', 1='\n', 2='b'
    assert_eq!(toks[1].span.len, 2);
}

#[test]
fn single_char_tokens() {
    use TokenKind::*;
    assert_eq!(
        kinds(". ; , : ? ( ) { } [ ] < > = ! + - * / %"),
        vec![
            Dot, Semicolon, Comma, Colon, Question, LParen, RParen, LBrace, RBrace, LBracket,
            RBracket, Lt, Gt, Eq, Bang, Plus, Minus, Star, Slash, Percent, Eof
        ]
    );
}

#[test]
fn multi_char_operators() {
    use TokenKind::*;
    assert_eq!(
        kinds("== != <= >= -> => |> && ||"),
        vec![EqEq, NotEq, Le, Ge, Arrow, FatArrow, Pipe, AndAnd, OrOr, Eof]
    );
}

#[test]
fn compound_assign_and_incdec_operators() {
    use TokenKind::*;
    // M-mut.2: the five `op=`, `??=`, and `++`/`--`.
    assert_eq!(
        kinds("+= -= *= /= %= ??= ++ --"),
        vec![
            PlusEq,
            MinusEq,
            StarEq,
            SlashEq,
            PercentEq,
            QuestionQuestionEq,
            PlusPlus,
            MinusMinus,
            Eof
        ]
    );
    // `??=` (3) is longest-match ahead of `??` (2): `??` alone still lexes as QuestionQuestion.
    assert_eq!(kinds("??"), vec![QuestionQuestion, Eof]);
    // `-=` / `--` / `->` coexist (distinct second byte).
    assert_eq!(kinds("-> -- -="), vec![Arrow, MinusMinus, MinusEq, Eof]);
    // `/=` is not a comment start (`//`, `/*`): it lexes as SlashEq.
    assert_eq!(
        kinds("a /= 2"),
        vec![Ident("a".into()), SlashEq, Int(2), Eof]
    );
}

#[test]
fn bitwise_operator_tokens() {
    use TokenKind::*;
    // `<<` is a two-char token; `^`/`~` are single-char; bare `&`/`|` are Amp/Bar (shared with
    // intersection/union types). There is no `>>` token — it is two `Gt` (protects nested generics).
    assert_eq!(
        kinds("& | ^ ~ << >>"),
        vec![Amp, Bar, Caret, Tilde, Shl, Gt, Gt, Eof]
    );
}

#[test]
fn range_operators_lex_longest_match() {
    use TokenKind::*;
    // `..=` (3) beats `..` (2) beats `.` (1); `0` stays an Int (no digit after the dot).
    assert_eq!(kinds("0..3"), vec![Int(0), DotDot, Int(3), Eof]);
    assert_eq!(kinds("0..=3"), vec![Int(0), DotDotEq, Int(3), Eof]);
    // a lone `.` is still a member-access Dot — `..` handling doesn't swallow it
    assert_eq!(
        kinds("a.b"),
        vec![Ident("a".into()), Dot, Ident("b".into()), Eof]
    );
}

#[test]
fn number_literals() {
    use TokenKind::*;
    assert_eq!(kinds("0 42 1000"), vec![Int(0), Int(42), Int(1000), Eof]);
    assert_eq!(kinds("3.5 0.5"), vec![Float(3.5), Float(0.5), Eof]);
}

#[test]
fn leading_zero_int_collapses() {
    // M1: leading zeros are absorbed by i64 parsing — `007` lexes to Int(7).
    assert_eq!(kinds("007"), vec![TokenKind::Int(7), TokenKind::Eof]);
}

#[test]
fn number_literal_formats() {
    // Base prefixes (Rust-style; a leading `0` is NOT octal — that PHP footgun is dropped).
    assert_eq!(kinds("0xFF"), vec![TokenKind::Int(255), TokenKind::Eof]);
    assert_eq!(kinds("0xff"), vec![TokenKind::Int(255), TokenKind::Eof]);
    assert_eq!(kinds("0b1010"), vec![TokenKind::Int(10), TokenKind::Eof]);
    assert_eq!(kinds("0o17"), vec![TokenKind::Int(15), TokenKind::Eof]);
    // Underscore digit separators (int and float).
    assert_eq!(
        kinds("1_000_000"),
        vec![TokenKind::Int(1_000_000), TokenKind::Eof]
    );
    assert_eq!(
        kinds("1_000.500_5"),
        vec![TokenKind::Float(1000.5005), TokenKind::Eof]
    );
    // Scientific notation → float.
    assert_eq!(kinds("1e3"), vec![TokenKind::Float(1000.0), TokenKind::Eof]);
    assert_eq!(
        kinds("2.5e-2"),
        vec![TokenKind::Float(0.025), TokenKind::Eof]
    );
    // `e` not followed by a (signed) digit is not an exponent: `3em` = Int(3) then ident `em`.
    assert_eq!(
        kinds("3em"),
        vec![
            TokenKind::Int(3),
            TokenKind::Ident("em".into()),
            TokenKind::Eof
        ]
    );
}

#[test]
fn integer_overflow_is_error_not_panic() {
    // 26-digit literal exceeds i64::MAX; must yield Diagnostic, never panic.
    let err = lex("99999999999999999999999999").unwrap_err();
    assert!(err.message.contains("out of range"), "got: {}", err.message);
    assert_eq!(err.line, 1);
    assert_eq!(err.col, 1);
}

#[test]
fn float_overflow_is_error_not_panic() {
    // A literal whose integer part alone exceeds f64::MAX (~1.8e308) overflows to inf, which the
    // lexer rejects as out-of-range (rather than panicking or yielding a non-finite value).
    let huge = format!("{}.0", "9".repeat(320));
    let err = lex(&huge).unwrap_err();
    assert!(err.message.contains("out of range"), "got: {}", err.message);
}

#[test]
fn identifiers_and_keywords() {
    use TokenKind::*;
    assert_eq!(
        kinds("function class enum constructor return match this true false null"),
        vec![
            Function,
            Class,
            Enum,
            Constructor,
            Return,
            Match,
            This,
            True,
            False,
            Null,
            Eof
        ]
    );
    assert_eq!(
        kinds("age myVar User _x"),
        vec![
            Ident("age".into()),
            Ident("myVar".into()),
            Ident("User".into()),
            Ident("_x".into()),
            Eof
        ]
    );
}

#[test]
fn instanceof_keyword_is_recognized() {
    use TokenKind::*;
    // `instanceof` is the type-test keyword (M-RT S1); the retired `is` is now a plain ident.
    assert_eq!(kinds("instanceof"), vec![Instanceof, Eof]);
    assert_eq!(kinds("is"), vec![Ident("is".into()), Eof]);
    // still an ident when part of a longer word
    assert_eq!(kinds("island"), vec![Ident("island".into()), Eof]);
}

#[test]
fn fn_is_a_reserved_keyword() {
    use TokenKind::*;
    assert_eq!(kinds("fn"), vec![Fn, Eof]);
    // contextual sanity: `fn (` still lexes as the keyword then a paren
    assert_eq!(kinds("fn ("), vec![Fn, LParen, Eof]);
}

#[test]
fn interface_keywords_are_recognized() {
    use TokenKind::*;
    // M-RT S2 keywords: `interface`, `implements`, `extends`.
    assert_eq!(
        kinds("interface implements extends"),
        vec![Interface, Implements, Extends, Eof]
    );
    // still idents when embedded in a longer word
    assert_eq!(kinds("interfaces"), vec![Ident("interfaces".into()), Eof]);
}

#[test]
fn with_keyword_is_recognized() {
    use TokenKind::*;
    // M-mut.4a `clone with` operator keyword.
    assert_eq!(kinds("with"), vec![With, Eof]);
    // still an ident embedded in a longer word.
    assert_eq!(kinds("within"), vec![Ident("within".into()), Eof]);
    assert_eq!(kinds("withdraw"), vec![Ident("withdraw".into()), Eof]);
}

#[test]
fn loop_keywords_are_recognized() {
    use TokenKind::*;
    // M-mut.3 condition-loop keywords.
    assert_eq!(
        kinds("while do break continue"),
        vec![While, Do, Break, Continue, Eof]
    );
    // still idents when embedded in a longer word
    assert_eq!(kinds("breakfast"), vec![Ident("breakfast".into()), Eof]);
    assert_eq!(kinds("doer"), vec![Ident("doer".into()), Eof]);
}

#[test]
fn mutable_keyword_is_recognized() {
    use TokenKind::*;
    // M-mut.1: `mutable` binding modifier.
    assert_eq!(
        kinds("mutable int x"),
        vec![Mutable, Ident("int".into()), Ident("x".into()), Eof]
    );
    // still an ident when embedded in a longer word
    assert_eq!(kinds("mutableness"), vec![Ident("mutableness".into()), Eof]);
}

#[test]
fn string_literals() {
    use TokenKind::*;
    assert_eq!(kinds("\"hello\""), vec![Str("hello".into()), Eof]);
    // escapes
    assert_eq!(
        kinds("\"a\\nb\\t\\\"c\""),
        vec![Str("a\nb\t\"c".into()), Eof]
    );
    // interpolation body preserved verbatim (split happens in the parser)
    assert_eq!(
        kinds("\"Hello {name}\""),
        vec![Str("Hello {name}".into()), Eof]
    );
}

#[test]
fn utf8_string_body_preserved() {
    use TokenKind::*;
    assert_eq!(kinds("\"café\""), vec![Str("café".into()), Eof]);
    assert_eq!(kinds("\"a 🎉 b\""), vec![Str("a 🎉 b".into()), Eof]);
}

#[test]
fn unterminated_string_errors() {
    let err = lex("\"oops").unwrap_err();
    assert!(err.message.contains("unterminated string"));
}

#[test]
fn byte_string_literals() {
    use TokenKind::*;
    assert_eq!(kinds("b\"Hi\""), vec![Bytes(vec![b'H', b'i']), Eof]);
    // \xHH escapes to arbitrary octets (incl. non-UTF-8).
    assert_eq!(
        kinds("b\"\\x48\\xff\\x00\""),
        vec![Bytes(vec![0x48, 0xff, 0x00]), Eof]
    );
    // ordinary escapes still work.
    assert_eq!(
        kinds("b\"a\\nb\""),
        vec![Bytes(vec![b'a', b'\n', b'b']), Eof]
    );
    // NO interpolation — braces are literal bytes.
    assert_eq!(
        kinds("b\"x{y}\""),
        vec![Bytes(vec![b'x', b'{', b'y', b'}']), Eof]
    );
    // a bare `b` is still an identifier; only `b"` triggers a byte literal.
    assert_eq!(kinds("b"), vec![Ident("b".into()), Eof]);
}

#[test]
fn html_literals() {
    use TokenKind::*;
    assert_eq!(
        kinds("html\"<h1>Hi</h1>\""),
        vec![Html("<h1>Hi</h1>".into()), Eof]
    );
    // interpolation body preserved verbatim (split + desugar happen later).
    assert_eq!(
        kinds("html\"<h1>{name}</h1>\""),
        vec![Html("<h1>{name}</h1>".into()), Eof]
    );
    // ordinary escapes work, including `\"` for an attribute quote.
    assert_eq!(
        kinds("html\"<a href=\\\"x\\\">a\\nb</a>\""),
        vec![Html("<a href=\"x\">a\nb</a>".into()), Eof]
    );
    // multi-line for free (raw newline copied verbatim, like a plain string).
    assert_eq!(
        kinds("html\"<ul>\n  <li>x</li>\n</ul>\""),
        vec![Html("<ul>\n  <li>x</li>\n</ul>".into()), Eof]
    );
    // only the exact `html"` sequence triggers it: a bare `html`, `Html.`, `htmlx` are idents.
    assert_eq!(kinds("html"), vec![Ident("html".into()), Eof]);
    assert_eq!(
        kinds("html.text"),
        vec![Ident("html".into()), Dot, Ident("text".into()), Eof]
    );
    assert_eq!(kinds("htmlx"), vec![Ident("htmlx".into()), Eof]);
}

#[test]
fn html_literal_errors() {
    assert!(lex("html\"oops")
        .unwrap_err()
        .message
        .contains("unterminated html literal"));
    assert!(lex("html\"\\q\"")
        .unwrap_err()
        .message
        .contains("invalid escape"));
}

#[test]
fn byte_string_errors() {
    assert!(lex("b\"oops")
        .unwrap_err()
        .message
        .contains("unterminated byte string"));
    assert!(lex("b\"\\xZZ\"").unwrap_err().message.contains("\\xHH"));
    assert!(lex("b\"\\q\"")
        .unwrap_err()
        .message
        .contains("invalid escape"));
}

#[test]
fn error_positions_are_accurate() {
    // unterminated string: points at the opening quote
    let e = lex("\"oops").unwrap_err();
    assert!(e.message.contains("unterminated string"));
    assert_eq!((e.line, e.col), (1, 1));

    // invalid escape: points at the offending backslash, not one past it
    let e = lex("\"ab\\q\"").unwrap_err();
    assert!(e.message.contains("invalid escape"));
    assert_eq!((e.line, e.col), (1, 4)); // " a b \  -> backslash at col 4

    // invalid escape on a later line reports the right line/col
    let e = lex("\"x\ny\\q\"").unwrap_err();
    assert!(e.message.contains("invalid escape"));
    assert_eq!((e.line, e.col), (2, 2)); // line 2: y(\)q  -> backslash at col 2

    // unterminated block comment: points at the comment start
    let e = lex("/* never ends").unwrap_err();
    assert!(e.message.contains("unterminated block comment"));
    assert_eq!((e.line, e.col), (1, 1));

    // unexpected char
    let e = lex("  @").unwrap_err();
    assert!(e.message.contains("unexpected character"));
    assert_eq!((e.line, e.col), (1, 3));
}

#[test]
fn non_ascii_outside_string_reports_decoded_char() {
    // Identifiers are ASCII-only by design (v0.1), so a stray non-ASCII char is an
    // error — but the message must show the real char, not a mojibake lead byte.
    let e = lex("é").unwrap_err();
    assert!(e.message.contains("unexpected character"));
    assert!(e.message.contains('é'), "got: {}", e.message);
    assert_eq!((e.line, e.col), (1, 1));

    // Column must count one per char, not per byte: after the 2-byte "é",
    // the '@' is at column 2.
    let e = lex("é@").unwrap_err();
    assert!(e.message.contains('é'), "got: {}", e.message);
    assert_eq!((e.line, e.col), (1, 1));
}

#[test]
fn comments_are_skipped() {
    use TokenKind::*;
    assert_eq!(kinds("1 // line comment\n2"), vec![Int(1), Int(2), Eof]);
    assert_eq!(kinds("1 /* block\ncomment */ 2"), vec![Int(1), Int(2), Eof]);
}
