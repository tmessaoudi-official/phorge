//! M-Lift L4 — the **lifter**: PHP AST ([`super::ast`]) → Phorge AST ([`crate::ast`]). The lossy
//! half of the bridge. PHP is the floor, not the ceiling: lifted Phorge is *idiomatic* (PHP `.`
//! concat → `+`, `===` → `==`, top-level code → a `main()`, PHP fields → `mutable`) and never mirrors
//! a wart. The contract is a **draft you verify**, so the output is annotated `// lifted (verify)` by
//! the CLI (L6); anything that has no faithful Phorge form is a **loud lift error**, never a guess.
//!
//! Tier-1 core: typed functions, classes (typed props, ctor promotion, methods), pure enums, and the
//! plain statement/expression set. The Tier-2 frontier (`array`→List/Map/Set inference, default
//! params, backed enums, key-foreach, …) errors clearly here and is built out in later L4 slices.

use super::ast as php;
use super::lexer::lex_php;
use super::parser::parse_php;
use super::printer::print_program;
use crate::ast::{
    BinaryOp, ClassDecl, ClassMember, CtorParam, EnumDecl, EnumVariant, Expr, FunctionDecl, Item,
    MatchArm, Modifier, Param, Pattern, Program, Stmt, StrPart, Type, UnaryOp,
};
use crate::token::Span;
use std::collections::HashSet;

/// A zero span for synthesized nodes. The lift output is re-parsed (which re-derives real spans), and
/// the printer ignores spans, so a dummy is sound here.
const SP: Span = Span {
    start: 0,
    len: 0,
    line: 0,
    col: 0,
};

/// End-to-end convenience: PHP source → Phorge `.phg` source. Lexes (L1), parses (L2), lifts (L4),
/// and prints (L3). Any stage's error propagates as a `lift …` / `printer: …` string.
pub fn lift_source(php_src: &str) -> Result<String, String> {
    let toks = lex_php(php_src)?;
    let prog = parse_php(toks)?;
    let phorge = lift(&prog)?;
    print_program(&phorge)
}

/// Lift a parsed PHP program into a Phorge program (`package Main;`).
pub fn lift(prog: &php::PhpProgram) -> Result<Program, String> {
    let mut l = Lifter {
        needs_console: false,
    };
    let mut items: Vec<Item> = Vec::new();
    let mut top_stmts: Vec<Stmt> = Vec::new();
    let mut has_main = false;

    for item in &prog.items {
        match item {
            php::PhpItem::Function(f) => {
                if f.name == "main" {
                    has_main = true;
                }
                items.push(Item::Function(l.lift_function(f)?));
            }
            php::PhpItem::Class(c) => items.push(Item::Class(l.lift_class(c)?)),
            php::PhpItem::Enum(e) => items.push(Item::Enum(lift_enum(e)?)),
            php::PhpItem::Stmt(s) => {
                let mut declared = HashSet::new();
                top_stmts.extend(l.lift_stmt(s, &mut declared)?);
            }
        }
    }

    // Top-level PHP code becomes the runnable entry `function main()` (M5 model).
    if !top_stmts.is_empty() {
        if has_main {
            return Err(
                "lift: file has both a main() function and top-level code (ambiguous entry)".into(),
            );
        }
        items.push(Item::Function(FunctionDecl {
            modifiers: Vec::new(),
            vis: crate::ast::Visibility::Public,
            name: "main".into(),
            type_params: Vec::new(),
            params: Vec::new(),
            ret: Some(named("void")),
            throws: Vec::new(),
            body: top_stmts,
            span: SP,
        }));
    }

    // Prepend `import Core.Console;` if any `echo` was lifted.
    let mut final_items = Vec::new();
    if l.needs_console {
        final_items.push(Item::Import {
            path: vec!["Core".into(), "Console".into()],
            alias: None,
            type_only: false,
            span: SP,
        });
    }
    final_items.extend(items);

    Ok(Program {
        package: vec!["Main".into()],
        items: final_items,
        span: SP,
    })
}

struct Lifter {
    /// Set when an `echo` is lifted to `Console.print`, so the import is prepended.
    needs_console: bool,
}

impl Lifter {
    // ── declarations ──

