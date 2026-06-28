//! Feature C — strip every `Expr::New` to its inner construction `Call` *before any backend runs*.
//!
//! `new` is a parser-required, checker-validated keyword that carries no runtime meaning: once the
//! checker has confirmed every construction is `new`-wrapped (`E-NEW-REQUIRED`) and every `new`
//! wraps a real construction (`E-NEW-ON-NONCONSTRUCT`), the wrapper is erased here so the
//! interpreter, compiler, and transpiler see exactly today's AST — construction semantics and the
//! byte-identity spine are unchanged. Same "front-end sugar, expanded out" discipline as `type`
//! aliases / `html"…"` / erased generics. Runs in `cli::check_and_expand` after the other passes.

use super::*;
use crate::ast::{CatchClause, ClassMember, Expr, Item, LambdaBody, MatchArm, Stmt, StrPart};
use crate::token::Span;

/// Default-null injection for optional instance fields (Soundness Batch D, DEFAULT-NULL policy). An
/// optional field with no initializer (`int? n;`) reads as `null` rather than faulting "no field n":
/// inject an explicit `= null` initializer so the existing field-initializer machinery sets it at
/// construction on every backend — a front-end desugar, so the interpreter, VM, and transpiled PHP all
/// initialize it identically (byte-identity safe, no backend change). Runs after `expand_aliases`, so a
/// field typed via an alias to an optional is already `Type::Optional` here. A constructor that *does*
/// assign the field still overrides this default (field initializers run before the ctor body).
pub fn inject_optional_field_defaults(mut program: Program) -> Program {
    for item in &mut program.items {
        let members = match item {
            Item::Class(c) => &mut c.members,
            Item::Trait(t) => &mut t.members,
            _ => continue,
        };
        for m in members {
            if let ClassMember::Field {
                modifiers,
                ty,
                init: init @ None,
                span,
                ..
            } = m
            {
                use crate::ast::Modifier;
                let instance =
                    !modifiers.contains(&Modifier::Static) && !modifiers.contains(&Modifier::Const);
                if instance && matches!(ty, crate::ast::Type::Optional { .. }) {
                    *init = Some(Expr::Null(*span));
                }
            }
        }
    }
    program
}

/// Erase every `Expr::New(inner, _)` to `*inner` throughout the program (in place).
pub fn unwrap_new(mut program: Program) -> Program {
    for item in &mut program.items {
        match item {
            Item::Function(f) => ue_block(&mut f.body),
            Item::Class(c) => ue_members(&mut c.members),
            Item::Trait(t) => ue_members(&mut t.members),
            // Enums/interfaces/imports/aliases carry no expressions to rewrite.
            _ => {}
        }
    }
    program
}

fn ue_members(members: &mut [ClassMember]) {
    for m in members {
        match m {
            ClassMember::Method(f) => ue_block(&mut f.body),
            ClassMember::Constructor { body, .. } => ue_block(body),
            ClassMember::Field { init, .. } => {
                if let Some(e) = init {
                    ue_expr(e);
                }
            }
            ClassMember::Hook { get, set, .. } => {
                if let Some(g) = get {
                    ue_expr(g);
                }
                if let Some((_, body)) = set {
                    ue_block(body);
                }
            }
        }
    }
}

fn ue_block(stmts: &mut [Stmt]) {
    for s in stmts {
        ue_stmt(s);
    }
}

fn ue_stmt(s: &mut Stmt) {
    match s {
        Stmt::VarDecl { init, .. } => ue_expr(init),
        Stmt::Assign { target, value, .. } => {
            ue_expr(target);
            ue_expr(value);
        }
        Stmt::Return { value, .. } => {
            if let Some(e) = value {
                ue_expr(e);
            }
        }
        Stmt::If {
            cond,
            then_block,
            else_block,
            ..
        } => {
            ue_expr(cond);
            ue_block(then_block);
            if let Some(b) = else_block {
                ue_block(b);
            }
        }
        Stmt::For { iter, body, .. } => {
            ue_expr(iter);
            ue_block(body);
        }
        Stmt::While { cond, body, .. } => {
            ue_expr(cond);
            ue_block(body);
        }
        Stmt::CFor {
            init,
            cond,
            step,
            body,
            ..
        } => {
            if let Some(i) = init {
                ue_stmt(i);
            }
            if let Some(c) = cond {
                ue_expr(c);
            }
            if let Some(st) = step {
                ue_stmt(st);
            }
            ue_block(body);
        }
        Stmt::Block(b, _) => ue_block(b),
        // Slice 5: unwrap `new` in the destructured initializer and the `else` block.
        Stmt::Destructure {
            init, else_block, ..
        } => {
            ue_expr(init);
            if let Some(eb) = else_block {
                ue_block(eb);
            }
        }
        Stmt::Expr(e, _) | Stmt::Discard(e, _) | Stmt::Throw { value: e, .. } => ue_expr(e),
        Stmt::Try {
            body,
            catches,
            finally_block,
            ..
        } => {
            ue_block(body);
            for CatchClause { body, .. } in catches {
                ue_block(body);
            }
            if let Some(fb) = finally_block {
                ue_block(fb);
            }
        }
        Stmt::Break(_) | Stmt::Continue(_) => {}
    }
}

fn ue_expr(e: &mut Expr) {
    // First rewrite the children, then unwrap this node if it is a `New` (its inner is now clean).
    match e {
        Expr::Unary { expr, .. } => ue_expr(expr),
        Expr::Force { inner, .. } | Expr::Propagate { inner, .. } => ue_expr(inner),
        Expr::Binary { lhs, rhs, .. } => {
            ue_expr(lhs);
            ue_expr(rhs);
        }
        Expr::InstanceOf { value, .. } | Expr::Cast { value, .. } => ue_expr(value),
        Expr::Call { callee, args, .. } => {
            ue_expr(callee);
            for a in args {
                ue_expr(a);
            }
        }
        Expr::Member { object, .. } => ue_expr(object),
        Expr::Index { object, index, .. } => {
            ue_expr(object);
            ue_expr(index);
        }
        Expr::Str(parts, _) | Expr::Html(parts, _) => {
            for p in parts {
                if let StrPart::Expr(x) = p {
                    ue_expr(x);
                }
            }
        }
        Expr::List(xs, _) => {
            for x in xs {
                ue_expr(x);
            }
        }
        Expr::Map(ps, _) => {
            for (k, v) in ps {
                ue_expr(k);
                ue_expr(v);
            }
        }
        Expr::Match {
            scrutinee, arms, ..
        } => {
            ue_expr(scrutinee);
            for MatchArm { guard, body, .. } in arms {
                if let Some(g) = guard {
                    ue_expr(g);
                }
                ue_expr(body);
            }
        }
        Expr::Range { start, end, .. } => {
            ue_expr(start);
            ue_expr(end);
        }
        Expr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            ue_expr(cond);
            ue_expr(then_expr);
            ue_expr(else_expr);
        }
        Expr::Lambda { body, .. } => match body {
            LambdaBody::Expr(x) => ue_expr(x),
            LambdaBody::Block(b) => ue_block(b),
        },
        Expr::CloneWith { object, fields, .. } => {
            ue_expr(object);
            for (_, v) in fields {
                ue_expr(v);
            }
        }
        Expr::New(inner, _) => ue_expr(inner),
        // Literals / `Ident` / `This` have no sub-expressions.
        _ => {}
    }
    if let Expr::New(inner, span) = e {
        let s: Span = *span;
        let taken = std::mem::replace(inner.as_mut(), Expr::Null(s));
        *e = taken;
    }
}
