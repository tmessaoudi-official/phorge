use super::*;
use crate::ast::{ClassMember, Expr, Item, Modifier, Pattern, Stmt, StrPart, Type, Visibility};
use crate::lexer::lex;

/// Helper: lex `src` and build a parser over the tokens.
fn parser(src: &str) -> Parser {
    Parser::new(lex(src).expect("lex ok"))
}

/// Helper: parse a whole program, panicking on a parse error.
fn prog(src: &str) -> Program {
    parser(src).parse_program().expect("parse ok")
}

/// Helper: parse a whole program expecting a parse error, returning its rendered message.
fn prog_err(src: &str) -> String {
    parser(src).parse_program().unwrap_err().render(src)
}

#[test]
fn parses_private_class_visibility() {
    match &prog("package Main;\nprivate class P {}").items[0] {
        Item::Class(c) => assert_eq!(c.vis, Visibility::Private),
        other => panic!("expected class, got {other:?}"),
    }
}

#[test]
fn parses_internal_function_visibility() {
    match &prog("package Main;\ninternal function f() {}").items[0] {
        Item::Function(f) => assert_eq!(f.vis, Visibility::Internal),
        other => panic!("expected function, got {other:?}"),
    }
}

#[test]
fn parses_internal_enum_and_interface_visibility() {
    match &prog("package Main;\ninternal enum E { A() }").items[0] {
        Item::Enum(e) => assert_eq!(e.vis, Visibility::Internal),
        other => panic!("expected enum, got {other:?}"),
    }
    match &prog("package Main;\nprivate interface I { function m() -> int; }").items[0] {
        Item::Interface(i) => assert_eq!(i.vis, Visibility::Private),
        other => panic!("expected interface, got {other:?}"),
    }
}

#[test]
fn bare_decl_defaults_to_public() {
    match &prog("package Main;\nclass C {}").items[0] {
        Item::Class(c) => assert_eq!(c.vis, Visibility::Public),
        other => panic!("expected class, got {other:?}"),
    }
}

#[test]
fn s8_use_dot_lookahead_splits_trait_from_resolution() {
    // M-RT S8 D9: `use T;` (no dot) is trait composition; `use A.foo` (dot) is an S6b resolution
    // clause. Both can appear in the same class body and must land in the right buckets.
    match &prog(
        "package Main;\nopen class A { open function foo() -> int { return 1; } }\n\
             trait T { function bar() -> int { return 2; } }\n\
             class C extends A { use T; use A.foo }",
    )
    .items
    .last()
    .unwrap()
    {
        Item::Class(c) => {
            assert_eq!(c.uses.len(), 1, "one trait `use`");
            assert_eq!(c.uses[0].name, "T");
            assert_eq!(c.resolutions.len(), 1, "one resolution clause");
        }
        other => panic!("expected class, got {other:?}"),
    }
}

#[test]
fn explicit_public_enum_parses() {
    match &prog("package Main;\npublic enum E { A() }").items[0] {
        Item::Enum(e) => assert_eq!(e.vis, Visibility::Public),
        other => panic!("expected enum, got {other:?}"),
    }
}

#[test]
fn conflicting_visibility_prefix_is_rejected() {
    let err = prog_err("package Main;\npublic private class C {}");
    assert!(err.contains("a single visibility"), "got: {err}");
}

#[test]
fn visibility_on_import_is_rejected() {
    let err = prog_err("package Main;\nprivate import Core.Console;");
    assert!(err.contains("cannot carry a visibility"), "got: {err}");
}

/// Helper: parse `src` as a single expression.
fn expr(src: &str) -> Expr {
    parser(src).parse_expr().expect("parse ok")
}

fn ty(src: &str) -> Type {
    parser(src).parse_type().expect("parse ok")
}

fn pat(src: &str) -> Pattern {
    parser(src).parse_pattern().expect("parse ok")
}

/// Helper: parse `src` as a single statement.
fn stmt(src: &str) -> Stmt {
    parser(src).parse_stmt().expect("parse ok")
}

#[test]
fn parse_type_union_and_single() {
    // A union of three; a single type is returned unchanged (no wrapping).
    match ty("A | B | C") {
        Type::Union(members, _) => assert_eq!(members.len(), 3),
        other => panic!("expected union, got {other:?}"),
    }
    assert!(matches!(ty("A"), Type::Named { .. }));
    // `?` binds to its immediate member: `A | B?` ≡ `A | (B?)`.
    match ty("A | B?") {
        Type::Union(m, _) => assert!(matches!(m[1], Type::Optional { .. })),
        other => panic!("expected union, got {other:?}"),
    }
    // a union nests inside a generic argument.
    assert!(matches!(ty("List<A | B>"), Type::Named { .. }));
}