    fn lift_function(&mut self, f: &php::PhpFunction) -> Result<FunctionDecl, String> {
        let mut declared = HashSet::new();
        let params = lift_params(&f.params)?;
        for p in &params {
            declared.insert(p.name.clone());
        }
        Ok(FunctionDecl {
            modifiers: Vec::new(),
            vis: crate::ast::Visibility::Public,
            name: f.name.clone(),
            type_params: Vec::new(),
            params,
            ret: lift_ret(&f.ret, Some(&f.body))?,
            throws: Vec::new(),
            body: self.lift_block(&f.body, &mut declared)?,
            span: SP,
        })
    }

    fn lift_class(&mut self, c: &php::PhpClass) -> Result<ClassDecl, String> {
        let mut members = Vec::new();
        for m in &c.members {
            members.push(self.lift_member(m)?);
        }
        Ok(ClassDecl {
            vis: crate::ast::Visibility::Public,
            name: c.name.clone(),
            type_params: Vec::new(),
            extends: c.extends.clone().into_iter().collect(),
            implements: c.implements.clone(),
            // PHP is extensible-by-default (only `final` seals it); Phorge is final-by-default, so a
            // non-final PHP class lifts to `open` to preserve extensibility. `abstract` implies open.
            open: c.is_abstract || !c.is_final,
            is_abstract: c.is_abstract,
            resolutions: Vec::new(),
            uses: Vec::new(),
            members,
            span: SP,
        })
    }

    fn lift_member(&mut self, m: &php::PhpMember) -> Result<ClassMember, String> {
        match m {
            php::PhpMember::Prop {
                vis,
                is_static,
                is_readonly,
                ty,
                name,
                default,
            } => {
                if !is_static && default.is_some() {
                    return Err(format!(
                        "lift: instance field `{name}` has a default — needs constructor synthesis (Tier-2)"
                    ));
                }
                let ty = lift_type(
                    ty.as_ref()
                        .ok_or_else(|| format!("lift: field `{name}` has no type (Tier-2)"))?,
                )?;
                let mut modifiers = vec![vis_modifier(*vis)];
                if *is_static {
                    modifiers.push(Modifier::Static);
                }
                // PHP properties are mutable unless `readonly`; mirror that faithfully.
                if !is_readonly {
                    modifiers.push(Modifier::Mutable);
                }
                let init = if *is_static {
                    default.as_ref().map(lift_expr).transpose()?
                } else {
                    None
                };
                Ok(ClassMember::Field {
                    modifiers,
                    ty,
                    name: name.clone(),
                    init,
                    span: SP,
                })
            }
            php::PhpMember::Const { vis, name, value } => {
                let tyname = lit_type(value).ok_or_else(|| {
                    format!("lift: const `{name}` has a non-literal value (Tier-2)")
                })?;
                Ok(ClassMember::Field {
                    modifiers: vec![vis_modifier(*vis), Modifier::Const],
                    ty: named(tyname),
                    name: name.clone(),
                    init: Some(lift_expr(value)?),
                    span: SP,
                })
            }
            php::PhpMember::Method(method) => self.lift_method(method),
        }
    }

    fn lift_method(&mut self, m: &php::PhpMethod) -> Result<ClassMember, String> {
        let mut declared = HashSet::new();
        // `__construct` → a Phorge `constructor` (with promotion), not an ordinary method.
        if m.name == "__construct" {
            let params = lift_ctor_params(&m.params)?;
            for p in &params {
                declared.insert(p.name.clone());
            }
            let body = match &m.body {
                Some(b) => self.lift_block(b, &mut declared)?,
                None => Vec::new(),
            };
            return Ok(ClassMember::Constructor {
                params,
                body,
                span: SP,
            });
        }
        let params = lift_params(&m.params)?;
        for p in &params {
            declared.insert(p.name.clone());
        }
        let mut modifiers = vec![vis_modifier(m.vis)];
        if m.is_static {
            modifiers.push(Modifier::Static);
        }
        if m.is_abstract {
            modifiers.push(Modifier::Abstract);
        } else if !m.is_final && m.vis != php::PhpVisibility::Private {
            // PHP methods are overridable by default; Phorge is final-by-default, so mark `open` to
            // preserve overridability (abstract is implicitly open, so only the concrete case).
            modifiers.push(Modifier::Open);
        }
        let body = match &m.body {
            Some(b) => self.lift_block(b, &mut declared)?,
            None => Vec::new(),
        };
        Ok(ClassMember::Method(FunctionDecl {
            modifiers,
            vis: crate::ast::Visibility::Public,
            name: m.name.clone(),
            type_params: Vec::new(),
            params,
            ret: lift_ret(&m.ret, m.body.as_deref())?,
            throws: Vec::new(),
            body,
            span: SP,
        }))
    }

