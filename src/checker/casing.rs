//! `impl Checker` — casing cluster (M-Decomp W2). See checker/mod.rs for the struct + entry points.

use super::*;

impl Checker {
    // ---- identifier casing (reshape slice 2a) ----
    /// Enforce the casing discipline as **hard** errors (front-end-only, so it cannot affect
    /// byte-identity — every backend sees the same AST, the rule just gates which programs reach
    /// them). Value identifiers (functions, methods, parameters, fields, `var` bindings, lambda
    /// parameters) must be camelCase (`E-NAME-CASE`); type identifiers (class, enum, enum variant,
    /// `type` alias names) must be PascalCase (`E-TYPE-CASE`). Package segments are NOT checked here
    /// — that is reshape slice 2b (`E-PKG-CASE`).
    pub(super) fn check_casing(&mut self, program: &Program) {
        use crate::ast::{ClassMember, Item};
        for item in &program.items {
            match item {
                Item::Function(f) => self.check_fn_casing(f),
                Item::Class(c) => {
                    self.want_type_case(&c.name, c.span);
                    // Generic class type parameters are type names — PascalCase (M-RT generics-all).
                    for tp in &c.type_params {
                        self.want_type_case(tp, c.span);
                    }
                    for m in &c.members {
                        match m {
                            ClassMember::Field {
                                name,
                                span,
                                modifiers,
                                ..
                            } => {
                                if modifiers.contains(&crate::ast::Modifier::Const) {
                                    self.want_const_case(name, *span);
                                } else {
                                    self.want_name_case(name, *span);
                                }
                            }
                            ClassMember::Constructor { params, .. } => {
                                for p in params {
                                    self.want_name_case(&p.name, p.span);
                                }
                            }
                            ClassMember::Method(f) => self.check_fn_casing(f),
                            // A hook name + its `set` parameter follow field/var casing (camelCase).
                            ClassMember::Hook {
                                name, set, span, ..
                            } => {
                                self.want_name_case(name, *span);
                                if let Some((p, _)) = set {
                                    self.want_name_case(&p.name, p.span);
                                }
                            }
                        }
                    }
                }
                Item::Enum(e) => {
                    self.want_type_case(&e.name, e.span);
                    for v in &e.variants {
                        self.want_type_case(&v.name, v.span);
                    }
                }
                Item::Interface(i) => {
                    self.want_type_case(&i.name, i.span);
                    for m in &i.methods {
                        self.check_fn_casing(m);
                    }
                }
                // M-RT S8: a trait name is a type identifier (PascalCase); its members follow the same
                // value-identifier casing as a class's.
                Item::Trait(t) => {
                    self.want_type_case(&t.name, t.span);
                    for m in &t.members {
                        match m {
                            ClassMember::Field {
                                name,
                                span,
                                modifiers,
                                ..
                            } => {
                                if modifiers.contains(&crate::ast::Modifier::Const) {
                                    self.want_const_case(name, *span);
                                } else {
                                    self.want_name_case(name, *span);
                                }
                            }
                            ClassMember::Constructor { params, .. } => {
                                for p in params {
                                    self.want_name_case(&p.name, p.span);
                                }
                            }
                            ClassMember::Method(f) => self.check_fn_casing(f),
                            ClassMember::Hook {
                                name, set, span, ..
                            } => {
                                self.want_name_case(name, *span);
                                if let Some((p, _)) = set {
                                    self.want_name_case(&p.name, p.span);
                                }
                            }
                        }
                    }
                }
                Item::TypeAlias { name, span, .. } => self.want_type_case(name, *span),
                Item::Import { .. } => {}
            }
        }
    }

    /// Casing for a function/method declaration: its name + parameters are camelCase, and its body
    /// is walked for `var` bindings and lambda parameters.
    pub(super) fn check_fn_casing(&mut self, f: &crate::ast::FunctionDecl) {
        self.want_name_case(&f.name, f.span);
        // Generic type parameters are type names — PascalCase, like classes/enums (M-RT S7).
        for tp in &f.type_params {
            self.want_type_case(tp, f.span);
        }
        for p in &f.params {
            self.want_name_case(&p.name, p.span);
        }
        for s in &f.body {
            self.check_stmt_casing(s);
        }
    }

