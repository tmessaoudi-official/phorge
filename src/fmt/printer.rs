//! `phg fmt` — a comment-preserving, **full-surface** Phorge AST → `.phg` source printer. Unlike the
//! Tier-1-subset lift printer (`src/lift/printer.rs`), this one covers the *entire* language so it can
//! format any parseable program. Its matches are exhaustive — the Rust compiler proves completeness,
//! so it can never silently mis-handle a node — and the only `Err` arms are for AST shapes that a
//! *parsed* program can never contain (e.g. `Type::Erased`, which is produced only by a post-check
//! pass `phg fmt` never runs).
//!
//! Correctness discipline (the formatter's one hard rule — meaning preservation): strings are escaped
//! (incl. `{`/`}` → `\{`/`\}`, since a bare `{` opens an interpolation); binary/unary expressions are
//! parenthesized **only where precedence/associativity requires it** mirroring the parser's
//! binding-power table; and every meaning-carrying field (class generics / `use` traits / resolution
//! clauses, function `throws`, …) is printed. The invariants `parse(fmt(x)) ≡ parse(x)` and
//! `fmt(fmt(x)) == fmt(x)` are asserted by the round-trip tests.
//!
//! Comments (which the token stream discards) are carried in via the lexer's `lex_with_comments`
//! side-channel (F1) and interleaved by source span (F2b).

use crate::ast::{
    BinaryOp, CatchClause, ClassDecl, ClassMember, CtorParam, DestructureField, DestructurePat,
    EnumDecl, Expr, FieldPat, FunctionDecl, Item, LambdaBody, Modifier, Param, Pattern, Program,
    Resolution, Stmt, StrPart, Type, UnaryOp,
};
use crate::token::Comment;

/// Format a whole program (already parsed) to canonical `.phg` source, interleaving `comments`
/// (from [`crate::lexer::lex_with_comments`]) by source position. `Err` only for an AST a parsed
/// program cannot contain (see the module docs).
pub fn format_program(p: &Program, comments: &[Comment]) -> Result<String, String> {
    let mut pr = Printer {
        out: String::new(),
        indent: 0,
        comments,
        next_comment: 0,
    };
    pr.program(p)?;
    pr.flush_remaining_comments();
    Ok(pr.out)
}

struct Printer<'a> {
    out: String,
    indent: usize,
    /// Captured comments in source order (F1 side-channel); `next_comment` is the cursor.
    comments: &'a [Comment],
    next_comment: usize,
}

