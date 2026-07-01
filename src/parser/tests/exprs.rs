//! Parser tests — exprs (M-Decomp W3.1b, mirrors the source clusters).

use super::support::*;

#[test]
fn peek_and_advance_walk_tokens() {
    use crate::token::TokenKind::*;
    let mut p = parser("+ -");
    assert_eq!(*p.peek(), Plus);
    assert_eq!(p.advance().kind, Plus);
    assert_eq!(*p.peek(), Minus);
    assert_eq!(p.advance().kind, Minus);
    assert_eq!(*p.peek(), Eof);
    // advancing at EOF stays at EOF (does not panic)
    assert_eq!(p.advance().kind, Eof);
    assert_eq!(*p.peek(), Eof);
}

#[test]
fn parses_literals_ident_this() {
    assert!(matches!(expr("42"), Expr::Int(42, _)));
    assert!(matches!(expr("3.5"), Expr::Float(f, _) if (f - 3.5).abs() < 1e-9));
    assert!(matches!(expr("true"), Expr::Bool(true, _)));
    assert!(matches!(expr("false"), Expr::Bool(false, _)));
    assert!(matches!(expr("null"), Expr::Null(_)));
    assert!(matches!(expr("this"), Expr::This(_)));
    match expr("foo") {
        Expr::Ident(name, _) => assert_eq!(name, "foo"),
        other => panic!("expected Ident, got {other:?}"),
    }
}

#[test]
fn parses_parenthesized() {
    // parens are grouping only — the inner expression is returned directly
    assert!(matches!(expr("(7)"), Expr::Int(7, _)));
}

#[test]
fn parses_decimal_literal_with_text_scale() {
    // `19.99d` → Expr::Decimal { 1999, scale 2 }; trailing zeros widen the scale (M-NUM S1).
    assert!(matches!(
        expr("19.99d"),
        Expr::Decimal {
            unscaled: 1999,
            scale: 2,
            ..
        }
    ));
    assert!(matches!(
        expr("1.500d"),
        Expr::Decimal {
            unscaled: 1500,
            scale: 3,
            ..
        }
    ));
    assert!(matches!(
        expr("100d"),
        Expr::Decimal {
            unscaled: 100,
            scale: 0,
            ..
        }
    ));
}

#[test]
fn precedence_and_associativity() {
    assert_eq!(sexpr(&expr("1 + 2 * 3")), "(+ 1 (* 2 3))");
    assert_eq!(sexpr(&expr("1 * 2 + 3")), "(+ (* 1 2) 3)");
    assert_eq!(sexpr(&expr("1 - 2 - 3")), "(- (- 1 2) 3)"); // left-assoc
    assert_eq!(sexpr(&expr("1 < 2 == true")), "(== (< 1 2) true)");
    assert_eq!(sexpr(&expr("a && b || c")), "(|| (&& a b) c)");
    assert_eq!(sexpr(&expr("-a + b")), "(+ (- a) b)");
    assert_eq!(sexpr(&expr("!a && b")), "(&& (! a) b)");
    assert_eq!(sexpr(&expr("x |> f")), "f(x)");
    // pipe is the lowest: `a + b |> f` == `(a + b) |> f`
    assert_eq!(sexpr(&expr("a + b |> f")), "f((+ a b))");
    // `**` binds tighter than `*` and is right-associative (PHP-identical).
    assert_eq!(sexpr(&expr("2 ** 3 ** 2")), "(** 2 (** 3 2))"); // right-assoc
    assert_eq!(sexpr(&expr("2 * 3 ** 2")), "(* 2 (** 3 2))"); // ** tighter than *
    assert_eq!(sexpr(&expr("-a ** 2")), "(** (- a) 2)"); // unary parsed before **
    assert_eq!(sexpr(&expr("a instanceof Foo")), "(instanceof a Foo)");
    assert_eq!(sexpr(&expr("a ?? b")), "(?? a b)");
    // `??` binds looser than `||`: `a || b ?? c` is `(a || b) ?? c`
    assert_eq!(sexpr(&expr("a || b ?? c")), "(?? (|| a b) c)");
    // `as` is the checked cast (M4): same precedence as `instanceof`, RHS is a type name.
    assert_eq!(sexpr(&expr("a as Foo")), "(as a Foo)");
    // `as` binds tighter than `??` (spec): `a as Foo ?? b` is `((a as Foo)) ?? b`.
    assert_eq!(sexpr(&expr("a as Foo ?? b")), "(?? (as a Foo) b)");
    // member access binds tighter than `as`: `a.b as Foo` is `((a.b)) as Foo`.
    assert_eq!(sexpr(&expr("a.b as Foo")), "(as a.b Foo)");
}