#[test]
fn parse_type_intersection_and_precedence() {
    // An intersection of three; a single type is returned unchanged.
    match ty("A & B & C") {
        Type::Intersection(members, _) => assert_eq!(members.len(), 3),
        other => panic!("expected intersection, got {other:?}"),
    }
    // `&` binds tighter than `|`: `A | B & C` ≡ `A | (B & C)` — a union whose 2nd member is an
    // intersection.
    match ty("A | B & C") {
        Type::Union(m, _) => {
            assert_eq!(m.len(), 2);
            assert!(matches!(m[0], Type::Named { .. }));
            assert!(matches!(m[1], Type::Intersection(_, _)));
        }
        other => panic!("expected union, got {other:?}"),
    }
    // an intersection nests inside a generic argument and a function param.
    assert!(matches!(ty("List<A & B>"), Type::Named { .. }));
    assert!(matches!(ty("(A & B) -> C"), Type::Function { .. }));
}

#[test]
fn parse_type_pattern_vs_binding() {
    match pat("Circle c") {
        Pattern::Type {
            type_name, binding, ..
        } => {
            assert_eq!(type_name, "Circle");
            assert_eq!(binding.as_deref(), Some("c"));
        }
        other => panic!("expected type pattern, got {other:?}"),
    }
    // `Type _` binds nothing.
    assert!(matches!(
        pat("Circle _"),
        Pattern::Type { binding: None, .. }
    ));
    // a lone ident stays a catch-all Binding (the documented footgun, preserved).
    assert!(matches!(pat("Circle"), Pattern::Binding { .. }));
}

/// Helper: parse `src` as a top-level item.
fn item(src: &str) -> Item {
    parser(src).parse_item().expect("parse ok")
}

