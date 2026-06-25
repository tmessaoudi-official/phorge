//! PHP transpiler — stmt (M-Decomp W4). See mod.rs for the struct + core + entry points.

use super::*;

impl Transpiler {
    pub(super) fn emit_stmt(&mut self, s: &Stmt) -> Result<(), String> {
        match s {
            // `match` is handled at statement granularity (return / var-decl-init position).
            // These specific arms must precede the generic VarDecl/Return arms.
            Stmt::Return {
                value: Some(Expr::Match {
                    scrutinee, arms, ..
                }),
                ..
            } => {
                self.emit_match(scrutinee, arms, MatchTarget::Return)?;
            }
            Stmt::VarDecl {
                name,
                init: Expr::Match {
                    scrutinee, arms, ..
                },
                ..
            } => {
                self.declare(name);
                self.emit_match(scrutinee, arms, MatchTarget::Assign(name.clone()))?;
            }
            // `T x = expr?;` (M-faults 2a) — PHP cannot caller-return from an expression, so the `?` is
            // hoisted to statements here: stash the Result in `$x`, return it unchanged if it is `Err`,
            // else unwrap the `Ok` payload in place. The checker restricts `?` to this position, so the
            // expression-level `Expr::Propagate` arm in `emit_expr` is unreachable.
            Stmt::VarDecl {
                name,
                init: Expr::Propagate { inner, .. },
                ..
            } => {
                let v = self.emit_expr(inner)?;
                self.declare(name);
                let err = self.variant_ref("Err");
                let ok_field = self
                    .variant_fields
                    .get("Ok")
                    .and_then(|f| f.first())
                    .cloned()
                    .unwrap_or_else(|| "value".to_string());
                self.line(&format!("${name} = {v};"));
                self.line(&format!(
                    "if (${name} instanceof {err}) {{ return ${name}; }}"
                ));
                self.line(&format!("${name} = ${name}->{ok_field};"));
            }
            Stmt::VarDecl { ty, name, init, .. } => {
                let e = self.emit_expr(init)?;
                self.declare(name);
                // T6: record the local's operand kind — prefer the explicit annotation; for `var`
                // (`Type::Infer`) fall back to the initializer's resolved kind.
                let kind = match kind_of_type(ty) {
                    OpKind::Other => self.expr_kind(init),
                    k => k,
                };
                self.declare_kind(name, kind);
                self.line(&format!("${name} = {e};"));
            }
            Stmt::Assign { target, value, .. } => {
                // Reassignment (`$x = …;`) and value-type element set (`$xs[$i] = …;`, M-mut.5) share
                // one shape: the lhs is the target rendered as an expression. PHP arrays are COW value
                // types, so `$xs[$i] = $e` has the same value semantics as Phorge's `Op::SetIndex`.
                // `mutable`/immutable is erased (PHP locals are always mutable).
                let lhs = match target {
                    Expr::Ident(n, _) => format!("${n}"),
                    // Element set `$xs[$i]` (M-mut.5) and instance-field set `$o->f` (M-mut.6) both
                    // render the target as a PHP lvalue expression. PHP objects are mutable handles,
                    // so `$o->f = $e` is byte-identical to the backends' `Op::SetField`.
                    Expr::Index { .. } | Expr::Member { .. } => self.emit_expr(target)?,
                    _ => unreachable!("checker rejects other assignment targets"),
                };
                let e = self.emit_expr(value)?;
                self.line(&format!("{lhs} = {e};"));
            }
            Stmt::Return { value, .. } => match value {
                Some(e) => {
                    let s = self.emit_expr(e)?;
                    self.line(&format!("return {s};"));
                }
                None => self.line("return;"),
            },
            Stmt::If {
                cond,
                bind,
                then_block,
                else_block,
                ..
            } => {
                let c = self.emit_expr(cond)?;
                // `if (var x = opt)` → PHP `if (($x = <scrutinee>) !== null)`: the assignment-in-
                // condition binds `$x` and the `!== null` test mirrors the optional narrowing.
                match bind {
                    Some(name) => self.line(&format!("if ((${name} = {c}) !== null) {{")),
                    None => self.line(&format!("if ({c}) {{")),
                }
                self.indent += 1;
                self.push_scope();
                if let Some(name) = bind {
                    self.declare(name);
                }
                for st in then_block {
                    self.emit_stmt(st)?;
                }
                self.pop_scope();
                self.indent -= 1;
                if let Some(eb) = else_block {
                    self.line("} else {");
                    self.indent += 1;
                    self.push_scope();
                    for st in eb {
                        self.emit_stmt(st)?;
                    }
                    self.pop_scope();
                    self.indent -= 1;
                }
                self.line("}");
            }
            Stmt::For {
                ty,
                name,
                iter,
                body,
                ..
            } => {
                let it = self.emit_expr(iter)?;
                self.line(&format!("foreach ({it} as ${name}) {{"));
                self.indent += 1;
                self.push_scope();
                self.declare(name);
                // T6: the loop variable's element type drives operand specialization in the body
                // (`for (int i in 0..n) { … i / 2 … }` → native `intdiv`).
                self.declare_kind(name, kind_of_type(ty));
                for st in body {
                    self.emit_stmt(st)?;
                }
                self.pop_scope();
                self.indent -= 1;
                self.line("}");
            }
            Stmt::While {
                cond,
                body,
                post_cond,
                ..
            } => {
                if *post_cond {
                    self.line("do {");
                    self.indent += 1;
                    self.push_scope();
                    for st in body {
                        self.emit_stmt(st)?;
                    }
                    self.pop_scope();
                    self.indent -= 1;
                    let c = self.emit_expr(cond)?;
                    self.line(&format!("}} while ({c});"));
                } else {
                    let c = self.emit_expr(cond)?;
                    self.line(&format!("while ({c}) {{"));
                    self.indent += 1;
                    self.push_scope();
                    for st in body {
                        self.emit_stmt(st)?;
                    }
                    self.pop_scope();
                    self.indent -= 1;
                    self.line("}");
                }
            }
            Stmt::CFor {
                init,
                cond,
                step,
                body,
                ..
            } => {
                // A real PHP `for (init; cond; step)` — NOT lowered to a `while`, because PHP's
                // `continue` inside a `for` runs the `step` (a `while` would skip it).
                self.push_scope(); // init's binding
                let init_s = match init {
                    Some(s) => self.emit_for_clause(s)?,
                    None => String::new(),
                };
                let cond_s = match cond {
                    Some(c) => self.emit_expr(c)?,
                    None => String::new(),
                };
                let step_s = match step {
                    Some(s) => self.emit_for_clause(s)?,
                    None => String::new(),
                };
                self.line(&format!("for ({init_s}; {cond_s}; {step_s}) {{"));
                self.indent += 1;
                self.push_scope();
                for st in body {
                    self.emit_stmt(st)?;
                }
                self.pop_scope();
                self.indent -= 1;
                self.line("}");
                self.pop_scope();
            }
            Stmt::Break(_) => self.line("break;"),
            Stmt::Continue(_) => self.line("continue;"),
            Stmt::Block(stmts, _) => {
                self.line("{");
                self.indent += 1;
                self.push_scope();
                for st in stmts {
                    self.emit_stmt(st)?;
                }
                self.pop_scope();
                self.indent -= 1;
                self.line("}");
            }
            Stmt::Expr(e, _) => {
                let s = self.emit_expr(e)?;
                self.line(&format!("{s};"));
            }
            // `throw e;` → PHP `throw $e;` (M-faults 2b). The thrown value is an `Error` subtype,
            // which transpiled to a `\Exception` subclass (Task 2b.2), so it is a valid PHP throwable.
            Stmt::Throw { value, .. } => {
                let e = self.emit_expr(value)?;
                self.line(&format!("throw {e};"));
            }
            // `try { } catch (T e) { } … [finally { }]` → the PHP construct 1:1 (M-faults 2b). Multiple
            // clauses map to multiple PHP `catch`es; a union `catch (A | B e)` → PHP 8's `catch (A | B
            // $e)`. The `throws` declaration is erased (no `@throws` docblock — keeps the output minimal
            // and byte-identical). `?`-throws was already erased to the bare call by the checker (2b.3).
            Stmt::Try {
                body,
                catches,
                finally_block,
                ..
            } => {
                self.line("try {");
                self.indent += 1;
                self.push_scope();
                for st in body {
                    self.emit_stmt(st)?;
                }
                self.pop_scope();
                self.indent -= 1;
                for clause in catches {
                    self.line(&format!(
                        "}} catch ({} ${}) {{",
                        php_catch_type(&clause.ty),
                        clause.name
                    ));
                    self.indent += 1;
                    self.push_scope();
                    self.declare(&clause.name);
                    // T6d: the caught value's type is its exception class — so `e.message` field
                    // reads in the handler resolve.
                    self.declare_kind(&clause.name, kind_of_type(&clause.ty));
                    for st in &clause.body {
                        self.emit_stmt(st)?;
                    }
                    self.pop_scope();
                    self.indent -= 1;
                }
                if let Some(fb) = finally_block {
                    self.line("} finally {");
                    self.indent += 1;
                    self.push_scope();
                    for st in fb {
                        self.emit_stmt(st)?;
                    }
                    self.pop_scope();
                    self.indent -= 1;
                }
                self.line("}");
            }
            // Let-destructuring (Phase 1 slice 5). Spill the init to a fresh `$__phorge_d{N}` temp, then:
            // a STRUCT pattern reads each public PHP property (`$d->field`) into its binder; a LIST
            // pattern emits the (diverging) `else` guarded by a `count(...)` mismatch, then PHP's native
            // list assignment `[$a, $b] = $d`. The temp avoids re-evaluating a side-effecting init.
            Stmt::Destructure {
                pat,
                init,
                else_block,
                ..
            } => {
                use crate::ast::DestructurePat;
                let e = self.emit_expr(init)?;
                let tmp = format!("__phorge_d{}", self.tmp);
                self.tmp += 1;
                self.line(&format!("${tmp} = {e};"));
                match pat {
                    DestructurePat::Struct { fields, .. } => {
                        for f in fields {
                            self.declare(&f.binding);
                            self.line(&format!("${} = ${tmp}->{};", f.binding, f.field));
                        }
                    }
                    DestructurePat::List { binders, .. } => {
                        if let Some(eb) = else_block {
                            self.line(&format!("if (count(${tmp}) !== {}) {{", binders.len()));
                            self.indent += 1;
                            self.push_scope();
                            for st in eb {
                                self.emit_stmt(st)?;
                            }
                            self.pop_scope();
                            self.indent -= 1;
                            self.line("}");
                        }
                        let targets: Vec<String> = binders
                            .iter()
                            .map(|(name, _)| {
                                self.declare(name);
                                format!("${name}")
                            })
                            .collect();
                        self.line(&format!("[{}] = ${tmp};", targets.join(", ")));
                    }
                }
            }
        }
        Ok(())
    }

    /// Render a C-`for` init/step clause inline (no trailing `;`, no newline) for the PHP for-header
    /// (M-mut.3). The clause is a `VarDecl` (`$i = …`), an `Assign`/compound-assign desugar (`$i =
    /// …`), or a bare expression.
    pub(super) fn emit_for_clause(&mut self, s: &Stmt) -> Result<String, String> {
        Ok(match s {
            Stmt::VarDecl { name, init, .. } => {
                let e = self.emit_expr(init)?;
                self.declare(name);
                format!("${name} = {e}")
            }
            Stmt::Assign { target, value, .. } => {
                let lhs = match target {
                    Expr::Ident(n, _) => format!("${n}"),
                    Expr::Index { .. } | Expr::Member { .. } => self.emit_expr(target)?,
                    _ => unreachable!("checker rejects other assignment targets"),
                };
                let e = self.emit_expr(value)?;
                format!("{lhs} = {e}")
            }
            Stmt::Expr(e, _) => self.emit_expr(e)?,
            _ => unreachable!("c-for init/step is a decl, assignment, or expression"),
        })
    }
}