#[test]
fn parses_postfix_chains() {
    // member access
    match expr("a.b") {
        Expr::Member { object, name, .. } => {
            assert!(matches!(*object, Expr::Ident(ref s, _) if s == "a"));
            assert_eq!(name, "b");
        }
        other => panic!("got {other:?}"),
    }
    // call with args (also covers constructor calls like Circle(2.0))
    match expr("f(1, 2)") {
        Expr::Call { callee, args, .. } => {
            assert!(matches!(*callee, Expr::Ident(ref s, _) if s == "f"));
            assert_eq!(args.len(), 2);
        }
        other => panic!("got {other:?}"),
    }
    match expr("Circle(2.0)") {
        Expr::Call { callee, args, .. } => {
            assert!(matches!(*callee, Expr::Ident(ref s, _) if s == "Circle"));
            assert_eq!(args.len(), 1);
        }
        other => panic!("got {other:?}"),
    }
    // index
    assert!(matches!(expr("a[0]"), Expr::Index { .. }));
    // empty-arg call
    match expr("g()") {
        Expr::Call { args, .. } => assert!(args.is_empty()),
        other => panic!("got {other:?}"),
    }
    // chaining: obj.method(x).field — outermost is Member "field"
    match expr("obj.method(x).field") {
        Expr::Member { name, .. } => assert_eq!(name, "field"),
        other => panic!("got {other:?}"),
    }
    // postfix binds tighter than unary: -a.b  ==  -(a.b)
    assert_eq!(sexpr(&expr("-a.b")), "(- a.b)");
}