    // ── statements ──

    fn lift_block(
        &mut self,
        stmts: &[php::PhpStmt],
        declared: &mut HashSet<String>,
    ) -> Result<Vec<Stmt>, String> {
        let mut out = Vec::new();
        for s in stmts {
            out.extend(self.lift_stmt(s, declared)?);
        }
        Ok(out)
    }

    fn lift_stmt(
        &mut self,
        s: &php::PhpStmt,
        declared: &mut HashSet<String>,
    ) -> Result<Vec<Stmt>, String> {
        Ok(match s {
            php::PhpStmt::Return(e) => {
                vec![Stmt::Return {
                    value: e.as_ref().map(lift_expr).transpose()?,
                    span: SP,
                }]
            }
            php::PhpStmt::Expr(e) => self.lift_expr_stmt(e, declared)?,
            php::PhpStmt::Echo(args) => {
                self.needs_console = true;
                let mut out = Vec::new();
                for a in args {
                    out.push(Stmt::Expr(console_print(lift_expr(a)?), SP));
                }
                out
            }
            php::PhpStmt::If {
                cond,
                then,
                elifs,
                els,
            } => {
                let mut else_block = match els {
                    Some(b) => Some(self.lift_block(b, declared)?),
                    None => None,
                };
                for (c, body) in elifs.iter().rev() {
                    else_block = Some(vec![Stmt::If {
                        cond: lift_expr(c)?,
                        bind: None,
                        then_block: self.lift_block(body, declared)?,
                        else_block,
                        span: SP,
                    }]);
                }
                vec![Stmt::If {
                    cond: lift_expr(cond)?,
                    bind: None,
                    then_block: self.lift_block(then, declared)?,
                    else_block,
                    span: SP,
                }]
            }
            php::PhpStmt::While { cond, body } => vec![Stmt::While {
                cond: lift_expr(cond)?,
                body: self.lift_block(body, declared)?,
                post_cond: false,
                span: SP,
            }],
            php::PhpStmt::For {
                init,
                cond,
                step,
                body,
            } => {
                let init = match init {
                    Some(e) => Some(Box::new(self.lift_for_clause(e, declared)?)),
                    None => None,
                };
                let step = match step {
                    Some(e) => Some(Box::new(self.lift_for_clause(e, declared)?)),
                    None => None,
                };
                vec![Stmt::CFor {
                    init,
                    cond: cond.as_ref().map(lift_expr).transpose()?,
                    step,
                    body: self.lift_block(body, declared)?,
                    span: SP,
                }]
            }
            php::PhpStmt::Foreach { key, .. } => {
                // Phorge's `for (Type x in iter)` requires a concrete element type (`var`/`Infer` is
                // valid only in a `VarDecl`), and PHP arrays carry no element type — so a faithful
                // `foreach` lift needs element-type inference. That (and the `$k => $v` key form) is
                // the Tier-2 frontier; refuse loudly rather than emit invalid Phorge.
                if key.is_some() {
                    return Err("lift: foreach with a key (`$k => $v`) is Tier-2".into());
                }
                return Err("lift: foreach needs element-type inference (Tier-2)".into());
            }
            php::PhpStmt::Break => vec![Stmt::Break(SP)],
            php::PhpStmt::Continue => vec![Stmt::Continue(SP)],
            php::PhpStmt::Block(stmts) => {
                vec![Stmt::Block(self.lift_block(stmts, declared)?, SP)]
            }
        })
    }

    /// A PHP expression statement: an assignment becomes a Phorge `var`-decl (first time) or
    /// `Stmt::Assign` (thereafter); `$i++`/`$x += e` desugar; anything else is an `Expr` statement.
    fn lift_expr_stmt(
        &mut self,
        e: &php::PhpExpr,
        declared: &mut HashSet<String>,
    ) -> Result<Vec<Stmt>, String> {
        Ok(vec![self.lift_assign_like(e, declared)?])
    }

