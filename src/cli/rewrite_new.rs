//! Feature C migration tool — `phg rewrite-new <file>`: rewrite every class / enum-variant
//! construction `Name(args)` to `new Name(args)` in place, using the parsed AST so that **match
//! patterns** (which are `Pattern` nodes, never `Expr::Call`) and **free-function / native calls**
//! are left untouched, and an already-`new`-wrapped construction is not double-wrapped (idempotent).
//!
//! It parses (pre-checker, so bare constructions still parse as plain `Call`s), collects the program's
//! class + enum-variant names, walks every expression collecting the byte offset of each construction
//! call's callee, then inserts `new ` at those offsets right-to-left. Not part of the language — a
//! one-shot dev command for the breaking migration.

use crate::ast::{CatchClause, ClassMember, Expr, Item, LambdaBody, Stmt};
use std::collections::HashSet;

pub fn cmd_rewrite_new(path: &str) -> Result<String, String> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let program = crate::cli::parse_program(&src)?;

    // Construction names: every class + every enum variant.
    let mut names: HashSet<String> = HashSet::new();
    for it in &program.items {
        match it {
            Item::Class(c) => {
                names.insert(c.name.clone());
            }
            Item::Enum(e) => {
                for v in &e.variants {
                    names.insert(v.name.clone());
                }
            }
            _ => {}
        }
    }

    let mut offsets: Vec<usize> = Vec::new();
    let mut w = Walker {
        names: &names,
        offsets: &mut offsets,
    };
    for it in &program.items {
        match it {
            Item::Function(f) => w.block(&f.body),
            Item::Class(c) => w.members(&c.members),
            Item::Trait(t) => w.members(&t.members),
            _ => {}
        }
    }

    // Insert `new ` at each construction callee offset, right-to-left so earlier offsets stay valid.
    offsets.sort_unstable();
    offsets.dedup();
    let mut bytes = src.into_bytes();
    for &off in offsets.iter().rev() {
        bytes.splice(off..off, b"new ".iter().copied());
    }
    let out = String::from_utf8(bytes).map_err(|e| format!("{path}: {e}"))?;
    std::fs::write(path, &out).map_err(|e| format!("{path}: {e}"))?;
    Ok(format!(
        "rewrote {} construction site(s) in {path}\n",
        offsets.len()
    ))
}

struct Walker<'a> {
    names: &'a HashSet<String>,
    offsets: &'a mut Vec<usize>,
}

impl Walker<'_> {
    fn members(&mut self, members: &[ClassMember]) {
        for m in members {
            match m {
                ClassMember::Method(f) => self.block(&f.body),
                ClassMember::Constructor { body, .. } => self.block(body),
                ClassMember::Field { init: Some(e), .. } => self.expr(e),
                ClassMember::Field { .. } => {}
                ClassMember::Hook { get, set, .. } => {
                    if let Some(g) = get {
                        self.expr(g);
                    }
                    if let Some((_, body)) = set {
                        self.block(body);
                    }
                }
            }
        }
    }

    fn block(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::VarDecl { init, .. } => self.expr(init),
            Stmt::Assign { target, value, .. } => {
                self.expr(target);
                self.expr(value);
            }
            Stmt::Return { value, .. } => {
                if let Some(e) = value {
                    self.expr(e);
                }
            }
            Stmt::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                self.expr(cond);
                self.block(then_block);
                if let Some(b) = else_block {
                    self.block(b);
                }
            }
            Stmt::For { iter, body, .. } => {
                self.expr(iter);
                self.block(body);
            }
            Stmt::While { cond, body, .. } => {
                self.expr(cond);
                self.block(body);
            }
            Stmt::CFor {
                init,
                cond,
                step,
                body,
                ..
            } => {
                if let Some(i) = init {
                    self.stmt(i);
                }
                if let Some(c) = cond {
                    self.expr(c);
                }
                if let Some(st) = step {
                    self.stmt(st);
                }
                self.block(body);
            }
            Stmt::Block(b, _) => self.block(b),
            Stmt::Expr(e, _) | Stmt::Discard(e, _) | Stmt::Throw { value: e, .. } => self.expr(e),
            Stmt::Try {
                body,
                catches,
                finally_block,
                ..
            } => {
                self.block(body);
                for CatchClause { body, .. } in catches {
                    self.block(body);
                }
                if let Some(fb) = finally_block {
                    self.block(fb);
                }
            }
            // Slice 5: scan the destructured init and the `else` block for bare constructions.
            Stmt::Destructure {
                init, else_block, ..
            } => {
                self.expr(init);
                if let Some(eb) = else_block {
                    self.block(eb);
                }
            }
            Stmt::Break(_) | Stmt::Continue(_) => {}
        }
    }

    fn expr(&mut self, e: &Expr) {
        match e {
            // A construction call `Name(args)`: record the callee's start offset (where `new ` goes),
            // then recurse into the arguments (a nested bare construction needs its own `new`).
            Expr::Call { callee, args, .. } => {
                if let Expr::Ident(name, span) = &**callee {
                    if self.names.contains(name) {
                        self.offsets.push(span.start);
                    }
                }
                self.expr(callee);
                for a in args {
                    self.expr(a);
                }
            }
            // Already `new`-wrapped: do NOT record the inner call (idempotent), but still recurse into
            // its arguments for any unwrapped nested constructions.
            Expr::New(inner, _) => {
                if let Expr::Call { callee, args, .. } = &**inner {
                    self.expr(callee);
                    for a in args {
                        self.expr(a);
                    }
                } else {
                    self.expr(inner);
                }
            }
            Expr::Unary { expr, .. } => self.expr(expr),
            Expr::Force { inner, .. } | Expr::Propagate { inner, .. } => self.expr(inner),
            Expr::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            Expr::InstanceOf { value, .. } | Expr::Cast { value, .. } => self.expr(value),
            Expr::Member { object, .. } => self.expr(object),
            Expr::Index { object, index, .. } => {
                self.expr(object);
                self.expr(index);
            }
            // NOTE: deliberately do NOT recurse into string-interpolation inner expressions — a
            // re-lexed interpolation hole carries spans *relative to the interpolation substring*, not
            // the file, so splicing at them would corrupt the source. A construction inside `"{…}"` is
            // rare; it surfaces as `E-NEW-REQUIRED` and is hand-fixed.
            Expr::Str(_, _) | Expr::Html(_, _) => {}
            Expr::List(xs, _) => {
                for x in xs {
                    self.expr(x);
                }
            }
            Expr::Map(ps, _) => {
                for (k, v) in ps {
                    self.expr(k);
                    self.expr(v);
                }
            }
            Expr::Match {
                scrutinee, arms, ..
            } => {
                self.expr(scrutinee);
                for a in arms {
                    if let Some(g) = &a.guard {
                        self.expr(g);
                    }
                    self.expr(&a.body);
                }
            }
            Expr::Range { start, end, .. } => {
                self.expr(start);
                self.expr(end);
            }
            Expr::If {
                cond,
                then_expr,
                else_expr,
                ..
            } => {
                self.expr(cond);
                self.expr(then_expr);
                self.expr(else_expr);
            }
            Expr::Lambda { body, .. } => match body {
                LambdaBody::Expr(x) => self.expr(x),
                LambdaBody::Block(b) => self.block(b),
            },
            Expr::CloneWith { object, fields, .. } => {
                self.expr(object);
                for (_, v) in fields {
                    self.expr(v);
                }
            }
            _ => {}
        }
    }
}