impl Printer<'_> {
    fn line(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    /// F2b: flush every own-line comment whose source position precedes byte offset `before`, each on
    /// its own indented line, ahead of the node about to be printed. (A trailing comment on the same
    /// line as preceding code is handled separately.) Comment text is emitted verbatim (no reflow).
    fn flush_comments_before(&mut self, before: usize) {
        while self.next_comment < self.comments.len()
            && self.comments[self.next_comment].span.start < before
        {
            let c = self.comments[self.next_comment].clone();
            self.next_comment += 1;
            for cl in c.text.lines() {
                self.line(cl);
            }
        }
    }

    /// Emit any comments that appear after the last printed node (trailing block/footer comments).
    fn flush_remaining_comments(&mut self) {
        let n = self.comments.len();
        while self.next_comment < n {
            let c = self.comments[self.next_comment].clone();
            self.next_comment += 1;
            for cl in c.text.lines() {
                self.line(cl);
            }
        }
    }

    fn program(&mut self, p: &Program) -> Result<(), String> {
        let pkg = if p.package.is_empty() {
            "Main".to_string()
        } else {
            p.package.join(".")
        };
        // A comment above the `package` line (a file header) is emitted first, before the package.
        let pkg_start = p.span.start;
        self.flush_comments_before(pkg_start);
        self.line(&format!("package {pkg};"));
        for item in &p.items {
            self.out.push('\n');
            // Own-line comments that precede this item (after the blank separator).
            self.flush_comments_before(item_start(item));
            self.item(item)?;
        }
        Ok(())
    }

    fn item(&mut self, item: &Item) -> Result<(), String> {
        match item {
            Item::Import {
                path,
                alias,
                type_only,
                ..
            } => {
                let kw = if *type_only { "import type" } else { "import" };
                let path = path.join(".");
                match alias {
                    Some(a) => self.line(&format!("{kw} {path} as {a};")),
                    None => self.line(&format!("{kw} {path};")),
                }
                Ok(())
            }
            Item::Function(f) => self.function(f),
            Item::Class(c) => self.class(c),
            Item::Enum(e) => self.enum_decl(e),
            Item::Interface(i) => self.interface(i),
            Item::Trait(t) => self.trait_decl(t),
            Item::TypeAlias { name, ty: t, .. } => {
                self.line(&format!("type {name} = {};", ty(t)?));
                Ok(())
            }
            Item::Test { name, body, .. } => {
                self.block_stmt(&format!("test {}", str_quote(name)), body)
            }
        }
    }

    fn interface(&mut self, i: &crate::ast::InterfaceDecl) -> Result<(), String> {
        let mut header = format!("interface {}", i.name);
        if !i.extends.is_empty() {
            header.push_str(&format!(" extends {}", i.extends.join(", ")));
        }
        self.line(&format!("{header} {{"));
        self.indent += 1;
        for m in &i.methods {
            // An interface method is a bodyless signature terminated by `;`.
            self.line(&format!("{};", self.fn_signature(m)?));
        }
        self.indent -= 1;
        self.line("}");
        Ok(())
    }

    fn trait_decl(&mut self, t: &crate::ast::TraitDecl) -> Result<(), String> {
        self.line(&format!("trait {} {{", t.name));
        self.indent += 1;
        for m in &t.members {
            self.member(m)?;
        }
        self.indent -= 1;
        self.line("}");
        Ok(())
    }

    // ── declarations ──

    /// The signature text of a function/method up to (not including) the body or `;`:
    /// `[mods]function name[<T>](params)[: Ret][ throws E]`. Shared by free functions, methods,
    /// abstract signatures, and interface method signatures — so every one prints `throws`.
    fn fn_signature(&self, f: &FunctionDecl) -> Result<String, String> {
        let mods = modifiers_str(&f.modifiers);
        let generics = if f.type_params.is_empty() {
            String::new()
        } else {
            format!("<{}>", f.type_params.join(", "))
        };
        let params = self.params(&f.params)?;
        let ret = match &f.ret {
            Some(t) => format!(": {}", ty(t)?),
            None => String::new(),
        };
        let throws = if f.throws.is_empty() {
            String::new()
        } else {
            let ts: Result<Vec<_>, _> = f.throws.iter().map(ty).collect();
            format!(" throws {}", ts?.join(" | "))
        };
        Ok(format!(
            "{mods}function {}{generics}({params}){ret}{throws}",
            f.name
        ))
    }

    fn function(&mut self, f: &FunctionDecl) -> Result<(), String> {
        let sig = self.fn_signature(f)?;
        if f.modifiers.contains(&Modifier::Abstract) {
            // A bodyless abstract method signature.
            self.line(&format!("{sig};"));
            return Ok(());
        }
        self.line(&format!("{sig} {{"));
        self.indent += 1;
        for s in &f.body {
            self.stmt(s)?;
        }
        self.indent -= 1;
        self.line("}");
        Ok(())
    }

    fn class(&mut self, c: &ClassDecl) -> Result<(), String> {
        // `abstract` implies `open`, so emit only the stronger keyword.
        let prefix = if c.is_abstract {
            "abstract "
        } else if c.open {
            "open "
        } else {
            ""
        };
        let generics = if c.type_params.is_empty() {
            String::new()
        } else {
            format!("<{}>", c.type_params.join(", "))
        };
        let mut header = format!("{prefix}class {}{generics}", c.name);
        if !c.extends.is_empty() {
            header.push_str(&format!(" extends {}", c.extends.join(", ")));
        }
        if !c.implements.is_empty() {
            header.push_str(&format!(" implements {}", c.implements.join(", ")));
        }
        header.push_str(" {");
        self.line(&header);
        self.indent += 1;
        // Trait composition (`use T;`) and multi-inheritance resolution clauses precede the members.
        for u in &c.uses {
            self.line(&format!("use {};", u.name));
        }
        for r in &c.resolutions {
            self.line(&resolution_str(r));
        }
        for m in &c.members {
            self.member(m)?;
        }
        self.indent -= 1;
        self.line("}");
        Ok(())
    }

    fn member(&mut self, m: &ClassMember) -> Result<(), String> {
        match m {
            ClassMember::Field {
                modifiers,
                ty: t,
                name,
                init,
                ..
            } => {
                let mods = modifiers_str(modifiers);
                match init {
                    Some(e) => self.line(&format!("{mods}{} {name} = {};", ty(t)?, self.expr(e)?)),
                    None => self.line(&format!("{mods}{} {name};", ty(t)?)),
                }
                Ok(())
            }
            ClassMember::Constructor { params, body, .. } => {
                let ps = self.ctor_params(params)?;
                if body.is_empty() {
                    self.line(&format!("constructor({ps}) {{}}"));
                } else {
                    self.line(&format!("constructor({ps}) {{"));
                    self.indent += 1;
                    for s in body {
                        self.stmt(s)?;
                    }
                    self.indent -= 1;
                    self.line("}");
                }
                Ok(())
            }
            ClassMember::Method(f) => self.function(f),
            ClassMember::Hook {
                ty: t,
                name,
                get,
                set,
                ..
            } => {
                self.line(&format!("{} {name} {{", ty(t)?));
                self.indent += 1;
                if let Some(g) = get {
                    self.line(&format!("get => {};", self.expr(g)?));
                }
                if let Some((param, body)) = set {
                    self.line(&format!("set({} {}) {{", ty(&param.ty)?, param.name));
                    self.indent += 1;
                    for s in body {
                        self.stmt(s)?;
                    }
                    self.indent -= 1;
                    self.line("}");
                }
                self.indent -= 1;
                self.line("}");
                Ok(())
            }
        }
    }

    fn enum_decl(&mut self, e: &EnumDecl) -> Result<(), String> {
        let generics = if e.type_params.is_empty() {
            String::new()
        } else {
            format!("<{}>", e.type_params.join(", "))
        };
        let mut variants = Vec::new();
        for v in &e.variants {
            if v.fields.is_empty() {
                variants.push(v.name.clone());
            } else {
                variants.push(format!("{}({})", v.name, self.params(&v.fields)?));
            }
        }
        self.line(&format!(
            "enum {}{generics} {{ {} }}",
            e.name,
            variants.join(", ")
        ));
        Ok(())
    }

    fn params(&self, params: &[Param]) -> Result<String, String> {
        let mut out = Vec::new();
        for p in params {
            // A default parameter (M4) prints its `= <expr>` so a format round-trip preserves it.
            let default = match &p.default {
                Some(e) => format!(" = {}", self.expr(e)?),
                None => String::new(),
            };
            out.push(format!("{} {}{default}", ty(&p.ty)?, p.name));
        }
        Ok(out.join(", "))
    }

    fn ctor_params(&self, params: &[CtorParam]) -> Result<String, String> {
        let mut out = Vec::new();
        for p in params {
            let mods = modifiers_str(&p.modifiers);
            out.push(format!("{mods}{} {}", ty(&p.ty)?, p.name));
        }
        Ok(out.join(", "))
    }

    // ── statements ──

    fn stmt(&mut self, s: &Stmt) -> Result<(), String> {
        // Flush any comments that appear before this statement in source (own-line placement). v1
        // reattaches to the nearest statement boundary — a trailing same-line comment becomes a
        // leading comment of the next node (documented limitation; comments are never lost).
        self.flush_comments_before(stmt_start(s));
        match s {
            Stmt::VarDecl {
                ty: t,
                name,
                init,
                mutable,
                ..
            } => {
                let m = if *mutable { "mutable " } else { "" };
                self.line(&format!("{m}{} {name} = {};", ty(t)?, self.expr(init)?));
                Ok(())
            }
            Stmt::Assign { target, value, .. } => {
                self.line(&format!("{} = {};", self.expr(target)?, self.expr(value)?));
                Ok(())
            }
            Stmt::Return { value, .. } => {
                match value {
                    Some(e) => self.line(&format!("return {};", self.expr(e)?)),
                    None => self.line("return;"),
                }
                Ok(())
            }
            Stmt::If {
                cond,
                bind,
                then_block,
                else_block,
                ..
            } => self.if_stmt(cond, bind.as_deref(), then_block, else_block.as_deref()),
            Stmt::While {
                cond,
                body,
                post_cond,
                ..
            } => {
                if *post_cond {
                    self.line("do {");
                    self.indent += 1;
                    for st in body {
                        self.stmt(st)?;
                    }
                    self.indent -= 1;
                    self.line(&format!("}} while ({});", self.expr(cond)?));
                } else {
                    self.block_stmt(&format!("while ({})", self.expr(cond)?), body)?;
                }
                Ok(())
            }
            Stmt::CFor {
                init,
                cond,
                step,
                body,
                ..
            } => {
                let i = match init {
                    Some(s) => self.stmt_inline(s)?,
                    None => String::new(),
                };
                let c = match cond {
                    Some(e) => self.expr(e)?,
                    None => String::new(),
                };
                let s = match step {
                    Some(s) => self.stmt_inline(s)?,
                    None => String::new(),
                };
                self.block_stmt(&format!("for ({i}; {c}; {s})"), body)
            }
            Stmt::For {
                ty: t,
                name,
                iter,
                body,
                ..
            } => {
                // An inferred-element for-in prints as the idiomatic `foreach (iter as name)`
                // (A-6); an explicit element type keeps the typed `for (T name in iter)` form.
                let head = if matches!(t, Type::Infer(_)) {
                    format!("foreach ({} as {name})", self.expr(iter)?)
                } else {
                    format!("for ({} {name} in {})", ty(t)?, self.expr(iter)?)
                };
                self.block_stmt(&head, body)
            }
            Stmt::Break(_) => {
                self.line("break;");
                Ok(())
            }
            Stmt::Continue(_) => {
                self.line("continue;");
                Ok(())
            }
            Stmt::Block(stmts, _) => self.block_stmt("", stmts),
            Stmt::Expr(e, _) => {
                self.line(&format!("{};", self.expr(e)?));
                Ok(())
            }
            Stmt::Throw { value, .. } => {
                self.line(&format!("throw {};", self.expr(value)?));
                Ok(())
            }
            Stmt::Try {
                body,
                catches,
                finally_block,
                ..
            } => self.try_stmt(body, catches, finally_block.as_deref()),
            Stmt::Destructure {
                pat,
                init,
                else_block,
                ..
            } => {
                let head = format!("var {} = {}", self.destructure_pat(pat)?, self.expr(init)?);
                match else_block {
                    None => {
                        self.line(&format!("{head};"));
                        Ok(())
                    }
                    Some(eb) => {
                        self.line(&format!("{head} else {{"));
                        self.indent += 1;
                        for s in eb {
                            self.stmt(s)?;
                        }
                        self.indent -= 1;
                        self.line("}");
                        Ok(())
                    }
                }
            }
        }
    }

    fn try_stmt(
        &mut self,
        body: &[Stmt],
        catches: &[CatchClause],
        finally_block: Option<&[Stmt]>,
    ) -> Result<(), String> {
        self.line("try {");
        self.indent += 1;
        for s in body {
            self.stmt(s)?;
        }
        self.indent -= 1;
        for c in catches {
            self.line(&format!("}} catch ({} {}) {{", ty(&c.ty)?, c.name));
            self.indent += 1;
            for s in &c.body {
                self.stmt(s)?;
            }
            self.indent -= 1;
        }
        match finally_block {
            Some(fb) => {
                self.line("} finally {");
                self.indent += 1;
                for s in fb {
                    self.stmt(s)?;
                }
                self.indent -= 1;
                self.line("}");
            }
            None => self.line("}"),
        }
        Ok(())
    }

    fn destructure_pat(&self, p: &DestructurePat) -> Result<String, String> {
        match p {
            DestructurePat::Struct {
                type_name, fields, ..
            } => {
                let fs: Vec<String> = fields
                    .iter()
                    .map(|f: &DestructureField| {
                        if f.field == f.binding {
                            f.field.clone()
                        } else {
                            format!("{}: {}", f.field, f.binding)
                        }
                    })
                    .collect();
                Ok(format!("{type_name} {{ {} }}", fs.join(", ")))
            }
            DestructurePat::List { binders, .. } => {
                let bs: Vec<String> = binders.iter().map(|(n, _)| n.clone()).collect();
                Ok(format!("[{}]", bs.join(", ")))
            }
        }
    }

    /// `<head> { <body> }` — a header plus an indented statement block.
    fn block_stmt(&mut self, head: &str, body: &[Stmt]) -> Result<(), String> {
        if head.is_empty() {
            self.line("{");
        } else {
            self.line(&format!("{head} {{"));
        }
        self.indent += 1;
        for s in body {
            self.stmt(s)?;
        }
        self.indent -= 1;
        self.line("}");
        Ok(())
    }

    fn if_stmt(
        &mut self,
        cond: &Expr,
        bind: Option<&str>,
        then_block: &[Stmt],
        else_block: Option<&[Stmt]>,
    ) -> Result<(), String> {
        let cond_s = match bind {
            Some(name) => format!("var {name} = {}", self.expr(cond)?),
            None => self.expr(cond)?,
        };
        self.line(&format!("if ({cond_s}) {{"));
        self.indent += 1;
        for s in then_block {
            self.stmt(s)?;
        }
        self.indent -= 1;
        match else_block {
            None => self.line("}"),
            // `else if` chain: an else-block holding exactly one `If` renders as `} else if (...) {`.
            Some(
                [Stmt::If {
                    cond,
                    bind,
                    then_block,
                    else_block,
                    ..
                }],
            ) => {
                let cond_s = match bind {
                    Some(name) => format!("var {name} = {}", self.expr(cond)?),
                    None => self.expr(cond)?,
                };
                self.line(&format!("}} else if ({cond_s}) {{"));
                self.indent += 1;
                for s in then_block {
                    self.stmt(s)?;
                }
                self.indent -= 1;
                // Recurse for any further chained else.
                return self.close_else(else_block.as_deref());
            }
            Some(body) => {
                self.line("} else {");
                self.indent += 1;
                for s in body {
                    self.stmt(s)?;
                }
                self.indent -= 1;
                self.line("}");
            }
        }
        Ok(())
    }

    /// Close out an `else`/`else if` tail (used by the `else if` chain in [`Self::if_stmt`]).
    fn close_else(&mut self, else_block: Option<&[Stmt]>) -> Result<(), String> {
        match else_block {
            None => self.line("}"),
            Some(
                [Stmt::If {
                    cond,
                    bind,
                    then_block,
                    else_block,
                    ..
                }],
            ) => {
                let cond_s = match bind {
                    Some(name) => format!("var {name} = {}", self.expr(cond)?),
                    None => self.expr(cond)?,
                };
                self.line(&format!("}} else if ({cond_s}) {{"));
                self.indent += 1;
                for s in then_block {
                    self.stmt(s)?;
                }
                self.indent -= 1;
                return self.close_else(else_block.as_deref());
            }
            Some(body) => {
                self.line("} else {");
                self.indent += 1;
                for s in body {
                    self.stmt(s)?;
                }
                self.indent -= 1;
                self.line("}");
            }
        }
        Ok(())
    }

    /// A statement rendered inline with no indent or trailing `;` — for the clauses of a C-style `for`.
    fn stmt_inline(&self, s: &Stmt) -> Result<String, String> {
        match s {
            Stmt::VarDecl {
                ty: t,
                name,
                init,
                mutable,
                ..
            } => {
                let m = if *mutable { "mutable " } else { "" };
                Ok(format!("{m}{} {name} = {}", ty(t)?, self.expr(init)?))
            }
            Stmt::Assign { target, value, .. } => {
                Ok(format!("{} = {}", self.expr(target)?, self.expr(value)?))
            }
            Stmt::Expr(e, _) => self.expr(e),
            _ => Err("printer: only var-decl/assign/expr are valid in a for-clause".into()),
        }
    }

    // ── expressions ──

    fn expr(&self, e: &Expr) -> Result<String, String> {
        match e {
            Expr::Int(n, _) => Ok(n.to_string()),
            Expr::Float(f, _) => Ok(format!("{f:?}")),
            // A `decimal` literal prints back as its rendered value + the `d` suffix (M-NUM S1) — the
            // round-trip-faithful surface form (`19.99d`).
            Expr::Decimal {
                unscaled, scale, ..
            } => Ok(format!("{}d", crate::value::fmt_decimal(*unscaled, *scale))),
            Expr::Bool(b, _) => Ok(b.to_string()),
            Expr::Null(_) => Ok("null".to_string()),
            Expr::Str(parts, _) => self.str_lit(parts),
            Expr::Ident(name, _) => Ok(name.clone()),
            Expr::This(_) => Ok("this".to_string()),
            Expr::List(items, _) => {
                let xs: Result<Vec<_>, _> = items.iter().map(|x| self.expr(x)).collect();
                Ok(format!("[{}]", xs?.join(", ")))
            }
            Expr::Map(pairs, _) => {
                let mut xs = Vec::new();
                for (k, v) in pairs {
                    xs.push(format!("{} => {}", self.expr(k)?, self.expr(v)?));
                }
                Ok(format!("[{}]", xs.join(", ")))
            }
            Expr::Unary { op, expr, .. } => {
                // Unary binds tighter than every binary op; a binary/range operand needs parens, and
                // so does a nested unary (to avoid `--`/`~~` re-lexing as a multi-char token).
                let needs = prec_of(expr) < PREC_UNARY || matches!(**expr, Expr::Unary { .. });
                let inner = self.expr(expr)?;
                let inner = if needs { format!("({inner})") } else { inner };
                Ok(format!("{}{inner}", unary_op(*op)))
            }
            Expr::Binary { op, lhs, rhs, .. } => {
                let p = bin_prec(*op);
                let right_assoc = matches!(op, BinaryOp::Pow);
                let l = self.operand(lhs, p, false, right_assoc)?;
                let r = self.operand(rhs, p, true, right_assoc)?;
                Ok(format!("{l} {} {r}", binary_op(*op)))
            }
            Expr::InstanceOf {
                value, type_name, ..
            } => {
                // `instanceof` is a left-precedence-8 test; its value operand needs parens below 8.
                let v = self.operand(value, 8, false, false)?;
                Ok(format!("{v} instanceof {type_name}"))
            }
            Expr::Cast {
                value, type_name, ..
            } => {
                // `as` is a left-precedence-8 cast (same level as `instanceof`); its value operand
                // needs parens below 8.
                let v = self.operand(value, 8, false, false)?;
                Ok(format!("{v} as {type_name}"))
            }
            Expr::Call { callee, args, .. } => {
                let a: Result<Vec<_>, _> = args.iter().map(|x| self.expr(x)).collect();
                Ok(format!(
                    "{}({})",
                    self.postfix_operand(callee)?,
                    a?.join(", ")
                ))
            }
            Expr::Member {
                object, name, safe, ..
            } => {
                let dot = if *safe { "?." } else { "." };
                Ok(format!("{}{dot}{name}", self.postfix_operand(object)?))
            }
            Expr::Index { object, index, .. } => Ok(format!(
                "{}[{}]",
                self.postfix_operand(object)?,
                self.expr(index)?
            )),
            Expr::Force { inner, .. } => Ok(format!("{}!", self.postfix_operand(inner)?)),
            Expr::Propagate { inner, .. } => Ok(format!("{}?", self.postfix_operand(inner)?)),
            Expr::Range {
                start,
                end,
                inclusive,
                ..
            } => {
                // Range is the loosest expression (operands are full binaries); only a nested range
                // operand needs parens.
                let dots = if *inclusive { "..=" } else { ".." };
                let wrap = |pr: &Self, e: &Expr| -> Result<String, String> {
                    let s = pr.expr(e)?;
                    // Only a nested range (the single loosest form) needs parens here.
                    Ok(if prec_of(e) == PREC_RANGE {
                        format!("({s})")
                    } else {
                        s
                    })
                };
                Ok(format!("{}{dots}{}", wrap(self, start)?, wrap(self, end)?))
            }
            Expr::If {
                cond,
                then_expr,
                else_expr,
                ..
            } => Ok(format!(
                "if ({}) {{ {} }} else {{ {} }}",
                self.expr(cond)?,
                self.expr(then_expr)?,
                self.expr(else_expr)?
            )),
            Expr::New(inner, _) => Ok(format!("new {}", self.expr(inner)?)),
            Expr::Match {
                scrutinee, arms, ..
            } => {
                let mut out = Vec::new();
                for arm in arms {
                    let guard = match &arm.guard {
                        Some(g) => format!(" when {}", self.expr(g)?),
                        None => String::new(),
                    };
                    out.push(format!(
                        "{}{guard} => {}",
                        self.pattern(&arm.pattern)?,
                        self.expr(&arm.body)?
                    ));
                }
                Ok(format!(
                    "match ({}) {{ {} }}",
                    self.expr(scrutinee)?,
                    out.join(", ")
                ))
            }
            Expr::Bytes(bytes, _) => Ok(format!("b\"{}\"", escape_bytes(bytes))),
            Expr::Lambda {
                params, ret, body, ..
            } => {
                let ps = self.params(params)?;
                match body {
                    LambdaBody::Expr(e) => {
                        // Expression body: `fn(params)[: Ret] => expr` (the `: Ret` annotation is
                        // optional on an expression lambda; print it when present).
                        let r = match ret {
                            Some(t) => format!(": {}", ty(t)?),
                            None => String::new(),
                        };
                        Ok(format!("fn({ps}){r} => {}", self.expr(e)?))
                    }
                    LambdaBody::Block(stmts) => {
                        // Statement body: `fn(params) -> Ret { … }` (the return type is required).
                        let r = match ret {
                            Some(t) => format!(" -> {}", ty(t)?),
                            None => String::new(),
                        };
                        let mut out = format!("fn({ps}){r} {{ ");
                        let inner: Result<Vec<_>, _> =
                            stmts.iter().map(|s| self.stmt_inline_loose(s)).collect();
                        out.push_str(&inner?.join(" "));
                        out.push_str(" }");
                        Ok(out)
                    }
                }
            }
            Expr::CloneWith { object, fields, .. } => {
                let mut fs = Vec::new();
                for (name, e) in fields {
                    fs.push(format!("{name}: {}", self.expr(e)?));
                }
                Ok(format!(
                    "{} with {{ {} }}",
                    self.postfix_operand(object)?,
                    fs.join(", ")
                ))
            }
            // `html"…"` literal — same segment model as a string, different delimiter.
            Expr::Html(parts, _) => {
                let mut s = String::from("html\"");
                for part in parts {
                    match part {
                        StrPart::Literal(lit) => s.push_str(&escape_str(lit)),
                        StrPart::Expr(e) => s.push_str(&format!("{{{}}}", self.expr(e)?)),
                    }
                }
                s.push('"');
                Ok(s)
            }
        }
    }

    /// A statement printed inline (for a single-line lambda block body) with a trailing `;` but no
    /// indent/newline. Reuses [`Self::stmt_inline`] for the expression-ish forms; a `return` is the
    /// common lambda-block statement.
    fn stmt_inline_loose(&self, s: &Stmt) -> Result<String, String> {
        match s {
            Stmt::Return { value, .. } => match value {
                Some(e) => Ok(format!("return {};", self.expr(e)?)),
                None => Ok("return;".to_string()),
            },
            other => Ok(format!("{};", self.stmt_inline(other)?)),
        }
    }

    fn str_lit(&self, parts: &[StrPart]) -> Result<String, String> {
        let mut s = String::from("\"");
        for part in parts {
            match part {
                StrPart::Literal(lit) => s.push_str(&escape_str(lit)),
                StrPart::Expr(e) => s.push_str(&format!("{{{}}}", self.expr(e)?)),
            }
        }
        s.push('"');
        Ok(s)
    }

    /// Print a binary operand, parenthesizing only when precedence/associativity requires it.
    /// `parent` is the operator's binding power; `is_right`/`right_assoc` pick the associativity
    /// side. Left-assoc: the right operand needs parens at equal precedence; right-assoc (`**`):
    /// the left operand does.
    fn operand(
        &self,
        e: &Expr,
        parent: u8,
        is_right: bool,
        right_assoc: bool,
    ) -> Result<String, String> {
        let cp = prec_of(e);
        let needs = if is_right == right_assoc {
            cp < parent
        } else {
            cp <= parent
        };
        let s = self.expr(e)?;
        Ok(if needs { format!("({s})") } else { s })
    }

    /// Print the receiver of a postfix operator (`.`/`[]`/call/`!`/`?`), which binds tighter than
    /// every prefix/binary form — so a non-atomic receiver (a binary, unary, or range) needs parens.
    fn postfix_operand(&self, e: &Expr) -> Result<String, String> {
        let s = self.expr(e)?;
        Ok(if prec_of(e) < PREC_ATOM {
            format!("({s})")
        } else {
            s
        })
    }

    fn pattern(&self, p: &Pattern) -> Result<String, String> {
        match p {
            Pattern::Wildcard(_) => Ok("_".to_string()),
            Pattern::Binding { name, .. } => Ok(name.clone()),
            Pattern::Int(n, _) => Ok(n.to_string()),
            Pattern::Float(f, _) => Ok(format!("{f:?}")),
            Pattern::Decimal {
                unscaled, scale, ..
            } => Ok(format!("{}d", crate::value::fmt_decimal(*unscaled, *scale))),
            Pattern::Str(s, _) => Ok(format!("\"{}\"", escape_str(s))),
            Pattern::Bool(b, _) => Ok(b.to_string()),
            Pattern::Null(_) => Ok("null".to_string()),
            Pattern::Variant { name, fields, .. } => {
                let fs: Result<Vec<_>, _> = fields.iter().map(|f| self.pattern(f)).collect();
                Ok(format!("{name}({})", fs?.join(", ")))
            }
            Pattern::Type {
                type_name, binding, ..
            } => match binding {
                Some(b) => Ok(format!("{type_name} {b}")),
                None => Ok(format!("{type_name} _")),
            },
            Pattern::Struct {
                type_name, fields, ..
            } => {
                let fs: Result<Vec<_>, _> = fields
                    .iter()
                    .map(|f: &FieldPat| {
                        Ok::<_, String>(format!("{}: {}", f.field, self.pattern(&f.pat)?))
                    })
                    .collect();
                Ok(format!("{type_name} {{ {} }}", fs?.join(", ")))
            }
        }
    }
}

