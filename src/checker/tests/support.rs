//! Shared checker-test helpers (M-Decomp W2b). Re-exports the checker internals so each
//! by-feature test file needs only `use super::support::*;`.

pub(super) use super::super::*;
use crate::lexer::lex;
use crate::parser::Parser;

/// Lex + parse `src` into a Program, panicking on lex/parse failure (tests here only care
/// about type-checking). Auto-prepends the reserved `package Main;` (M5 S1, line-preserving)
/// unless the source already declares a package, so existing checker tests need no per-case
/// edit. Use [`prog_raw`] when a test must exercise the package rules themselves.
pub(super) fn prog(src: &str) -> Program {
    let src = if src.trim_start().starts_with("package ") {
        src.to_string()
    } else {
        format!("package Main; {src}")
    };
    prog_raw(&src)
}

/// Lex + parse without injecting a package — for tests of the package rules themselves.
pub(super) fn prog_raw(src: &str) -> Program {
    let tokens = lex(src).expect("lex ok");
    Parser::new(tokens).parse_program().expect("parse ok")
}

/// Type-check `src` and return the errors (empty == well-typed).
pub(super) fn errors_of(src: &str) -> Vec<Diagnostic> {
    match check(&prog(src)) {
        Ok(_warnings) => Vec::new(),
        Err(e) => e,
    }
}

/// Type-check `src` and return the non-fatal warnings (empty unless a lint fired).
pub(super) fn warnings_of(src: &str) -> Vec<Diagnostic> {
    check(&prog(src)).unwrap_or_default()
}

/// Type-check a *raw* source (no injected package) and return the errors.
pub(super) fn errors_of_raw(src: &str) -> Vec<Diagnostic> {
    match check(&prog_raw(src)) {
        Ok(_) => Vec::new(),
        Err(e) => e,
    }
}

/// Recursively scan a checked program for any surviving `Expr::Propagate` node (test helper for
/// the throws-`?` erasure invariant).
pub(super) fn program_has_propagate(p: &Program) -> bool {
    use crate::ast::{ClassMember, Expr, Item, LambdaBody, Stmt};
    fn in_expr(e: &Expr) -> bool {
        match e {
            Expr::Propagate { .. } => true,
            Expr::Unary { expr, .. } | Expr::Force { inner: expr, .. } => in_expr(expr),
            Expr::Binary { lhs, rhs, .. } => in_expr(lhs) || in_expr(rhs),
            Expr::Call { callee, args, .. } => in_expr(callee) || args.iter().any(in_expr),
            Expr::Member { object, .. } => in_expr(object),
            Expr::Index { object, index, .. } => in_expr(object) || in_expr(index),
            Expr::List(items, _) => items.iter().any(in_expr),
            Expr::Match {
                scrutinee, arms, ..
            } => in_expr(scrutinee) || arms.iter().any(|a| in_expr(&a.body)),
            Expr::Lambda { body, .. } => match body {
                LambdaBody::Expr(e) => in_expr(e),
                LambdaBody::Block(b) => b.iter().any(in_stmt),
            },
            _ => false,
        }
    }
    fn in_stmt(s: &Stmt) -> bool {
        match s {
            Stmt::Expr(e, _) | Stmt::Throw { value: e, .. } => in_expr(e),
            Stmt::VarDecl { init, .. } => in_expr(init),
            Stmt::Return { value: Some(e), .. } => in_expr(e),
            Stmt::Block(b, _) => b.iter().any(in_stmt),
            Stmt::Try {
                body,
                catches,
                finally_block,
                ..
            } => {
                body.iter().any(in_stmt)
                    || catches.iter().any(|c| c.body.iter().any(in_stmt))
                    || finally_block
                        .as_ref()
                        .is_some_and(|fb| fb.iter().any(in_stmt))
            }
            _ => false,
        }
    }
    fn in_fn(body: &[Stmt]) -> bool {
        body.iter().any(in_stmt)
    }
    p.items.iter().any(|it| match it {
        Item::Function(f) => in_fn(&f.body),
        Item::Class(c) => c.members.iter().any(|m| match m {
            ClassMember::Method(f) => in_fn(&f.body),
            ClassMember::Constructor { body, .. } => in_fn(body),
            _ => false,
        }),
        _ => false,
    })
}

// Shared multi-test fixtures (centralized from the flat tests.rs — M-Decomp W2b).
pub(super) const OPTION: &str = "enum Option<T> { None, Some(T value) }";
pub(super) const RESULT: &str = "enum Result<T, E> { Ok(T value), Err(E error) }";
pub(super) const RESULT_DEF: &str = "enum Result<T, E> { Ok(T value), Err(E error) }";
pub(super) const ERRDEF: &str =
    "class BadInput implements Error { constructor(public string message) {} } \
         class NotFound implements Error { constructor(public string message) {} }";
pub(super) const SHAPES: &str = "class Circle { constructor(public int radius) {} } \
        class Square { constructor(public int side) {} } \
        class Triangle { constructor(public int base) {} }";
pub(super) const IFACES: &str = "interface Drawable { function draw() -> string; } \
        interface Named { function name() -> string; } \
        class Badge implements Drawable, Named { \
            constructor(public string label) {} \
            function draw() -> string { return \"[]\"; } \
            function name() -> string { return this.label; } }";
pub(super) const SHAPE: &str = "enum Shape { Circle(float radius), Rect(float w, float h), }";
pub(super) const GREETER: &str = "class Greeter { private string name; constructor(string name) {} function greet() -> string { return \"Hi\"; } }";