    fn lift_assign_like(
        &mut self,
        e: &php::PhpExpr,
        declared: &mut HashSet<String>,
    ) -> Result<Stmt, String> {
        match e {
            php::PhpExpr::Assign { target, value } => {
                if let php::PhpExpr::Var(name) = target.as_ref() {
                    if !declared.contains(name) {
                        declared.insert(name.clone());
                        return Ok(Stmt::VarDecl {
                            ty: Type::Infer(SP),
                            name: name.clone(),
                            init: lift_expr(value)?,
                            mutable: true, // PHP locals are freely reassignable
                            span: SP,
                        });
                    }
                }
                Ok(Stmt::Assign {
                    target: lift_expr(target)?,
                    value: lift_expr(value)?,
                    span: SP,
                })
            }
            php::PhpExpr::CompoundAssign { target, op, value } => {
                // `x op= e` → `x = x op e`.
                let t = lift_expr(target)?;
                Ok(Stmt::Assign {
                    target: lift_expr(target)?,
                    value: Expr::Binary {
                        op: lift_binop(*op)?,
                        lhs: Box::new(t),
                        rhs: Box::new(lift_expr(value)?),
                        span: SP,
                    },
                    span: SP,
                })
            }
            php::PhpExpr::IncDec { target, inc, .. } => {
                // `x++`/`x--` → `x = x +/- 1`.
                let t = lift_expr(target)?;
                Ok(Stmt::Assign {
                    target: lift_expr(target)?,
                    value: Expr::Binary {
                        op: if *inc { BinaryOp::Add } else { BinaryOp::Sub },
                        lhs: Box::new(t),
                        rhs: Box::new(Expr::Int(1, SP)),
                        span: SP,
                    },
                    span: SP,
                })
            }
            other => Ok(Stmt::Expr(lift_expr(other)?, SP)),
        }
    }

    /// Lift a single PHP expression used as a C-`for` init/step clause into one Phorge statement.
    fn lift_for_clause(
        &mut self,
        e: &php::PhpExpr,
        declared: &mut HashSet<String>,
    ) -> Result<Stmt, String> {
        self.lift_assign_like(e, declared)
    }
}

// ── expressions (no scope state) ──

fn lift_expr(e: &php::PhpExpr) -> Result<Expr, String> {
    Ok(match e {
        php::PhpExpr::Int(n) => Expr::Int(*n, SP),
        php::PhpExpr::Float(f) => Expr::Float(*f, SP),
        php::PhpExpr::Str(s) => Expr::Str(vec![StrPart::Literal(s.clone())], SP),
        php::PhpExpr::Interp(parts) => {
            let mut out = Vec::with_capacity(parts.len());
            for part in parts {
                out.push(match part {
                    php::PhpStrPart::Lit(s) => StrPart::Literal(s.clone()),
                    php::PhpStrPart::Expr(e) => StrPart::Expr(Box::new(lift_expr(e)?)),
                });
            }
            Expr::Str(out, SP)
        }
        php::PhpExpr::Bool(b) => Expr::Bool(*b, SP),
        php::PhpExpr::Null => Expr::Null(SP),
        php::PhpExpr::Var(name) if name == "this" => Expr::This(SP),
        php::PhpExpr::Var(name) | php::PhpExpr::Name(name) => Expr::Ident(name.clone(), SP),
        php::PhpExpr::Array(elems) => lift_array(elems)?,
        php::PhpExpr::Unary { op, expr } => Expr::Unary {
            op: match op {
                php::PhpUnOp::Not => UnaryOp::Not,
                php::PhpUnOp::Neg => UnaryOp::Neg,
                php::PhpUnOp::BitNot => UnaryOp::BitNot,
            },
            expr: Box::new(lift_expr(expr)?),
            span: SP,
        },
        php::PhpExpr::Binary { op, left, right } => Expr::Binary {
            op: lift_binop(*op)?,
            lhs: Box::new(lift_expr(left)?),
            rhs: Box::new(lift_expr(right)?),
            span: SP,
        },
        // C-46: PHP `value instanceof ClassName` → Phorge's existing `instanceof` (M-RT S1).
        php::PhpExpr::InstanceOf { value, class } => Expr::InstanceOf {
            value: Box::new(lift_expr(value)?),
            type_name: class.clone(),
            span: SP,
        },
        php::PhpExpr::Assign { .. }
        | php::PhpExpr::CompoundAssign { .. }
        | php::PhpExpr::IncDec { .. } => {
            return Err("lift: assignment / `++` / `--` as a sub-expression is Tier-2".into());
        }
        php::PhpExpr::Ternary { cond, then, els } => {
            let then = then
                .as_ref()
                .ok_or("lift: elvis `?:` is Tier-2 (use a full ternary)")?;
            Expr::If {
                cond: Box::new(lift_expr(cond)?),
                then_expr: Box::new(lift_expr(then)?),
                else_expr: Box::new(lift_expr(els)?),
                span: SP,
            }
        }
        php::PhpExpr::Call { callee, args } => Expr::Call {
            callee: Box::new(lift_expr(callee)?),
            args: lift_exprs(args)?,
            span: SP,
        },
        php::PhpExpr::MethodCall {
            recv,
            name,
            args,
            nullsafe,
        } => Expr::Call {
            callee: Box::new(Expr::Member {
                object: Box::new(lift_expr(recv)?),
                name: name.clone(),
                safe: *nullsafe,
                span: SP,
            }),
            args: lift_exprs(args)?,
            span: SP,
        },
        php::PhpExpr::Member {
            recv,
            name,
            nullsafe,
        } => Expr::Member {
            object: Box::new(lift_expr(recv)?),
            name: name.clone(),
            safe: *nullsafe,
            span: SP,
        },
        php::PhpExpr::StaticCall { class, name, args } => Expr::Call {
            callee: Box::new(static_member(class, name)),
            args: lift_exprs(args)?,
            span: SP,
        },
        php::PhpExpr::ClassConst { class, name } | php::PhpExpr::StaticProp { class, name } => {
            static_member(class, name)
        }
        php::PhpExpr::Index { base, index } => Expr::Index {
            object: Box::new(lift_expr(base)?),
            index: Box::new(lift_expr(index)?),
            span: SP,
        },
        php::PhpExpr::New { class, args } => Expr::New(
            Box::new(Expr::Call {
                callee: Box::new(Expr::Ident(class.clone(), SP)),
                args: lift_exprs(args)?,
                span: SP,
            }),
            SP,
        ),
        php::PhpExpr::Match { subject, arms } => lift_match(subject, arms)?,
    })
}