/// Render a `Type`. `Type::Infer` prints as `var` (the local-inference keyword). `Err` for nodes the
/// lift subset never produces (function types, fixed lists, unions/intersections, erased).
fn ty(t: &Type) -> Result<String, String> {
    match t {
        Type::Named { name, args, .. } => {
            if args.is_empty() {
                Ok(name.clone())
            } else {
                let a: Result<Vec<_>, _> = args.iter().map(ty).collect();
                Ok(format!("{name}<{}>", a?.join(", ")))
            }
        }
        Type::Optional { inner, .. } => Ok(format!("{}?", ty(inner)?)),
        Type::Infer(_) => Ok("var".to_string()),
        Type::Union(members, _) => {
            let m: Result<Vec<_>, _> = members.iter().map(ty).collect();
            Ok(m?.join(" | "))
        }
        Type::Intersection(members, _) => {
            let m: Result<Vec<_>, _> = members.iter().map(ty).collect();
            Ok(m?.join(" & "))
        }
        Type::Function { params, ret, .. } => {
            let ps: Result<Vec<_>, _> = params.iter().map(ty).collect();
            Ok(format!("({}) -> {}", ps?.join(", "), ty(ret)?))
        }
        Type::FixedList { elem, len, .. } => Ok(format!("[{}; {len}]", ty(elem)?)),
        // `Type::Erased` is produced only by the post-check `erase_generics` pass, which `phg fmt`
        // (parse → print, no checking) never runs — so a parsed program cannot contain it.
        Type::Erased(_) => Err("printer: Type::Erased cannot occur in a parsed program".into()),
    }
}