    /// Walk a statement for value-binding casing (`var` declarations, `for`-loop variables,
    /// if-let bindings) and any nested lambda parameters.
    pub(super) fn check_stmt_casing(&mut self, s: &crate::ast::Stmt) {
        use crate::ast::Stmt;
        match s {
            Stmt::VarDecl {
                name, init, span, ..
            } => {
                self.want_name_case(name, *span);
                self.check_expr_casing(init);
            }
            Stmt::Return { value, .. } => {
                if let Some(e) = value {
                    self.check_expr_casing(e);
                }
            }
            Stmt::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                self.check_expr_casing(cond);
                for st in then_block {
                    self.check_stmt_casing(st);
                }
                if let Some(eb) = else_block {
                    for st in eb {
                        self.check_stmt_casing(st);
                    }
                }
            }
            Stmt::For { iter, body, .. } => {
                self.check_expr_casing(iter);
                for st in body {
                    self.check_stmt_casing(st);
                }
            }
            Stmt::While { cond, body, .. } => {
                self.check_expr_casing(cond);
                for st in body {
                    self.check_stmt_casing(st);
                }
            }
            Stmt::CFor {
                init,
                cond,
                step,
                body,
                ..
            } => {
                if let Some(s) = init {
                    self.check_stmt_casing(s);
                }
                if let Some(c) = cond {
                    self.check_expr_casing(c);
                }
                if let Some(s) = step {
                    self.check_stmt_casing(s);
                }
                for st in body {
                    self.check_stmt_casing(st);
                }
            }
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Block(stmts, _) => {
                for st in stmts {
                    self.check_stmt_casing(st);
                }
            }
            Stmt::Assign { target, value, .. } => {
                self.check_expr_casing(target);
                self.check_expr_casing(value);
            }
            Stmt::Expr(e, _) => self.check_expr_casing(e),
            Stmt::Throw { value, .. } => self.check_expr_casing(value),
            Stmt::Try {
                body,
                catches,
                finally_block,
                ..
            } => {
                for st in body {
                    self.check_stmt_casing(st);
                }
                for c in catches {
                    self.want_name_case(&c.name, c.span);
                    for st in &c.body {
                        self.check_stmt_casing(st);
                    }
                }
                if let Some(fb) = finally_block {
                    for st in fb {
                        self.check_stmt_casing(st);
                    }
                }
            }
        }
    }

    /// Walk an expression for lambda parameters (the only value bindings introduced inside an
    /// expression) and recurse through every sub-expression.
    pub(super) fn check_expr_casing(&mut self, e: &crate::ast::Expr) {
        use crate::ast::{Expr, LambdaBody, StrPart};
        match e {
            Expr::Int(..)
            | Expr::Float(..)
            | Expr::Bool(..)
            | Expr::Null(..)
            | Expr::Bytes(..)
            | Expr::Ident(..)
            | Expr::This(..) => {}
            Expr::Str(parts, _) | Expr::Html(parts, _) => {
                for p in parts {
                    if let StrPart::Expr(inner) = p {
                        self.check_expr_casing(inner);
                    }
                }
            }
            Expr::List(items, _) => {
                for it in items {
                    self.check_expr_casing(it);
                }
            }
            Expr::Map(pairs, _) => {
                for (k, v) in pairs {
                    self.check_expr_casing(k);
                    self.check_expr_casing(v);
                }
            }
            Expr::Unary { expr, .. } => self.check_expr_casing(expr),
            Expr::Binary { lhs, rhs, .. } => {
                self.check_expr_casing(lhs);
                self.check_expr_casing(rhs);
            }
            Expr::InstanceOf { value, .. } => self.check_expr_casing(value),
            Expr::Call { callee, args, .. } => {
                self.check_expr_casing(callee);
                for a in args {
                    self.check_expr_casing(a);
                }
            }
            Expr::Member { object, .. } => self.check_expr_casing(object),
            Expr::Index { object, index, .. } => {
                self.check_expr_casing(object);
                self.check_expr_casing(index);
            }
            Expr::Force { inner, .. } => self.check_expr_casing(inner),
            Expr::Propagate { inner, .. } => self.check_expr_casing(inner),
            Expr::CloneWith { object, fields, .. } => {
                self.check_expr_casing(object);
                for (_, e) in fields {
                    self.check_expr_casing(e);
                }
            }
            Expr::Match {
                scrutinee, arms, ..
            } => {
                self.check_expr_casing(scrutinee);
                for arm in arms {
                    if let Some(g) = &arm.guard {
                        self.check_expr_casing(g);
                    }
                    self.check_expr_casing(&arm.body);
                }
            }
            Expr::Range { start, end, .. } => {
                self.check_expr_casing(start);
                self.check_expr_casing(end);
            }
            Expr::If {
                cond,
                then_expr,
                else_expr,
                ..
            } => {
                self.check_expr_casing(cond);
                self.check_expr_casing(then_expr);
                self.check_expr_casing(else_expr);
            }
            Expr::Lambda { params, body, .. } => {
                for p in params {
                    self.want_name_case(&p.name, p.span);
                }
                match body {
                    LambdaBody::Expr(inner) => self.check_expr_casing(inner),
                    LambdaBody::Block(stmts) => {
                        for st in stmts {
                            self.check_stmt_casing(st);
                        }
                    }
                }
            }
        }
    }

    /// A value identifier must be camelCase; otherwise `E-NAME-CASE` with a converted-form hint.
    ///
    /// The loader's cross-package mangling (M5 S2c) rewrites a library def name to a PHP-FQN key
    /// (`acme.util` + `compute` ⇒ `Acme\Util\compute`) *before* the checker runs. Casing applies to
    /// the **original source identifier**, so validate only the last `\`-segment — the leaf — which
    /// is byte-for-byte the name the developer wrote.
    pub(super) fn want_name_case(&mut self, name: &str, span: Span) {
        let leaf = leaf_ident(name);
        if !is_camel(leaf) {
            self.err_coded(
                span,
                format!("`{leaf}` must be camelCase"),
                "E-NAME-CASE",
                Some(format!("did you mean `{}`?", to_camel(leaf))),
            );
        }
    }

    /// A `const` class-constant name must be SCREAMING_SNAKE_CASE (Feature A); otherwise
    /// `E-CONST-CASE` with a converted-form hint. The PHP/C/Java constant convention.
    pub(super) fn want_const_case(&mut self, name: &str, span: Span) {
        let leaf = leaf_ident(name);
        if !is_screaming_snake(leaf) {
            self.err_coded(
                span,
                format!("`{leaf}` must be SCREAMING_SNAKE_CASE (it is a `const`)"),
                "E-CONST-CASE",
                Some(format!("did you mean `{}`?", to_screaming_snake(leaf))),
            );
        }
    }

    /// A type identifier must be PascalCase; otherwise `E-TYPE-CASE` with a converted-form hint.
    /// Validates the leaf identifier (see [`Self::want_name_case`] for why the FQN prefix is
    /// stripped). Cross-package types do not exist yet (`E-PKG-TYPE`), so a type name is never
    /// mangled today — but the leaf-strip keeps this robust if that changes.
    pub(super) fn want_type_case(&mut self, name: &str, span: Span) {
        let leaf = leaf_ident(name);
        if !is_pascal(leaf) {
            self.err_coded(
                span,
                format!("`{leaf}` must be PascalCase"),
                "E-TYPE-CASE",
                Some(format!("did you mean `{}`?", to_pascal(leaf))),
            );
        }
    }
}