fn lift_exprs(es: &[php::PhpExpr]) -> Result<Vec<Expr>, String> {
    es.iter().map(lift_expr).collect()
}

fn lift_array(elems: &[php::PhpArrayElem]) -> Result<Expr, String> {
    if elems.is_empty() {
        return Ok(Expr::List(Vec::new(), SP));
    }
    let any_key = elems.iter().any(|e| e.key.is_some());
    let all_key = elems.iter().all(|e| e.key.is_some());
    if any_key && !all_key {
        return Err("lift: a mixed keyed/positional array is Tier-2".into());
    }
    if all_key {
        let mut pairs = Vec::new();
        for e in elems {
            pairs.push((lift_expr(e.key.as_ref().unwrap())?, lift_expr(&e.value)?));
        }
        Ok(Expr::Map(pairs, SP))
    } else {
        let items: Result<Vec<_>, _> = elems.iter().map(|e| lift_expr(&e.value)).collect();
        Ok(Expr::List(items?, SP))
    }
}

fn lift_match(subject: &php::PhpExpr, arms: &[php::PhpMatchArm]) -> Result<Expr, String> {
    let mut out = Vec::new();
    for arm in arms {
        match &arm.conds {
            None => out.push(MatchArm {
                pattern: Pattern::Wildcard(SP),
                guard: None,
                body: lift_expr(&arm.body)?,
                span: SP,
            }),
            Some(conds) => {
                // PHP shares one body across comma-separated conditions; Phorge has one pattern per
                // arm, so duplicate the (cloned) body per literal condition.
                let body = lift_expr(&arm.body)?;
                for c in conds {
                    out.push(MatchArm {
                        pattern: literal_pattern(c)?,
                        guard: None,
                        body: body.clone(),
                        span: SP,
                    });
                }
            }
        }
    }
    Ok(Expr::Match {
        scrutinee: Box::new(lift_expr(subject)?),
        arms: out,
        span: SP,
    })
}