fn modifiers_str(mods: &[Modifier]) -> String {
    // A stable canonical order, each followed by a space.
    const ORDER: &[(Modifier, &str)] = &[
        (Modifier::Public, "public"),
        (Modifier::Private, "private"),
        (Modifier::Protected, "protected"),
        (Modifier::Open, "open"),
        (Modifier::Abstract, "abstract"),
        (Modifier::Mutable, "mutable"),
        (Modifier::Static, "static"),
        (Modifier::Const, "const"),
    ];
    let mut s = String::new();
    for (m, kw) in ORDER {
        if mods.contains(m) {
            s.push_str(kw);
            s.push(' ');
        }
    }
    s
}

/// Atoms and every postfix form (member/index/call/force/propagate) — and keyword-led primaries
/// (`if`/`match`/`new`) — never need parentheses as a child. Above any operator.
const PREC_ATOM: u8 = 100;
/// Prefix unary (`-`/`!`/`~`): tighter than every binary op, looser than postfix.
const PREC_UNARY: u8 = 80;
/// Ranges (`a..b`) bind looser than every binary operator (operands are full binaries).
const PREC_RANGE: u8 = 0;

/// Binding power of a binary operator — mirrors the Phorge parser's `infix_op` table exactly
/// (`src/parser/exprs.rs`); higher binds tighter. The shared source of truth for re-parse fidelity.
fn bin_prec(op: BinaryOp) -> u8 {
    match op {
        BinaryOp::Pipe => 1,
        BinaryOp::Coalesce => 2,
        BinaryOp::Or => 3,
        BinaryOp::And => 4,
        BinaryOp::BitOr => 5,
        BinaryOp::BitXor => 6,
        BinaryOp::BitAnd => 7,
        BinaryOp::Eq | BinaryOp::NotEq => 8,
        BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge => 9,
        BinaryOp::Shl | BinaryOp::Shr => 10,
        BinaryOp::Add | BinaryOp::Sub => 11,
        BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => 12,
        BinaryOp::Pow => 13,
    }
}