/// Render an expression to a fully-parenthesized string so precedence is visible.
fn sexpr(e: &Expr) -> String {
    match e {
        Expr::Int(n, _) => n.to_string(),
        Expr::Float(f, _) => format!("{f}"),
        Expr::Bool(b, _) => b.to_string(),
        Expr::Null(_) => "null".into(),
        Expr::Ident(s, _) => s.clone(),
        Expr::This(_) => "this".into(),
        Expr::Unary { op, expr, .. } => {
            let o = match op {
                UnaryOp::Neg => "-",
                UnaryOp::Not => "!",
            };
            format!("({o} {})", sexpr(expr))
        }
        Expr::Binary { op, lhs, rhs, .. } => {
            let o = match op {
                BinaryOp::Add => "+",
                BinaryOp::Sub => "-",
                BinaryOp::Mul => "*",
                BinaryOp::Div => "/",
                BinaryOp::Rem => "%",
                BinaryOp::Eq => "==",
                BinaryOp::NotEq => "!=",
                BinaryOp::Lt => "<",
                BinaryOp::Gt => ">",
                BinaryOp::Le => "<=",
                BinaryOp::Ge => ">=",
                BinaryOp::And => "&&",
                BinaryOp::Or => "||",
                BinaryOp::Pipe => "|>",
                BinaryOp::Coalesce => "??",
            };
            format!("({o} {} {})", sexpr(lhs), sexpr(rhs))
        }
        Expr::Member {
            object, name, safe, ..
        } => format!(
            "{}{}{}",
            sexpr(object),
            if *safe { "?." } else { "." },
            name
        ),
        Expr::Call { callee, args, .. } => {
            let a: Vec<String> = args.iter().map(sexpr).collect();
            format!("{}({})", sexpr(callee), a.join(", "))
        }
        Expr::Index { object, index, .. } => format!("{}[{}]", sexpr(object), sexpr(index)),
        Expr::Lambda { params, body, .. } => {
            let ps: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
            let body_str = match body {
                LambdaBody::Expr(e) => sexpr(e),
                LambdaBody::Block(_) => "<block>".into(),
            };
            format!("(lambda ({}) {})", ps.join(" "), body_str)
        }
        Expr::InstanceOf {
            value, type_name, ..
        } => format!("(instanceof {} {type_name})", sexpr(value)),
        other => format!("{other:?}"),
    }
}

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
fn parses_types() {
    match ty("int") {
        Type::Named { name, args, .. } => {
            assert_eq!(name, "int");
            assert!(args.is_empty());
        }
        other => panic!("got {other:?}"),
    }
    match ty("List<Shape>") {
        Type::Named { name, args, .. } => {
            assert_eq!(name, "List");
            assert_eq!(args.len(), 1);
        }
        other => panic!("got {other:?}"),
    }
    match ty("Map<string, int>") {
        Type::Named { name, args, .. } => {
            assert_eq!(name, "Map");
            assert_eq!(args.len(), 2);
        }
        other => panic!("got {other:?}"),
    }
    assert!(matches!(ty("int?"), Type::Optional { .. }));
    // nested generics
    match ty("List<Map<string, int>>") {
        Type::Named { name, args, .. } => {
            assert_eq!(name, "List");
            assert_eq!(args.len(), 1);
        }
        other => panic!("got {other:?}"),
    }
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
    assert_eq!(sexpr(&expr("a instanceof Foo")), "(instanceof a Foo)");
    assert_eq!(sexpr(&expr("a ?? b")), "(?? a b)");
    // `??` binds looser than `||`: `a || b ?? c` is `(a || b) ?? c`
    assert_eq!(sexpr(&expr("a || b ?? c")), "(?? (|| a b) c)");
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
    // A lambda element consumes its own `=>`, so `[fn(int x) => x]` is a one-element list.
    match expr("[fn(int x) => x]") {
        Expr::List(items, _) => {
            assert_eq!(items.len(), 1);
            assert!(matches!(items[0], Expr::Lambda { .. }));
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn rejects_mixed_list_map_separators() {
    // Once list-or-map is chosen by the first element, a mismatched separator errors cleanly.
    assert!(parser("[1, 2 => 3]").parse_expr().is_err()); // list mode, stray `=>`
    assert!(parser("[\"a\" => 1, \"b\"]").parse_expr().is_err()); // map mode, missing `=> v`
}

#[test]
fn parses_generic_function_type_params() {
    // `function id<T>(T x) -> T { … }` records the type parameter list (M-RT S7).
    match item("function id<T, U>(T a, U b) -> T { return a; }") {
        Item::Function(f) => assert_eq!(f.type_params, vec!["T".to_string(), "U".to_string()]),
        other => panic!("expected a generic function, got {other:?}"),
    }
    // A non-generic function has an empty type-param list.
    match item("function plain(int x) -> int { return x; }") {
        Item::Function(f) => assert!(f.type_params.is_empty()),
        other => panic!("expected a function, got {other:?}"),
    }
}

#[test]
fn parses_generic_methods() {
    // M-RT generics-all: a method may declare `<T>` just like a free function.
    let item = parser("class C { function m<T>(T x) -> T { return x; } }")
        .parse_item()
        .expect("generic method should parse");
    match item {
        Item::Class(c) => match &c.members[0] {
            crate::ast::ClassMember::Method(f) => {
                assert_eq!(f.type_params, vec!["T".to_string()]);
            }
            _ => panic!("expected a method"),
        },
        _ => panic!("expected a class"),
    }
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
    let mut p = parser("\"Hello {name\"");
    assert!(p.parse_expr().is_err());
}

#[test]
fn parses_patterns() {
    assert!(matches!(pat("_"), Pattern::Wildcard(_)));
    match pat("x") {
        Pattern::Binding { name, .. } => assert_eq!(name, "x"),
        other => panic!("got {other:?}"),
    }
    assert!(matches!(pat("42"), Pattern::Int(42, _)));
    assert!(matches!(pat("true"), Pattern::Bool(true, _)));
    assert!(matches!(pat("null"), Pattern::Null(_)));
    // variant destructure
    match pat("Circle(r)") {
        Pattern::Variant { name, fields, .. } => {
            assert_eq!(name, "Circle");
            assert_eq!(fields.len(), 1);
            assert!(matches!(&fields[0], Pattern::Binding { name, .. } if name == "r"));
        }
        other => panic!("got {other:?}"),
    }
    match pat("Rect(w, h)") {
        Pattern::Variant { name, fields, .. } => {
            assert_eq!(name, "Rect");
            assert_eq!(fields.len(), 2);
        }
        other => panic!("got {other:?}"),
    }
    // nested variant patterns
    match pat("Wrap(Circle(r))") {
        Pattern::Variant { fields, .. } => {
            assert!(matches!(&fields[0], Pattern::Variant { .. }))
        }
        other => panic!("got {other:?}"),
    }
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
fn parses_return_stmt() {
    assert!(matches!(stmt("return;"), Stmt::Return { value: None, .. }));
    match stmt("return 1 + 2;") {
        Stmt::Return {
            value: Some(Expr::Binary { .. }),
            ..
        } => {}
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_expr_stmt() {
    match stmt("Console.println(x);") {
        Stmt::Expr(Expr::Call { .. }, _) => {}
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_block_stmt() {
    match stmt("{ return; return 1; }") {
        Stmt::Block(body, _) => assert_eq!(body.len(), 2),
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_throw_stmt() {
    match stmt("throw ParseError(\"x\");") {
        Stmt::Throw {
            value: Expr::Call { .. },
            ..
        } => {}
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_try_catch_finally() {
    match stmt("try { f(); } catch (ParseError e) { g(); } finally { h(); }") {
        Stmt::Try {
            body,
            catches,
            finally_block,
            ..
        } => {
            assert_eq!(body.len(), 1);
            assert_eq!(catches.len(), 1);
            assert_eq!(catches[0].name, "e");
            assert!(matches!(&catches[0].ty, Type::Named { name, .. } if name == "ParseError"));
            assert!(finally_block.is_some());
        }
        other => panic!("got {other:?}"),
    }
    // A finally-only try (no catch) is allowed.
    assert!(matches!(
        stmt("try { f(); } finally { h(); }"),
        Stmt::Try {
            catches,
            finally_block: Some(_),
            ..
        } if catches.is_empty()
    ));
    // A bare `try {}` with neither catch nor finally is a parse error.
    assert!(parser("try { f(); }").parse_stmt().is_err());
}

#[test]
fn parses_multi_catch() {
    match stmt("try { f(); } catch (A a) { x(); } catch (B b) { y(); }") {
        Stmt::Try {
            catches,
            finally_block,
            ..
        } => {
            assert_eq!(catches.len(), 2);
            assert_eq!(catches[0].name, "a");
            assert_eq!(catches[1].name, "b");
            assert!(finally_block.is_none());
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_union_catch() {
    match stmt("try { f(); } catch (A | B e) { x(); }") {
        Stmt::Try { catches, .. } => {
            assert_eq!(catches.len(), 1);
            assert_eq!(catches[0].name, "e");
            assert!(matches!(&catches[0].ty, Type::Union(members, _) if members.len() == 2));
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_fn_throws_clause() {
    // Single declared exception type.
    match &prog("package Main;\nfunction f() -> int throws ParseError { return 1; }").items[0] {
        Item::Function(f) => {
            assert_eq!(f.throws.len(), 1);
            assert!(matches!(&f.throws[0], Type::Named { name, .. } if name == "ParseError"));
        }
        other => panic!("expected function, got {other:?}"),
    }
    // `throws A | B` captures the whole union as one `Type::Union`.
    match &prog("package Main;\nfunction g() throws A | B { return; }").items[0] {
        Item::Function(f) => {
            assert_eq!(f.throws.len(), 1);
            assert!(matches!(&f.throws[0], Type::Union(members, _) if members.len() == 2));
        }
        other => panic!("expected function, got {other:?}"),
    }
    // No throws clause ⇒ empty.
    match &prog("package Main;\nfunction h() {}").items[0] {
        Item::Function(f) => assert!(f.throws.is_empty()),
        other => panic!("expected function, got {other:?}"),
    }
}

#[test]
fn parses_var_decl_stmt() {
    match stmt("int n = 5;") {
        Stmt::VarDecl { ty, name, init, .. } => {
            assert!(matches!(ty, Type::Named { ref name, .. } if name == "int"));
            assert_eq!(name, "n");
            assert!(matches!(init, Expr::Int(5, _)));
        }
        other => panic!("got {other:?}"),
    }
    // generic-typed var-decl must not be mistaken for comparison
    match stmt("List<Shape> shapes = items;") {
        Stmt::VarDecl { name, .. } => assert_eq!(name, "shapes"),
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_mutable_typed_var_decl() {
    match stmt("mutable int x = 1;") {
        Stmt::VarDecl { name, mutable, .. } => {
            assert!(mutable);
            assert_eq!(name, "x");
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_mutable_inferred_var_decl() {
    match stmt("mutable var x = 1;") {
        Stmt::VarDecl { name, mutable, .. } => {
            assert!(mutable);
            assert_eq!(name, "x");
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn plain_var_decl_is_not_mutable() {
    match stmt("int x = 1;") {
        Stmt::VarDecl { mutable, .. } => assert!(!mutable),
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_reassignment() {
    match stmt("x = 2;") {
        Stmt::Assign { target, .. } => {
            assert!(matches!(target, Expr::Ident(ref n, _) if n == "x"));
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_compound_assign_desugars_to_binary() {
    use crate::ast::BinaryOp;
    // `x += 1;` ⟶ `x = x + 1` (M-mut.2): target is `x`, value is `x + 1`.
    for (src, want) in [
        ("x += 1;", BinaryOp::Add),
        ("x -= 1;", BinaryOp::Sub),
        ("x *= 2;", BinaryOp::Mul),
        ("x /= 2;", BinaryOp::Div),
        ("x %= 2;", BinaryOp::Rem),
        ("x ??= 0;", BinaryOp::Coalesce),
    ] {
        match stmt(src) {
            Stmt::Assign { target, value, .. } => {
                assert!(matches!(target, Expr::Ident(ref n, _) if n == "x"), "{src}");
                match value {
                    Expr::Binary { op, lhs, .. } => {
                        assert_eq!(op, want, "{src}");
                        assert!(matches!(*lhs, Expr::Ident(ref n, _) if n == "x"), "{src}");
                    }
                    other => panic!("{src}: expected Binary value, got {other:?}"),
                }
            }
            other => panic!("{src}: expected Assign, got {other:?}"),
        }
    }
}

#[test]
fn parses_increment_decrement_statements() {
    use crate::ast::BinaryOp;
    // `x++;` ⟶ `x = x + 1`; `x--;` ⟶ `x = x - 1` (statement form).
    for (src, want) in [("x++;", BinaryOp::Add), ("x--;", BinaryOp::Sub)] {
        match stmt(src) {
            Stmt::Assign { target, value, .. } => {
                assert!(matches!(target, Expr::Ident(ref n, _) if n == "x"), "{src}");
                match value {
                    Expr::Binary { op, lhs, rhs, .. } => {
                        assert_eq!(op, want, "{src}");
                        assert!(matches!(*lhs, Expr::Ident(ref n, _) if n == "x"), "{src}");
                        assert!(matches!(*rhs, Expr::Int(1, _)), "{src}");
                    }
                    other => panic!("{src}: expected Binary value, got {other:?}"),
                }
            }
            other => panic!("{src}: expected Assign, got {other:?}"),
        }
    }
}

#[test]
fn parses_clone_with() {
    match expr("p with { x = 9, y = 10 }") {
        Expr::CloneWith { object, fields, .. } => {
            assert!(matches!(*object, Expr::Ident(ref n, _) if n == "p"));
            assert_eq!(fields.len(), 2);
            assert_eq!(fields[0].0, "x");
            assert_eq!(fields[1].0, "y");
        }
        other => panic!("got {other:?}"),
    }
    // empty override list parses.
    match expr("p with { }") {
        Expr::CloneWith { fields, .. } => assert!(fields.is_empty()),
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_while_and_do_while() {
    match stmt("while (x < 3) { x = x + 1; }") {
        Stmt::While {
            post_cond, body, ..
        } => {
            assert!(!post_cond);
            assert_eq!(body.len(), 1);
        }
        other => panic!("got {other:?}"),
    }
    match stmt("do { x = x + 1; } while (x < 3);") {
        Stmt::While { post_cond, .. } => assert!(post_cond),
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_while_let_desugars_to_while_true_if_let() {
    // `while (var v = opt) { B }` ⟶ `while (true) { if (var v = opt) { B } else { break; } }`.
    match stmt("while (var v = opt) { use(v); }") {
        Stmt::While {
            cond,
            body,
            post_cond,
            ..
        } => {
            assert!(!post_cond);
            assert!(matches!(cond, Expr::Bool(true, _)));
            assert_eq!(body.len(), 1);
            match &body[0] {
                Stmt::If {
                    bind: Some(n),
                    else_block: Some(eb),
                    ..
                } => {
                    assert_eq!(n, "v");
                    assert!(matches!(eb.as_slice(), [Stmt::Break(_)]));
                }
                other => panic!("expected if-let, got {other:?}"),
            }
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_break_and_continue() {
    assert!(matches!(stmt("break;"), Stmt::Break(_)));
    assert!(matches!(stmt("continue;"), Stmt::Continue(_)));
}

#[test]
fn parses_c_style_for() {
    // Full C-for with all three clauses.
    match stmt("for (mutable int i = 0; i < n; i++) { use(i); }") {
        Stmt::CFor {
            init: Some(init),
            cond: Some(_),
            step: Some(step),
            body,
            ..
        } => {
            assert!(matches!(*init, Stmt::VarDecl { mutable: true, .. }));
            assert!(matches!(*step, Stmt::Assign { .. })); // i++ desugars to Assign
            assert_eq!(body.len(), 1);
        }
        other => panic!("got {other:?}"),
    }
    // All clauses empty: `for (;;)`.
    match stmt("for (;;) { x = 1; }") {
        Stmt::CFor {
            init: None,
            cond: None,
            step: None,
            ..
        } => {}
        other => panic!("got {other:?}"),
    }
}

#[test]
fn for_in_still_parses_as_for_in() {
    // The disambiguation must not regress the existing range/list for-in form.
    match stmt("for (int i in 0..3) { use(i); }") {
        Stmt::For { name, .. } => assert_eq!(name, "i"),
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_if_else() {
    match stmt("if (a) { return 1; } else { return 2; }") {
        Stmt::If {
            then_block,
            else_block: Some(eb),
            ..
        } => {
            assert_eq!(then_block.len(), 1);
            assert_eq!(eb.len(), 1);
        }
        other => panic!("got {other:?}"),
    }
    match stmt("if (a) { return 1; }") {
        Stmt::If {
            else_block: None, ..
        } => {}
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_else_if_chain() {
    match stmt("if (a) { return 1; } else if (b) { return 2; }") {
        Stmt::If {
            else_block: Some(eb),
            ..
        } => {
            assert_eq!(eb.len(), 1);
            assert!(matches!(eb[0], Stmt::If { .. }));
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_if_let_binding() {
    // `if (var x = e)` carries the bound name; the condition expr is the scrutinee.
    match stmt("if (var x = o) { return 1; } else { return 2; }") {
        Stmt::If {
            bind: Some(name),
            else_block: Some(eb),
            ..
        } => {
            assert_eq!(name, "x");
            assert_eq!(eb.len(), 1);
        }
        other => panic!("got {other:?}"),
    }
    // a plain condition has no binding
    match stmt("if (a) { return 1; }") {
        Stmt::If { bind: None, .. } => {}
        other => panic!("got {other:?}"),
    }
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

#[test]
fn parses_for_in() {
    match stmt("for (Shape s in shapes) { Console.println(s); }") {
        Stmt::For {
            ty,
            name,
            iter,
            body,
            ..
        } => {
            assert!(matches!(ty, Type::Named { ref name, .. } if name == "Shape"));
            assert_eq!(name, "s");
            assert!(matches!(iter, Expr::Ident(ref n, _) if n == "shapes"));
            assert_eq!(body.len(), 1);
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_function_decl() {
    match item("function area(Shape s) -> float { return s; }") {
        Item::Function(f) => {
            assert_eq!(f.name, "area");
            assert_eq!(f.params.len(), 1);
            assert_eq!(f.params[0].name, "s");
            assert!(f.ret.is_some());
            assert_eq!(f.body.len(), 1);
            assert!(f.modifiers.is_empty());
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_function_no_ret_no_params() {
    match item("function main() { Console.println(1); }") {
        Item::Function(f) => {
            assert_eq!(f.name, "main");
            assert!(f.params.is_empty());
            assert!(f.ret.is_none());
            assert_eq!(f.body.len(), 1);
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_enum_decl() {
    let src = "enum Shape { Circle(float radius), Rect(float w, float h), Unit, }";
    match item(src) {
        Item::Enum(e) => {
            assert_eq!(e.name, "Shape");
            assert_eq!(e.variants.len(), 3);
            assert_eq!(e.variants[0].name, "Circle");
            assert_eq!(e.variants[0].fields.len(), 1);
            assert_eq!(e.variants[1].fields.len(), 2);
            assert!(e.variants[2].fields.is_empty()); // bare variant
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_class_decl() {
    let src = "class Greeter { \
                     private string name; \
                     constructor(private string name) {} \
                     function greet() -> string { return name; } \
                   }";
    match item(src) {
        Item::Class(c) => {
            assert_eq!(c.name, "Greeter");
            assert_eq!(c.members.len(), 3);
            match &c.members[0] {
                ClassMember::Field {
                    modifiers, name, ..
                } => {
                    assert_eq!(name, "name");
                    assert_eq!(modifiers, &vec![Modifier::Private]);
                }
                other => panic!("member 0: {other:?}"),
            }
            match &c.members[1] {
                ClassMember::Constructor { params, .. } => {
                    assert_eq!(params.len(), 1);
                    assert_eq!(params[0].modifiers, vec![Modifier::Private]);
                    assert_eq!(params[0].name, "name");
                }
                other => panic!("member 1: {other:?}"),
            }
            match &c.members[2] {
                ClassMember::Method(f) => assert_eq!(f.name, "greet"),
                other => panic!("member 2: {other:?}"),
            }
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_mutable_field_and_ctor_param_modifier() {
    // M-mut.6: `mutable` is accepted in field + promoted-ctor-param modifier position.
    let src = "class C { \
                     mutable int count; \
                     constructor(public mutable int total) {} \
                   }";
    match item(src) {
        Item::Class(c) => {
            match &c.members[0] {
                ClassMember::Field {
                    modifiers, name, ..
                } => {
                    assert_eq!(name, "count");
                    assert_eq!(modifiers, &vec![Modifier::Mutable]);
                }
                other => panic!("member 0: {other:?}"),
            }
            match &c.members[1] {
                ClassMember::Constructor { params, .. } => {
                    assert_eq!(
                        params[0].modifiers,
                        vec![Modifier::Public, Modifier::Mutable]
                    );
                }
                other => panic!("member 1: {other:?}"),
            }
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn open_method_modifier_and_final_retired() {
    // S6a.1: `open` parses as a method modifier. (Methods use block bodies, not `=> expr`.)
    match item("class C { open function f() -> int { return 1; } }") {
        Item::Class(c) => match &c.members[0] {
            ClassMember::Method(m) => {
                assert_eq!(m.name, "f");
                assert_eq!(m.modifiers, vec![Modifier::Open]);
            }
            other => panic!("member 0: {other:?}"),
        },
        other => panic!("got {other:?}"),
    }
    // S6a.1: `final` is no longer a keyword — it now lexes as an ordinary identifier.
    let toks = lex("final").expect("lex ok");
    assert!(
        matches!(&toks[0].kind, TokenKind::Ident(s) if s == "final"),
        "expected `final` to lex as Ident, got {:?}",
        toks[0].kind
    );
}

#[test]
fn parses_open_class_with_single_extends() {
    // S6a.2: `open` class prefix + a single `extends` parent.
    let p = prog("package Main;\nopen class Animal {}\nclass Dog extends Animal {}");
    let animal = match &p.items[0] {
        Item::Class(c) => c,
        o => panic!("item 0: {o:?}"),
    };
    assert!(animal.open, "Animal should be open");
    assert!(animal.extends.is_empty(), "Animal extends nothing");
    let dog = match &p.items[1] {
        Item::Class(c) => c,
        o => panic!("item 1: {o:?}"),
    };
    assert!(!dog.open, "Dog is final-by-default (not open)");
    assert_eq!(dog.extends, vec!["Animal".to_string()]);
}

#[test]
fn open_prefix_on_a_non_class_is_an_error() {
    // S6a.2: `open` only applies to classes.
    let msg = prog_err("package Main;\nopen function f() {}");
    assert!(msg.contains("only a class"), "got: {msg}");
}

#[test]
fn parses_static_field_with_initializer() {
    // M-mut.7: `static mutable int total = 0;` — static modifier + field-level initializer.
    let src = "class C { static mutable int total = 0; }";
    match item(src) {
        Item::Class(c) => match &c.members[0] {
            ClassMember::Field {
                modifiers,
                name,
                init,
                ..
            } => {
                assert_eq!(name, "total");
                assert_eq!(modifiers, &vec![Modifier::Static, Modifier::Mutable]);
                assert!(matches!(init, Some(Expr::Int(0, _))));
            }
            other => panic!("member 0: {other:?}"),
        },
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_property_hook_get_and_set() {
    // M-mut.7b: `float fahrenheit { get => …; set(float v) { … } }` — a property hook with
    // both a computed-read body and an intercepted-write body.
    let src = "class Temp { \
                     mutable float celsius; \
                     float fahrenheit { \
                       get => this.celsius * 2.0; \
                       set(float v) { this.celsius = v; } \
                     } \
                   }";
    match item(src) {
        Item::Class(c) => match &c.members[1] {
            ClassMember::Hook {
                name, get, set, ty, ..
            } => {
                assert_eq!(name, "fahrenheit");
                assert!(matches!(ty, Type::Named { name, .. } if name == "float"));
                assert!(get.is_some(), "expected a get body");
                let (p, stmts) = set.as_ref().expect("expected a set body");
                assert_eq!(p.name, "v");
                assert_eq!(stmts.len(), 1);
            }
            other => panic!("member 1: {other:?}"),
        },
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_read_only_property_hook() {
    // A get-only hook (no `set`) is a read-only computed property.
    match item("class C { int doubled { get => 2; } }") {
        Item::Class(c) => match &c.members[0] {
            ClassMember::Hook { get, set, .. } => {
                assert!(get.is_some());
                assert!(set.is_none());
            }
            other => panic!("member 0: {other:?}"),
        },
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_class_implements_list() {
    // M-RT S2: `implements A, B` is parsed into ClassDecl.implements.
    match item("class Dog implements Speaker, Pet { function speak() -> string { return \"w\"; } }")
    {
        Item::Class(c) => {
            assert_eq!(c.name, "Dog");
            assert_eq!(c.implements, vec!["Speaker".to_string(), "Pet".to_string()]);
            assert_eq!(c.members.len(), 1);
        }
        other => panic!("got {other:?}"),
    }
    // No `implements` ⇒ empty list.
    match item("class Plain {}") {
        Item::Class(c) => assert!(c.implements.is_empty()),
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_interface_decl() {
    // M-RT S2: an interface is method signatures (no bodies) + an optional `extends` list.
    match item("interface Pet extends Speaker, Named { function speak() -> string; function age() -> int; }") {
            Item::Interface(i) => {
                assert_eq!(i.name, "Pet");
                assert_eq!(i.extends, vec!["Speaker".to_string(), "Named".to_string()]);
                assert_eq!(i.methods.len(), 2);
                assert_eq!(i.methods[0].name, "speak");
                assert!(i.methods[0].body.is_empty(), "signature has no body");
                assert_eq!(i.methods[1].name, "age");
            }
            other => panic!("got {other:?}"),
        }
}

#[test]
fn parses_import() {
    match item("import Core.Console;") {
        Item::Import { path, .. } => assert_eq!(path, vec!["Core", "Console"]),
        other => panic!("got {other:?}"),
    }
    match item("import a;") {
        Item::Import { path, .. } => assert_eq!(path, vec!["a"]),
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_package_declaration() {
    // `package a.b;` is captured on the Program, not as an Item (M5 S1).
    let prog = parser("package app.util; function main() {}")
        .parse_program()
        .expect("parse ok");
    assert_eq!(prog.package, vec!["app".to_string(), "util".to_string()]);
    // A bare file parses with an empty package — the checker, not the parser, enforces presence.
    let bare = parser("function main() {}")
        .parse_program()
        .expect("parse ok");
    assert!(bare.package.is_empty());
    // `package` after another item is a parse error (it must be the first declaration).
    assert!(parser("function main() {} package app;")
        .parse_program()
        .is_err());
}

#[test]
fn parses_program_multiple_items() {
    let src = "import Core.Console; enum E { A, } function main() { return; }";
    let prog = parser(src).parse_program().expect("parse ok");
    assert_eq!(prog.items.len(), 3);
    assert!(matches!(prog.items[0], Item::Import { .. }));
    assert!(matches!(prog.items[1], Item::Enum(_)));
    assert!(matches!(prog.items[2], Item::Function(_)));
}

#[test]
fn empty_program_parses() {
    let prog = parser("").parse_program().expect("parse ok");
    assert!(prog.items.is_empty());
}

#[test]
fn parses_function_type_annotation() {
    // a function-typed parameter must parse
    let result = parser("package Main; function apply(int x, (int) -> int f) -> int { return x; }")
        .parse_program();
    assert!(
        result.is_ok(),
        "function-typed param should parse: {result:?}"
    );
    // nested + zero-arg
    let result2 = parser("package Main; function f() -> () -> int { }").parse_program();
    assert!(
        result2.is_ok(),
        "zero-arg function type should parse: {result2:?}"
    );
    // direct type parsing
    match ty("(int) -> int") {
        Type::Function { params, ret, .. } => {
            assert_eq!(params.len(), 1);
            assert!(matches!(ret.as_ref(), Type::Named { name, .. } if name == "int"));
        }
        other => panic!("expected Type::Function, got {other:?}"),
    }
}
