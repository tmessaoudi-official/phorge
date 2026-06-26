//! M-Lift L2 — PHP parser tests. Asserts AST shape for the Tier-1 subset and that out-of-tier
//! constructs are rejected *loudly* (never silently misparsed).

use super::ast::*;
use super::lexer::lex_php;
use super::parser::parse_php;

fn parse(src: &str) -> PhpProgram {
    parse_php(lex_php(src).expect("lex")).expect("parse")
}

fn perr(src: &str) -> String {
    parse_php(lex_php(src).expect("lex")).expect_err("expected parse error")
}

/// Convenience: parse a single top-level statement out of a `<?php …` snippet.
fn stmt(src: &str) -> PhpStmt {
    match parse(src).items.into_iter().next().expect("one item") {
        PhpItem::Stmt(s) => s,
        other => panic!("expected a statement, got {other:?}"),
    }
}

/// Convenience: parse a single expression from `<?php <expr>;`.
fn expr(src: &str) -> PhpExpr {
    match stmt(&format!("<?php {src};")) {
        PhpStmt::Expr(e) => e,
        other => panic!("expected an expression statement, got {other:?}"),
    }
}

#[test]
fn parses_typed_function() {
    let p = parse("<?php\nfunction add(int $a, int $b): int {\n  return $a + $b;\n}\n");
    let PhpItem::Function(f) = &p.items[0] else {
        panic!("expected function, got {:?}", p.items[0]);
    };
    assert_eq!(f.name, "add");
    assert_eq!(f.params.len(), 2);
    assert_eq!(f.params[0].name, "a");
    assert_eq!(f.params[0].ty, Some(PhpType::Named("int".into())));
    assert_eq!(f.ret, Some(PhpType::Named("int".into())));
    assert_eq!(f.line, 2);
    assert_eq!(
        f.body,
        vec![PhpStmt::Return(Some(PhpExpr::Binary {
            op: PhpBinOp::Add,
            left: Box::new(PhpExpr::Var("a".into())),
            right: Box::new(PhpExpr::Var("b".into())),
        }))]
    );
}

#[test]
fn nullable_and_default_params() {
    let p = parse("<?php function f(?string $s, int $n = 7) {}");
    let PhpItem::Function(f) = &p.items[0] else {
        panic!()
    };
    assert_eq!(
        f.params[0].ty,
        Some(PhpType::Nullable(Box::new(PhpType::Named("string".into()))))
    );
    assert_eq!(f.params[1].default, Some(PhpExpr::Int(7)));
    assert_eq!(f.ret, None);
}