/// The precedence of an expression's top node, for deciding whether it needs parens as a child.
fn prec_of(e: &Expr) -> u8 {
    match e {
        Expr::Binary { op, .. } => bin_prec(*op),
        Expr::InstanceOf { .. } | Expr::Cast { .. } => 8,
        Expr::Unary { .. } => PREC_UNARY,
        Expr::Range { .. } => PREC_RANGE,
        _ => PREC_ATOM,
    }
}

fn binary_op(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Pow => "**",
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
        BinaryOp::BitAnd => "&",
        BinaryOp::BitOr => "|",
        BinaryOp::BitXor => "^",
        BinaryOp::Shl => "<<",
        BinaryOp::Shr => ">>",
    }
}

fn unary_op(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Neg => "-",
        UnaryOp::Not => "!",
        UnaryOp::BitNot => "~",
    }
}

/// Byte offset where an item's source begins — for flushing own-line comments before it.
fn item_start(item: &Item) -> usize {
    match item {
        Item::Import { span, .. } | Item::TypeAlias { span, .. } | Item::Test { span, .. } => {
            span.start
        }
        Item::Function(f) => f.span.start,
        Item::Enum(e) => e.span.start,
        Item::Class(c) => c.span.start,
        Item::Interface(i) => i.span.start,
        Item::Trait(t) => t.span.start,
    }
}