/// A PHP `match` condition must be a literal to become a Phorge pattern (a non-literal arm compares
/// by `===` at runtime — no pattern equivalent, so it's a loud Tier-2 error).
fn literal_pattern(e: &php::PhpExpr) -> Result<Pattern, String> {
    Ok(match e {
        php::PhpExpr::Int(n) => Pattern::Int(*n, SP),
        php::PhpExpr::Float(f) => Pattern::Float(*f, SP),
        php::PhpExpr::Str(s) => Pattern::Str(s.clone(), SP),
        php::PhpExpr::Bool(b) => Pattern::Bool(*b, SP),
        php::PhpExpr::Null => Pattern::Null(SP),
        _ => return Err("lift: a `match` arm with a non-literal condition is Tier-2".into()),
    })
}

// ── enums + types + small helpers ──

fn lift_enum(e: &php::PhpEnum) -> Result<EnumDecl, String> {
    if e.backing.is_some() {
        return Err(format!(
            "lift: backed enum `{}` (cases with scalar values) has no Phorge equivalent (Tier-2)",
            e.name
        ));
    }
    if !e.methods.is_empty() {
        return Err(format!(
            "lift: enum `{}` has methods — Phorge enums carry no methods (Tier-2)",
            e.name
        ));
    }
    let variants = e
        .cases
        .iter()
        .map(|c| EnumVariant {
            name: c.name.clone(),
            fields: Vec::new(),
            span: SP,
        })
        .collect();
    Ok(EnumDecl {
        vis: crate::ast::Visibility::Public,
        name: e.name.clone(),
        type_params: Vec::new(),
        variants,
        span: SP,
    })
}

fn lift_params(params: &[php::PhpParam]) -> Result<Vec<Param>, String> {
    let mut out = Vec::new();
    for p in params {
        if p.default.is_some() {
            return Err(format!(
                "lift: default value on parameter `{}` is Tier-2 (Wave 2)",
                p.name
            ));
        }
        let ty = lift_type(p.ty.as_ref().ok_or_else(|| {
            format!("lift: parameter `{}` has no type (Tier-1 is typed)", p.name)
        })?)?;
        out.push(Param {
            ty,
            name: p.name.clone(),
            span: SP,
        });
    }
    Ok(out)
}

fn lift_ctor_params(params: &[php::PhpParam]) -> Result<Vec<CtorParam>, String> {
    let mut out = Vec::new();
    for p in params {
        if p.default.is_some() {
            return Err(format!(
                "lift: default value on constructor parameter `{}` is Tier-2 (Wave 2)",
                p.name
            ));
        }
        let ty = lift_type(
            p.ty.as_ref()
                .ok_or_else(|| format!("lift: ctor parameter `{}` has no type", p.name))?,
        )?;
        let mut modifiers = Vec::new();
        if let Some(vis) = p.promotion {
            // A promoted property — mirror PHP's mutability (promoted props are mutable).
            modifiers.push(vis_modifier(vis));
            modifiers.push(Modifier::Mutable);
        }
        out.push(CtorParam {
            modifiers,
            ty,
            name: p.name.clone(),
            span: SP,
        });
    }
    Ok(out)
}

/// Lift a function/method's declared return type (C-45). A PHP `: T` lifts directly. **No** hint is
/// the trap: the old code emitted a Phorge function with no return type, which *parses* but fails the
/// checker (Tier-1 requires explicit returns) — a silent non-compiling draft. Instead: if the body
/// never returns a value, the function is provably `void` (a fact from the body, not a guess); if it
/// returns a value we cannot infer the type, so reject loudly rather than emit invalid Phorge.
fn lift_ret(
    php_ret: &Option<php::PhpType>,
    body: Option<&[php::PhpStmt]>,
) -> Result<Option<Type>, String> {
    match php_ret {
        Some(t) => Ok(Some(lift_type(t)?)),
        None => match body {
            Some(b) if !body_has_value_return(b) => Ok(Some(named("void"))),
            Some(_) => Err(
                "lift: function has no return type but returns a value — add an explicit return type (Tier-2)"
                    .into(),
            ),
            None => {
                Err("lift: an abstract method with no return type needs an explicit one (Tier-2)".into())
            }
        },
    }
}

