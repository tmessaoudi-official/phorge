//! Parser tests — stmts (M-Decomp W3.1b, mirrors the source clusters).

use super::support::*;

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
    match stmt("Output.printLine(x);") {
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
fn parses_if_let_when_guard_desugars_to_nested_if() {
    // `if (var u = e when g) THEN else ELSE` desugars (S5.3) to
    // `if (var u = e) { if (g) THEN else ELSE } else ELSE` — no `Stmt::If.guard` field. The outer
    // keeps the binding; its then-block is the single nested `if` over the guard.
    match stmt("if (var u = lookup() when u.active) { return 1; } else { return 2; }") {
        Stmt::If {
            bind: Some(name),
            then_block,
            else_block: Some(_),
            ..
        } => {
            assert_eq!(name, "u");
            assert_eq!(
                then_block.len(),
                1,
                "then-block is the single nested guard if"
            );
            assert!(
                matches!(&then_block[0], Stmt::If { bind: None, .. }),
                "nested guard if: {:?}",
                then_block[0]
            );
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_for_in() {
    match stmt("for (Shape s in shapes) { Output.printLine(s); }") {
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
fn parses_struct_destructure() {
    match stmt("var Point { x, y: row } = p;") {
        Stmt::Destructure {
            pat:
                crate::ast::DestructurePat::Struct {
                    type_name, fields, ..
                },
            else_block: None,
            ..
        } => {
            assert_eq!(type_name, "Point");
            assert_eq!(fields.len(), 2);
            assert_eq!(fields[0].field, "x");
            assert_eq!(fields[0].binding, "x"); // shorthand
            assert_eq!(fields[1].field, "y");
            assert_eq!(fields[1].binding, "row"); // rename
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_list_destructure_with_else() {
    match stmt("var [a, b] = xs else { return; }") {
        Stmt::Destructure {
            pat: crate::ast::DestructurePat::List { binders, .. },
            else_block: Some(eb),
            ..
        } => {
            assert_eq!(binders.len(), 2);
            assert_eq!(binders[0].0, "a");
            assert_eq!(binders[1].0, "b");
            assert_eq!(eb.len(), 1);
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_list_destructure_no_else() {
    match stmt("var [a, b] = pair;") {
        Stmt::Destructure {
            pat: crate::ast::DestructurePat::List { binders, .. },
            else_block: None,
            ..
        } => assert_eq!(binders.len(), 2),
        other => panic!("got {other:?}"),
    }
}

#[test]
fn plain_var_still_parses_after_destructure_dispatch() {
    // The `var` dispatcher must not mistake an ordinary binding for a destructure.
    assert!(matches!(stmt("var n = 5;"), Stmt::VarDecl { .. }));
}

// ── contextual `var` — a legal identifier wherever it denotes a value (the inference keyword only at
//    a binding start: `var IDENT` / `var [` / `var IDENT {`). Bug: `var` was hard-reserved. ──

#[test]
fn var_keyword_still_introduces_declarations() {
    // All the binding forms keep working unchanged (additive change).
    assert!(matches!(
        stmt("var x = 5;"),
        Stmt::VarDecl {
            ty: Type::Infer(_),
            ..
        }
    ));
    assert!(matches!(
        stmt("mutable var y = 5;"),
        Stmt::VarDecl {
            mutable: true,
            ty: Type::Infer(_),
            ..
        }
    ));
    assert!(matches!(stmt("var [a, b] = xs;"), Stmt::Destructure { .. }));
    assert!(matches!(
        stmt("var Point { x, y } = p;"),
        Stmt::Destructure { .. }
    ));
}

#[test]
fn var_is_usable_as_a_value_identifier() {
    // Expression position: `var` is just a name now.
    match expr("var + 1") {
        Expr::Binary { .. } => {}
        other => panic!("`var + 1`: {other:?}"),
    }
    // Reassignment of a variable named `var`.
    match stmt("var = 5;") {
        Stmt::Assign {
            target: Expr::Ident(n, _),
            ..
        } => assert_eq!(n, "var"),
        other => panic!("`var = 5;`: {other:?}"),
    }
    // Member / call receiver named `var`.
    match stmt("var.run();") {
        Stmt::Expr(Expr::Call { .. }, _) => {}
        other => panic!("`var.run();`: {other:?}"),
    }
    // Compound assignment + increment on a variable named `var`.
    assert!(matches!(stmt("var += 1;"), Stmt::Assign { .. }));
    assert!(matches!(stmt("var++;"), Stmt::Assign { .. }));
    // Interpolation hole referencing `var`.
    assert!(matches!(
        stmt("Output.printLine(\"{var}\");"),
        Stmt::Expr(..)
    ));
}

#[test]
fn var_is_usable_as_a_parameter_and_field_name() {
    // Parameter named `var` (the original bug).
    match item("function inc(int var): int { return var + 1; }") {
        Item::Function(_) => {}
        other => panic!("param named `var`: {other:?}"),
    }
    // Field named `var`.
    match item("class C { mutable int var = 0; }") {
        Item::Class(_) => {}
        other => panic!("field named `var`: {other:?}"),
    }
}

// ── A-6: PHP `foreach (xs as x)` — value form (inferred element type) + `with int i` counter ──

#[test]
fn parses_foreach_value_form() {
    // `foreach (xs as x) { … }` desugars to a for-in with an inferred element type.
    match stmt("foreach (xs as x) { Output.printLine(\"{x}\"); }") {
        Stmt::For { ty, name, .. } => {
            assert!(
                matches!(ty, Type::Infer(_)),
                "expected inferred elem type, got {ty:?}"
            );
            assert_eq!(name, "x");
        }
        other => panic!("expected for-in, got {other:?}"),
    }
}

#[test]
fn parses_foreach_with_counter() {
    // `foreach (xs as x with int i) { … }` desugars to a Block: a counter VarDecl + a for-in whose
    // body increments the counter.
    match stmt("foreach (xs as x with int i) { Output.printLine(\"{x}\"); }") {
        Stmt::Block(stmts, _) => {
            assert!(
                matches!(stmts[0], Stmt::VarDecl { mutable: true, .. }),
                "counter decl"
            );
            assert!(matches!(stmts[1], Stmt::For { .. }), "the loop");
        }
        other => panic!("expected a desugared block, got {other:?}"),
    }
}

#[test]
fn foreach_counter_must_be_int() {
    let e = parser("foreach (xs as x with string i) { f(); }")
        .parse_stmt()
        .unwrap_err()
        .render("");
    assert!(e.contains("counter must be typed `int`"), "{e}");
}

#[test]
fn foreach_keyvalue_form_is_rejected_for_now() {
    // `as k => v` is a documented follow-up (needs iteration-model changes); reject with guidance.
    let e = parser("foreach (m as k => v) { f(); }")
        .parse_stmt()
        .unwrap_err()
        .render("");
    assert!(
        e.contains("key/value") || e.contains("not supported yet"),
        "{e}"
    );
}