#[test]
fn parses_map_and_list_literals() {
    // A `=>` after the first element makes it a map literal.
    match expr("[\"a\" => 1, \"b\" => 2]") {
        Expr::Map(pairs, _) => assert_eq!(pairs.len(), 2),
        other => panic!("got {other:?}"),
    }
    // No `=>` → a list literal (unchanged).
    match expr("[1, 2, 3]") {
        Expr::List(items, _) => assert_eq!(items.len(), 3),
        other => panic!("got {other:?}"),
    }
    // `[]` stays the empty *list* (an empty map literal is deferred).
    match expr("[]") {
        Expr::List(items, _) => assert!(items.is_empty()),
        other => panic!("got {other:?}"),
    }
    // A lambda element consumes its own `=>`, so `[function(int x) => x]` is a one-element list.
    match expr("[function(int x) => x]") {
        Expr::List(items, _) => {
            assert_eq!(items.len(), 1);
            assert!(matches!(items[0], Expr::Lambda { .. }));
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_lambda_colon_return_type() {
    // A-1: a typed lambda uses `:` for its return type, then `=>` for the (expression) body:
    // `function(int x): string => …`. The `->` form stays as a transition alias.
    match expr(r#"function(int x): string => "n""#) {
        Expr::Lambda { ret, body, .. } => {
            assert!(ret.is_some());
            assert!(matches!(body, LambdaBody::Expr(_)));
        }
        other => panic!("expected lambda, got {other:?}"),
    }
    // block body with `:` return type
    match expr("function(int x): int { return x; }") {
        Expr::Lambda { ret, body, .. } => {
            assert!(ret.is_some());
            assert!(matches!(body, LambdaBody::Block(_)));
        }
        other => panic!("expected lambda, got {other:?}"),
    }
    // `->` alias still parses
    assert!(matches!(
        expr("function(int x) -> int => x"),
        Expr::Lambda { .. }
    ));
}

#[test]
fn rejects_mixed_list_map_separators() {
    // Once list-or-map is chosen by the first element, a mismatched separator errors cleanly.
    assert!(parser("[1, 2 => 3]").parse_expr().is_err()); // list mode, stray `=>`
    assert!(parser("[\"a\" => 1, \"b\"]").parse_expr().is_err()); // map mode, missing `=> v`
}

#[test]
fn parses_propagate_postfix() {
    // Postfix `?` is error propagation (M-faults 2a). The lexer munches `??`/`?.` separately, so a
    // lone `?` here is unambiguous and `a?.b` still parses as a safe Member, not propagation.
    assert!(matches!(expr("a?"), Expr::Propagate { .. }));
    assert!(matches!(expr("f(x)?"), Expr::Propagate { .. }));
    assert!(matches!(expr("a?.b"), Expr::Member { safe: true, .. }));
}

#[test]
fn parses_safe_member_access() {
    // `?.` parses as a *safe* Member; plain `.` stays unsafe. `sexpr` renders the distinction.
    assert_eq!(sexpr(&expr("a?.b")), "a?.b");
    assert_eq!(sexpr(&expr("a.b")), "a.b");
    // chained safe access stays right-extending
    assert_eq!(sexpr(&expr("a?.b?.c")), "a?.b?.c");
    // a safe method call is a `Call` whose callee is a safe `Member`
    assert_eq!(sexpr(&expr("a?.m(x)")), "a?.m(x)");
    match expr("a?.b") {
        Expr::Member { name, safe, .. } => {
            assert_eq!(name, "b");
            assert!(safe, "`?.` must set safe = true");
        }
        other => panic!("got {other:?}"),
    }
    match expr("a.b") {
        Expr::Member { safe, .. } => assert!(!safe, "`.` must set safe = false"),
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_list_literals() {
    match expr("[1, 2, 3]") {
        Expr::List(items, _) => assert_eq!(items.len(), 3),
        other => panic!("got {other:?}"),
    }
    match expr("[]") {
        Expr::List(items, _) => assert!(items.is_empty()),
        other => panic!("got {other:?}"),
    }
    // trailing comma allowed
    match expr("[1, 2,]") {
        Expr::List(items, _) => assert_eq!(items.len(), 2),
        other => panic!("got {other:?}"),
    }
    // nested + constructor-call elements (the spec sample: [Circle(2.0), Rect(3.0, 4.0)])
    match expr("[Circle(2.0), Rect(3.0, 4.0)]") {
        Expr::List(items, _) => assert_eq!(items.len(), 2),
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_string_interpolation() {
    // plain string -> a single literal part
    match expr("\"hello\"") {
        Expr::Str(parts, _) => {
            assert_eq!(parts.len(), 1);
            assert!(matches!(&parts[0], StrPart::Literal(s) if s == "hello"));
        }
        other => panic!("got {other:?}"),
    }
    // interpolation: "Hello {name}" -> [Literal("Hello "), Expr(name)]
    match expr("\"Hello {name}\"") {
        Expr::Str(parts, _) => {
            assert_eq!(parts.len(), 2);
            assert!(matches!(&parts[0], StrPart::Literal(s) if s == "Hello "));
            assert!(
                matches!(&parts[1], StrPart::Expr(b) if matches!(**b, Expr::Ident(ref n,_) if n == "name"))
            );
        }
        other => panic!("got {other:?}"),
    }
    // embedded call expression: "area = {area(s)}"
    match expr("\"area = {area(s)}\"") {
        Expr::Str(parts, _) => {
            assert_eq!(parts.len(), 2);
            assert!(matches!(&parts[1], StrPart::Expr(b) if matches!(**b, Expr::Call { .. })));
        }
        other => panic!("got {other:?}"),
    }
    // no parts before/after braces -> single Expr part
    match expr("\"{x}\"") {
        Expr::Str(parts, _) => {
            assert_eq!(parts.len(), 1);
            assert!(matches!(&parts[0], StrPart::Expr(_)));
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn unterminated_interpolation_errors() {
    // The lexer owns the interpolation split (so `\{` literal braces are unambiguous), so an
    // unterminated interpolation is caught at lex stage rather than parse. Since M-DOGFOOD W2 a `"`
    // inside `{…}` opens a NESTED string, so `"Hello {name"` (missing `}`) now surfaces as an
    // unterminated nested string — still a lex error naming the interpolation, still at the right spot.
    let e = lex("\"Hello {name\"").unwrap_err();
    assert!(
        e.message.contains("interpolation"),
        "expected an interpolation lex error, got: {}",
        e.message
    );
}

#[test]
fn parses_match_expression() {
    let e = expr("match s { Circle(r) => r, Rect(w, h) => w, _ => 0 }");
    match e {
        Expr::Match {
            scrutinee, arms, ..
        } => {
            assert!(matches!(*scrutinee, Expr::Ident(ref n, _) if n == "s"));
            assert_eq!(arms.len(), 3);
            assert!(matches!(arms[0].pattern, Pattern::Variant { .. }));
            assert!(matches!(arms[2].pattern, Pattern::Wildcard(_)));
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_match_with_trailing_comma_and_exprs() {
    // mirrors the spec sample body
    let e = expr("match s { Circle(r) => 3.14159 * r * r, Rect(w, h) => w * h, }");
    match e {
        Expr::Match { arms, .. } => {
            assert_eq!(arms.len(), 2);
            assert!(matches!(arms[0].body, Expr::Binary { .. }));
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn or_pattern_desugars_to_one_arm_per_alternative() {
    // `1 | 2 | 3 => "low"` expands to three arms each carrying the (cloned) body — every backend
    // sees ordinary arms, so exhaustiveness / dedup / narrowing are unchanged.
    let e = expr(r#"match n { 1 | 2 | 3 => "low", _ => "hi" }"#);
    match e {
        Expr::Match { arms, .. } => {
            assert_eq!(arms.len(), 4); // 3 expanded + the wildcard
            assert!(matches!(arms[0].pattern, Pattern::Int(1, _)));
            assert!(matches!(arms[1].pattern, Pattern::Int(2, _)));
            assert!(matches!(arms[2].pattern, Pattern::Int(3, _)));
            assert!(matches!(arms[3].pattern, Pattern::Wildcard(_)));
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn or_pattern_rejects_binding_alternatives() {
    // a bare-name alternative, a `_` alternative, and a variant binder are all rejected.
    for src in [
        "match n { 1 | x => 0, _ => 1 }",
        "match n { _ | 1 => 0 }",
        "match e { A(v) | B() => 0 }",
    ] {
        let d = parser(src).parse_expr().unwrap_err();
        assert_eq!(d.code, Some("E-OR-PATTERN-BIND"), "{src}");
    }
    // a binding-free variant alternative IS allowed (`Some(_) | None()`).
    assert!(parser("match o { Some(_) | None() => 0 }")
        .parse_expr()
        .is_ok());
}

#[test]
fn parses_ranges() {
    match expr("0..3") {
        Expr::Range { inclusive, .. } => assert!(!inclusive),
        other => panic!("got {other:?}"),
    }
    match expr("1..=n") {
        Expr::Range { inclusive, .. } => assert!(inclusive),
        other => panic!("got {other:?}"),
    }
    // ranges bind looser than `+`: `0..n + 1` is `0..(n + 1)`
    match expr("0..n + 1") {
        Expr::Range { end, .. } => assert!(matches!(*end, Expr::Binary { .. })),
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_expression_if() {
    match expr("if (true) { 1 } else { 2 }") {
        Expr::If { .. } => {}
        other => panic!("got {other:?}"),
    }
    // a missing else is a parse error in expression position
    let mut p = parser("if (true) { 1 }");
    assert!(p.parse_expr().is_err());
}

#[test]
fn parses_force_unwrap() {
    // postfix `!` is a force-unwrap; prefix `!` stays a logical-not unary
    match expr("o!") {
        Expr::Force { .. } => {}
        other => panic!("got {other:?}"),
    }
    match expr("!b") {
        Expr::Unary {
            op: UnaryOp::Not, ..
        } => {}
        other => panic!("got {other:?}"),
    }
    // `a != b` must remain a single NotEq comparison, never `a` `!` `= b`
    match expr("a != b") {
        Expr::Binary {
            op: BinaryOp::NotEq,
            ..
        } => {}
        other => panic!("got {other:?}"),
    }
}