/// Does any path in `body` `return` a *value* (`return expr;`)? Recurses through nested control flow
/// and blocks. A bare `return;` (and `echo`/`break`/`continue`/expr statements) do not count.
fn body_has_value_return(body: &[php::PhpStmt]) -> bool {
    use php::PhpStmt::{Block, Echo, Expr, For, Foreach, If, Return, While};
    body.iter().any(|s| match s {
        Return(opt) => opt.is_some(),
        If {
            then, elifs, els, ..
        } => {
            body_has_value_return(then)
                || elifs.iter().any(|(_, b)| body_has_value_return(b))
                || els.as_deref().is_some_and(body_has_value_return)
        }
        While { body, .. } | For { body, .. } | Foreach { body, .. } | Block(body) => {
            body_has_value_return(body)
        }
        Expr(_) | Echo(_) | php::PhpStmt::Break | php::PhpStmt::Continue => false,
    })
}

fn lift_type(t: &php::PhpType) -> Result<Type, String> {
    match t {
        php::PhpType::Named(name) => match name.as_str() {
            "int" | "float" | "string" | "bool" | "void" => Ok(named(name)),
            "array" => Err("lift: an `array` type needs List/Map/Set inference (Tier-2)".into()),
            "mixed" | "iterable" | "object" | "callable" | "self" | "static" | "parent" => {
                Err(format!("lift: the `{name}` type is Tier-2/Tier-3"))
            }
            // A class/enum/interface name.
            _ => Ok(named(name)),
        },
        php::PhpType::Nullable(inner) => Ok(Type::Optional {
            inner: Box::new(lift_type(inner)?),
            span: SP,
        }),
    }
}

fn lift_binop(op: php::PhpBinOp) -> Result<BinaryOp, String> {
    use php::PhpBinOp as P;
    Ok(match op {
        P::Add => BinaryOp::Add,
        P::Sub => BinaryOp::Sub,
        P::Mul => BinaryOp::Mul,
        P::Div => BinaryOp::Div,
        P::Rem => BinaryOp::Rem,
        // PHP string concatenation `.` is Phorge's type-directed `+`.
        P::Concat => BinaryOp::Add,
        // Phorge is statically typed, so loose and strict equality coincide.
        P::Eq | P::Identical => BinaryOp::Eq,
        P::NotEq | P::NotIdentical => BinaryOp::NotEq,
        P::Lt => BinaryOp::Lt,
        P::Le => BinaryOp::Le,
        P::Gt => BinaryOp::Gt,
        P::Ge => BinaryOp::Ge,
        P::And => BinaryOp::And,
        P::Or => BinaryOp::Or,
        P::Coalesce => BinaryOp::Coalesce,
        // C-47: bitwise / shift map 1:1 to Phorge's existing operators (PHP-identical int semantics).
        P::BitAnd => BinaryOp::BitAnd,
        P::BitOr => BinaryOp::BitOr,
        P::BitXor => BinaryOp::BitXor,
        P::Shl => BinaryOp::Shl,
        P::Shr => BinaryOp::Shr,
    })
}

/// A static-member access `Class.name` (covers `Class::CONST`, `Class::$prop`, and the callee of
/// `Class::method(...)`).
fn static_member(class: &str, name: &str) -> Expr {
    Expr::Member {
        object: Box::new(Expr::Ident(class.to_string(), SP)),
        name: name.to_string(),
        safe: false,
        span: SP,
    }
}

/// `Console.print(arg)` — the lift target of a PHP `echo`.
fn console_print(arg: Expr) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::Member {
            object: Box::new(Expr::Ident("Console".into(), SP)),
            name: "print".into(),
            safe: false,
            span: SP,
        }),
        args: vec![arg],
        span: SP,
    }
}

fn vis_modifier(v: php::PhpVisibility) -> Modifier {
    match v {
        php::PhpVisibility::Public => Modifier::Public,
        php::PhpVisibility::Private => Modifier::Private,
        php::PhpVisibility::Protected => Modifier::Protected,
    }
}

/// The Phorge type-name for a literal expression (used to type a lifted class `const`).
fn lit_type(e: &php::PhpExpr) -> Option<&'static str> {
    match e {
        php::PhpExpr::Int(_) => Some("int"),
        php::PhpExpr::Float(_) => Some("float"),
        php::PhpExpr::Str(_) => Some("string"),
        php::PhpExpr::Bool(_) => Some("bool"),
        _ => None,
    }
}

fn named(name: &str) -> Type {
    Type::Named {
        name: name.to_string(),
        args: Vec::new(),
        span: SP,
    }
}
