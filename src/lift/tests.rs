//! M-Lift L1 — PHP lexer tests.

use super::lexer::{lex_php, PTok};

fn toks(src: &str) -> Vec<PTok> {
    lex_php(src)
        .expect("lex")
        .into_iter()
        .map(|t| t.tok)
        .collect()
}

#[test]
fn lexes_typed_function() {
    let t = toks("<?php\nfunction add(int $a, int $b): int {\n  return $a + $b;\n}\n");
    use PTok::*;
    assert_eq!(
        t,
        vec![
            OpenTag,
            Ident("function".into()),
            Ident("add".into()),
            LParen,
            Ident("int".into()),
            Var("a".into()),
            Comma,
            Ident("int".into()),
            Var("b".into()),
            RParen,
            Colon,
            Ident("int".into()),
            LBrace,
            Ident("return".into()),
            Var("a".into()),
            Plus,
            Var("b".into()),
            Semi,
            RBrace,
            Eof,
        ]
    );
}

#[test]
fn lexes_literals_and_strings() {
    let t = toks(r#"<?php $x = 42; $y = 3.5; $s = "hi\n"; $z = 'raw';"#);
    use PTok::*;
    assert!(t.contains(&Int(42)));
    assert!(t.contains(&Float(3.5)));
    assert!(t.contains(&Str("hi\n".into())));
    assert!(t.contains(&Str("raw".into())));
}

#[test]
fn lexes_operators_and_member_access() {
    let t = toks("<?php $a === $b !== $c && $d || !$e ?? $f ?-> g :: H -> i => j");
    use PTok::*;
    for want in [
        EqEqEq,
        NotEqEq,
        AndAnd,
        OrOr,
        Not,
        Coalesce,
        NullArrow,
        DoubleColon,
        Arrow,
        FatArrow,
    ] {
        assert!(t.contains(&want), "missing {want:?} in {t:?}");
    }
}

#[test]
fn skips_all_comment_forms() {
    let t = toks("<?php // line\n# hash\n/* block\n spanning */ $kept = 1;");
    use PTok::*;
    assert_eq!(
        t,
        vec![OpenTag, Var("kept".into()), Assign, Int(1), Semi, Eof]
    );
}

#[test]
fn tracks_line_numbers() {
    let spanned = lex_php("<?php\n\n$x = 1;").expect("lex");
    let var = spanned
        .iter()
        .find(|t| matches!(t.tok, PTok::Var(_)))
        .expect("var token");
    assert_eq!(var.line, 3, "$x is on line 3");
}

#[test]
fn rejects_unsupported_character() {
    // A backtick (PHP shell-exec) is outside Tier-1 — lex error, not a silent skip.
    let err = lex_php("<?php $x = `ls`;").unwrap_err();
    assert!(err.contains("unexpected character"), "{err}");
}

#[test]
fn rejects_unterminated_string() {
    let err = lex_php("<?php $s = \"oops;").unwrap_err();
    assert!(err.contains("unterminated string"), "{err}");
}

#[test]
fn flags_interpolated_double_quoted_string() {
    // `$name` and `{$x}` interpolate → InterpStr (raw, undecoded).
    let t = toks(r#"<?php $a = "hi $name"; $b = "v={$x}";"#);
    assert!(
        t.iter()
            .any(|x| matches!(x, PTok::InterpStr(s) if s == "hi $name")),
        "expected InterpStr for \"hi $name\" in {t:?}"
    );
    assert!(t.iter().any(|x| matches!(x, PTok::InterpStr(_) if true)));
    assert!(
        !t.iter().any(|x| matches!(x, PTok::Str(_))),
        "no plain Str expected in {t:?}"
    );
}

#[test]
fn does_not_flag_non_interpolating_strings() {
    // Single-quote never interpolates; double-quote with `$5`/no var-start is literal.
    let t = toks(r#"<?php $a = 'has $name'; $b = "cost $5"; $c = "plain";"#);
    use PTok::*;
    assert!(
        t.contains(&Str("has $name".into())),
        "single-quote literal: {t:?}"
    );
    assert!(t.contains(&Str("cost $5".into())), "$5 is literal: {t:?}");
    assert!(t.contains(&Str("plain".into())));
    assert!(
        !t.iter().any(|x| matches!(x, InterpStr(_))),
        "no InterpStr expected in {t:?}"
    );
}

#[test]
fn lexes_increment_and_compound_assign() {
    let t = toks("<?php $i++; --$j; $n += 1; $s .= \"x\"; $m ??= 0;");
    use PTok::*;
    for want in [Inc, Dec, PlusEq, DotEq, CoalesceEq] {
        assert!(t.contains(&want), "missing {want:?} in {t:?}");
    }
}