/// Byte offset where a statement's source begins — for flushing own-line comments before it.
fn stmt_start(s: &Stmt) -> usize {
    match s {
        Stmt::VarDecl { span, .. }
        | Stmt::Assign { span, .. }
        | Stmt::Return { span, .. }
        | Stmt::If { span, .. }
        | Stmt::For { span, .. }
        | Stmt::While { span, .. }
        | Stmt::CFor { span, .. }
        | Stmt::Throw { span, .. }
        | Stmt::Try { span, .. }
        | Stmt::Destructure { span, .. } => span.start,
        Stmt::Break(sp) | Stmt::Continue(sp) | Stmt::Block(_, sp) | Stmt::Expr(_, sp) => sp.start,
    }
}

/// A plain double-quoted string literal (for a `test` name) — escaped, no interpolation holes.
fn str_quote(s: &str) -> String {
    format!("\"{}\"", escape_str(s))
}

/// Render a multi-inheritance resolution clause (M-RT S6b) inside a class body.
fn resolution_str(r: &Resolution) -> String {
    match r {
        Resolution::Use { parent, method, .. } => format!("use {parent}.{method};"),
        Resolution::Exclude { parent, method, .. } => format!("exclude {parent}.{method};"),
        Resolution::Rename {
            parent,
            method,
            as_name,
            ..
        } => format!("rename {parent}.{method} as {as_name};"),
    }
}

/// Escape raw bytes for a `b"…"` byte-string literal: printable ASCII verbatim (with `\`/`"`
/// escaped), everything else as `\xHH`.
fn escape_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        match b {
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            0x20..=0x7e => out.push(b as char),
            _ => out.push_str(&format!("\\x{b:02x}")),
        }
    }
    out
}

/// Escape a string literal's contents for a Phorge double-quoted string. `{`/`}` become `\{`/`\}`
/// because a bare `{` opens an interpolation.
fn escape_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            _ => out.push(c),
        }
    }
    out
}