#[test]
fn php8_concat_binds_below_additive_above_comparison() {
    // `1 + 2 . "x"` ≡ `(1 + 2) . "x"` — concat is looser than `+`.
    let e = expr(r#"1 + 2 . "x""#);
    let PhpExpr::Binary {
        op: PhpBinOp::Concat,
        left,
        ..
    } = e
    else {
        panic!("top op should be Concat, got {e:?}");
    };
    assert!(matches!(
        *left,
        PhpExpr::Binary {
            op: PhpBinOp::Add,
            ..
        }
    ));

    // `"a" . $b == $c` ≡ `("a" . $b) == $c` — equality is looser than concat.
    let e2 = expr(r#""a" . $b == $c"#);
    let PhpExpr::Binary {
        op: PhpBinOp::Eq,
        left,
        ..
    } = e2
    else {
        panic!("top op should be Eq, got {e2:?}");
    };
    assert!(matches!(
        *left,
        PhpExpr::Binary {
            op: PhpBinOp::Concat,
            ..
        }
    ));
}

#[test]
fn multiplicative_binds_tighter_than_additive() {
    // `1 + 2 * 3` ≡ `1 + (2 * 3)`.
    let e = expr("1 + 2 * 3");
    let PhpExpr::Binary {
        op: PhpBinOp::Add,
        right,
        ..
    } = e
    else {
        panic!("{e:?}");
    };
    assert!(matches!(
        *right,
        PhpExpr::Binary {
            op: PhpBinOp::Mul,
            ..
        }
    ));
}

#[test]
fn coalesce_is_right_associative() {
    // `$a ?? $b ?? $c` ≡ `$a ?? ($b ?? $c)`.
    let e = expr("$a ?? $b ?? $c");
    let PhpExpr::Binary {
        op: PhpBinOp::Coalesce,
        right,
        ..
    } = e
    else {
        panic!("{e:?}");
    };
    assert!(matches!(
        *right,
        PhpExpr::Binary {
            op: PhpBinOp::Coalesce,
            ..
        }
    ));
}

#[test]
fn ternary_and_elvis() {
    let e = expr("$a ? $b : $c");
    let PhpExpr::Ternary { then, .. } = &e else {
        panic!("{e:?}");
    };
    assert!(then.is_some());

    let e2 = expr("$a ?: $c");
    let PhpExpr::Ternary { then, .. } = &e2 else {
        panic!("{e2:?}");
    };
    assert!(then.is_none(), "elvis arm should be None");
}

#[test]
fn assignment_is_right_associative() {
    let e = expr("$a = $b = 1");
    let PhpExpr::Assign { target, value } = e else {
        panic!("{e:?}");
    };
    assert_eq!(*target, PhpExpr::Var("a".into()));
    assert!(matches!(*value, PhpExpr::Assign { .. }));
}

#[test]
fn compound_assign_and_incdec() {
    assert!(matches!(
        expr("$n += 5"),
        PhpExpr::CompoundAssign {
            op: PhpBinOp::Add,
            ..
        }
    ));
    assert!(matches!(
        expr("$s .= $t"),
        PhpExpr::CompoundAssign {
            op: PhpBinOp::Concat,
            ..
        }
    ));
    assert!(matches!(
        expr("$x ??= 0"),
        PhpExpr::CompoundAssign {
            op: PhpBinOp::Coalesce,
            ..
        }
    ));
    assert!(matches!(
        expr("$i++"),
        PhpExpr::IncDec {
            inc: true,
            prefix: false,
            ..
        }
    ));
    assert!(matches!(
        expr("--$j"),
        PhpExpr::IncDec {
            inc: false,
            prefix: true,
            ..
        }
    ));
}

#[test]
fn postfix_call_method_static_member_index() {
    assert!(matches!(expr("foo(1, 2)"), PhpExpr::Call { .. }));
    assert!(matches!(
        expr("$o->run(3)"),
        PhpExpr::MethodCall {
            nullsafe: false,
            ..
        }
    ));
    assert!(matches!(
        expr("$o?->name"),
        PhpExpr::Member { nullsafe: true, .. }
    ));
    assert!(matches!(expr("Limits::MAX"), PhpExpr::ClassConst { .. }));
    assert!(matches!(expr("Math::abs(-1)"), PhpExpr::StaticCall { .. }));
    assert!(matches!(expr("Reg::$count"), PhpExpr::StaticProp { .. }));
    assert!(matches!(expr("$xs[0]"), PhpExpr::Index { .. }));
    // chained: $a->b()->c[0]
    assert!(matches!(expr("$a->b()->c[0]"), PhpExpr::Index { .. }));
}

#[test]
fn new_with_and_without_parens() {
    assert!(matches!(expr("new Point(1, 2)"), PhpExpr::New { .. }));
    let e = expr("new Empty");
    let PhpExpr::New { class, args } = e else {
        panic!("{e:?}");
    };
    assert_eq!(class, "Empty");
    assert!(args.is_empty());
}

#[test]
fn array_literal_list_and_map() {
    let e = expr(r#"[1, 2, "k" => 3,]"#);
    let PhpExpr::Array(elems) = e else {
        panic!("{e:?}")
    };
    assert_eq!(elems.len(), 3);
    assert!(elems[0].key.is_none());
    assert_eq!(elems[2].key, Some(PhpExpr::Str("k".into())));
    // `array(...)` long form parses as a call to `array`.
    assert!(matches!(expr("array(1, 2)"), PhpExpr::Call { .. }));
}

#[test]
fn match_with_multi_cond_and_default() {
    let e = expr(r#"match ($x) { 1, 2 => "a", default => "z", }"#);
    let PhpExpr::Match { arms, .. } = e else {
        panic!("{e:?}");
    };
    assert_eq!(arms.len(), 2);
    assert_eq!(arms[0].conds.as_ref().map(Vec::len), Some(2));
    assert!(arms[1].conds.is_none(), "default arm");
}

#[test]
fn literals_true_false_null() {
    assert_eq!(expr("true"), PhpExpr::Bool(true));
    assert_eq!(expr("false"), PhpExpr::Bool(false));
    assert_eq!(expr("null"), PhpExpr::Null);
}

#[test]
fn if_elseif_else_and_else_if() {
    let PhpStmt::If { elifs, els, .. } =
        stmt("<?php if ($a) { return 1; } elseif ($b) { return 2; } else { return 3; }")
    else {
        panic!()
    };
    assert_eq!(elifs.len(), 1);
    assert!(els.is_some());

    // `else if` (two words) folds into an elif.
    let PhpStmt::If { elifs, .. } = stmt("<?php if ($a) {} else if ($b) {}") else {
        panic!()
    };
    assert_eq!(elifs.len(), 1);
}

#[test]
fn braceless_if_body() {
    let PhpStmt::If { then, .. } = stmt("<?php if ($a) return 1;") else {
        panic!()
    };
    assert_eq!(then, vec![PhpStmt::Return(Some(PhpExpr::Int(1)))]);
}

#[test]
fn for_loop_with_incdec_step() {
    let PhpStmt::For {
        init, cond, step, ..
    } = stmt("<?php for ($i = 0; $i < 10; $i++) { echo $i; }")
    else {
        panic!()
    };
    assert!(init.is_some() && cond.is_some());
    assert!(matches!(step, Some(PhpExpr::IncDec { .. })));
}

#[test]
fn empty_for_clauses() {
    let PhpStmt::For {
        init, cond, step, ..
    } = stmt("<?php for (;;) { break; }")
    else {
        panic!()
    };
    assert!(init.is_none() && cond.is_none() && step.is_none());
}

#[test]
fn foreach_value_and_keyvalue() {
    let PhpStmt::Foreach { key, value, .. } = stmt("<?php foreach ($xs as $v) {}") else {
        panic!()
    };
    assert_eq!(key, None);
    assert_eq!(value, "v");

    let PhpStmt::Foreach { key, value, .. } = stmt("<?php foreach ($m as $k => $v) {}") else {
        panic!()
    };
    assert_eq!(key, Some("k".into()));
    assert_eq!(value, "v");
}

#[test]
fn while_and_echo_multi() {
    assert!(matches!(
        stmt("<?php while ($a) { $a = false; }"),
        PhpStmt::While { .. }
    ));
    let PhpStmt::Echo(args) = stmt(r#"<?php echo $a, "b", 3;"#) else {
        panic!()
    };
    assert_eq!(args.len(), 3);
}

#[test]
fn top_level_statements_and_functions_interleave() {
    let p = parse("<?php $x = 1; function f() { return $x; } echo f();");
    assert!(matches!(p.items[0], PhpItem::Stmt(PhpStmt::Expr(_))));
    assert!(matches!(p.items[1], PhpItem::Function(_)));
    assert!(matches!(p.items[2], PhpItem::Stmt(PhpStmt::Echo(_))));
}

// ── loud rejection of out-of-tier constructs (never silently misparse) ──

#[test]
fn rejects_operator_in_string_interpolation() {
    // C-1: simple interpolation now parses; a top-level operator inside `{$…}` stays Tier-2
    // (it is also a PHP parse error — the faithful subset is an access chain only).
    let e = perr(r#"<?php $a = "{$x + $y}";"#);
    assert!(e.contains("access chain"), "{e}");
}

#[test]
fn rejects_unsupported_keywords() {
    for (src, frag) in [
        (
            "<?php try { foo(); } catch (E $e) {}",
            "`try` is not supported",
        ),
        ("<?php switch ($x) {}", "`switch` is not supported"),
        ("<?php throw new E();", "`throw` is not supported"),
        ("<?php namespace App;", "`namespace` is not supported"),
        ("<?php interface I {}", "`interface` is not supported"),
        ("<?php trait T {}", "`trait` is not supported"),
    ] {
        let e = perr(src);
        assert!(e.contains(frag), "for {src:?} got {e}");
    }
}

#[test]
fn rejects_closures_and_arrow_fns() {
    assert!(perr("<?php $f = function () { return 1; };").contains("closures"));
    assert!(perr("<?php $f = fn ($x) => $x;").contains("closures"));
}

#[test]
fn rejects_cast_and_dynamic_new() {
    assert!(perr("<?php $n = (int) $s;").contains("cast expressions are Tier-2"));
    assert!(perr("<?php $o = new $cls();").contains("dynamic `new $class` is Tier-3"));
}

#[test]
fn rejects_array_append_and_dynamic_static() {
    assert!(perr("<?php $a[] = 1;").contains("array append) is Tier-2"));
    assert!(perr("<?php $x = $obj::FOO;").contains("dynamic `::` access is Tier-3"));
}

#[test]
fn rejects_invalid_assignment_target() {
    assert!(perr("<?php 1 = $x;").contains("invalid assignment target"));
}

// ── L2b: classes + enums ──

fn class(src: &str) -> PhpClass {
    match parse(src).items.into_iter().next().expect("one item") {
        PhpItem::Class(c) => c,
        other => panic!("expected a class, got {other:?}"),
    }
}

#[test]
fn parses_class_with_props_methods_and_modifiers() {
    let c = class(
        "<?php class Account {\n\
           private int $balance = 0;\n\
           public static string $kind = \"basic\";\n\
           const LIMIT = 1000;\n\
           public function deposit(int $n): void { $this->balance += $n; }\n\
           private function audit() {}\n\
         }",
    );
    assert_eq!(c.name, "Account");
    assert!(!c.is_abstract && !c.is_final);
    assert_eq!(c.members.len(), 5);
    let PhpMember::Prop {
        vis,
        is_static,
        ty,
        default,
        ..
    } = &c.members[0]
    else {
        panic!("first member should be a prop: {:?}", c.members[0]);
    };
    assert_eq!(*vis, PhpVisibility::Private);
    assert!(!is_static);
    assert_eq!(*ty, Some(PhpType::Named("int".into())));
    assert_eq!(*default, Some(PhpExpr::Int(0)));
    assert!(matches!(
        &c.members[1],
        PhpMember::Prop {
            is_static: true,
            vis: PhpVisibility::Public,
            ..
        }
    ));
    assert!(matches!(&c.members[2], PhpMember::Const { .. }));
    assert!(matches!(
        &c.members[3],
        PhpMember::Method(m) if m.name == "deposit" && m.vis == PhpVisibility::Public
    ));
}

#[test]
fn parses_abstract_class_extends_implements_and_abstract_method() {
    let c = class(
        "<?php abstract class Shape extends Base implements Drawable, Named {\n\
           abstract public function area(): float;\n\
         }",
    );
    assert!(c.is_abstract);
    assert_eq!(c.extends, Some("Base".into()));
    assert_eq!(
        c.implements,
        vec!["Drawable".to_string(), "Named".to_string()]
    );
    let PhpMember::Method(m) = &c.members[0] else {
        panic!()
    };
    assert!(m.is_abstract);
    assert_eq!(m.body, None, "abstract method has no body");
    assert_eq!(m.ret, Some(PhpType::Named("float".into())));
}

#[test]
fn final_class_and_constructor_promotion() {
    let c = class(
        "<?php final class Point {\n\
           public function __construct(public int $x, private readonly int $y = 0) {}\n\
         }",
    );
    assert!(c.is_final);
    let PhpMember::Method(m) = &c.members[0] else {
        panic!()
    };
    assert_eq!(m.name, "__construct");
    assert_eq!(m.params[0].promotion, Some(PhpVisibility::Public));
    assert_eq!(m.params[1].promotion, Some(PhpVisibility::Private));
    assert_eq!(m.params[1].default, Some(PhpExpr::Int(0)));
}

#[test]
fn parses_backed_enum_with_cases_and_method() {
    let p = parse(
        "<?php enum Suit: string implements HasLabel {\n\
           case Hearts = \"H\";\n\
           case Spades = \"S\";\n\
           public function label(): string { return \"suit\"; }\n\
         }",
    );
    let PhpItem::Enum(e) = &p.items[0] else {
        panic!("expected enum, got {:?}", p.items[0]);
    };
    assert_eq!(e.name, "Suit");
    assert_eq!(e.backing, Some(PhpType::Named("string".into())));
    assert_eq!(e.implements, vec!["HasLabel".to_string()]);
    assert_eq!(e.cases.len(), 2);
    assert_eq!(e.cases[0].name, "Hearts");
    assert_eq!(e.cases[0].value, Some(PhpExpr::Str("H".into())));
    assert_eq!(e.methods.len(), 1);
    assert_eq!(e.methods[0].name, "label");
}

#[test]
fn parses_pure_enum() {
    let p = parse("<?php enum Dir { case Up; case Down; }");
    let PhpItem::Enum(e) = &p.items[0] else {
        panic!()
    };
    assert_eq!(e.backing, None);
    assert_eq!(e.cases.len(), 2);
    assert!(e.cases[0].value.is_none());
}

#[test]
fn rejects_non_case_non_method_in_enum() {
    let e = perr("<?php enum E { public int $x; }");
    assert!(
        e.contains("an enum may only contain cases and methods"),
        "{e}"
    );
}

#[test]
fn rejects_deeply_nested_expression() {
    // A pathological paren nest must hit the depth guard (MAX_NEST_DEPTH = 512) and error cleanly
    // rather than recurse forever. Production parses run on a large worker stack (`on_deep_stack`,
    // 256 MiB); the 2 MiB test-harness stack would itself overflow before the guard fires, so we
    // run the parse on a big stack the same way production does.
    let src = format!("<?php $x = {}1{};", "(".repeat(600), ")".repeat(600));
    let e = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || perr(&src))
        .expect("spawn")
        .join()
        .expect("join");
    assert!(e.contains("nests too deeply"), "{e}");
}
