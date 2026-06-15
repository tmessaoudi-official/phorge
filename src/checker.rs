//! Sound static type-checker. Two sub-phases: `collect` (hoist decls + prelude),
//! then `check` (walk bodies). Returns all type errors at once.

use std::collections::HashMap;

use crate::ast::Program;
use crate::token::Span;
use crate::types::Ty;

/// A type error with source position. Mirrors `parser::ParseError`.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeError {
    pub message: String,
    pub line: u32,
    pub col: u32,
}

struct FnSig {
    params: Vec<Ty>,
    ret: Ty,
}

struct EnumInfo {
    /// variant name -> field types (in declaration order)
    variants: HashMap<String, Vec<Ty>>,
}

struct ClassInfo {
    fields: HashMap<String, Ty>,
    methods: HashMap<String, FnSig>,
    /// constructor parameter types, for `ClassName(args)` calls
    ctor: Vec<Ty>,
}

pub struct Checker {
    funcs: HashMap<String, FnSig>,
    enums: HashMap<String, EnumInfo>,
    classes: HashMap<String, ClassInfo>,
    /// lexical block scopes; last is innermost
    scopes: Vec<HashMap<String, Ty>>,
    errors: Vec<TypeError>,
    /// return type of the function/method currently being checked
    cur_ret: Ty,
    /// class currently being checked (for `this` and bare field refs)
    cur_class: Option<String>,
}

impl Checker {
    fn new() -> Self {
        Checker {
            funcs: HashMap::new(),
            enums: HashMap::new(),
            classes: HashMap::new(),
            scopes: Vec::new(),
            errors: Vec::new(),
            cur_ret: Ty::Unit,
            cur_class: None,
        }
    }

    /// Record an error and return the poison type so callers can keep going.
    fn err(&mut self, span: Span, msg: impl Into<String>) -> Ty {
        self.errors.push(TypeError {
            message: msg.into(),
            line: span.line,
            col: span.col,
        });
        Ty::Error
    }

    /// Resolve an AST type annotation to an internal `Ty`. Records and poisons on
    /// unknown / deferred types.
    fn resolve_type(&mut self, ty: &crate::ast::Type) -> Ty {
        use crate::ast::Type;
        match ty {
            Type::Optional { span, .. } => {
                self.err(*span, "optional types are not yet supported in M1")
            }
            Type::Named { name, args, span } => match name.as_str() {
                "int" => self.no_args(name, args, *span, Ty::Int),
                "float" => self.no_args(name, args, *span, Ty::Float),
                "bool" => self.no_args(name, args, *span, Ty::Bool),
                "string" => self.no_args(name, args, *span, Ty::String),
                "List" => Ty::List(Box::new(self.one_arg(name, args, *span))),
                "Set" => Ty::Set(Box::new(self.one_arg(name, args, *span))),
                "Map" => {
                    if args.len() != 2 {
                        return self.err(
                            *span,
                            format!("Map expects 2 type arguments, got {}", args.len()),
                        );
                    }
                    let k = self.resolve_type(&args[0]);
                    let v = self.resolve_type(&args[1]);
                    Ty::Map(Box::new(k), Box::new(v))
                }
                "decimal" | "double" | "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32"
                | "u64" => self.err(
                    *span,
                    format!("the numeric type `{name}` is not yet supported in M1"),
                ),
                other => {
                    if self.enums.contains_key(other) || self.classes.contains_key(other) {
                        Ty::Named(other.to_string())
                    } else {
                        self.err(*span, format!("unknown type `{other}`"))
                    }
                }
            },
        }
    }

    fn no_args(&mut self, name: &str, args: &[crate::ast::Type], span: Span, ty: Ty) -> Ty {
        if args.is_empty() {
            ty
        } else {
            self.err(span, format!("type `{name}` takes no type arguments"))
        }
    }

    fn one_arg(&mut self, name: &str, args: &[crate::ast::Type], span: Span) -> Ty {
        if args.len() != 1 {
            self.err(
                span,
                format!("{name} expects 1 type argument, got {}", args.len()),
            );
            Ty::Error
        } else {
            self.resolve_type(&args[0])
        }
    }

    /// Register builtin functions available without explicit user definition.
    fn register_prelude(&mut self) {
        self.funcs.insert(
            "println".into(),
            FnSig {
                params: vec![Ty::String],
                ret: Ty::Unit,
            },
        );
    }

    /// Phase 1 — hoist all top-level declarations and the builtin prelude.
    fn collect(&mut self, program: &Program) {
        use crate::ast::Item;
        self.register_prelude();
        for item in &program.items {
            match item {
                Item::Function(f) => self.collect_function(f),
                Item::Enum(e) => self.collect_enum(e),
                Item::Class(c) => self.collect_class(c),
                Item::Import { .. } => {} // module resolution deferred; prelude covers println
            }
        }
    }

    fn collect_function(&mut self, f: &crate::ast::FunctionDecl) {
        if self.funcs.contains_key(&f.name) {
            self.err(
                f.span,
                format!(
                    "function overloading is not yet supported in M1 (`{}` already defined)",
                    f.name
                ),
            );
            return;
        }
        let params = f.params.iter().map(|p| self.resolve_type(&p.ty)).collect();
        let ret = match &f.ret {
            Some(t) => self.resolve_type(t),
            None => Ty::Unit,
        };
        self.funcs.insert(f.name.clone(), FnSig { params, ret });
    }

    fn collect_enum(&mut self, e: &crate::ast::EnumDecl) {
        if self.enums.contains_key(&e.name) || self.classes.contains_key(&e.name) {
            self.err(e.span, format!("type `{}` is already defined", e.name));
            return;
        }
        // Register the name first so variant field types can reference the enum itself.
        self.enums.insert(
            e.name.clone(),
            EnumInfo {
                variants: HashMap::new(),
            },
        );
        let mut variants = HashMap::new();
        for v in &e.variants {
            let fields = v.fields.iter().map(|p| self.resolve_type(&p.ty)).collect();
            variants.insert(v.name.clone(), fields);
        }
        self.enums.get_mut(&e.name).unwrap().variants = variants;
    }

    fn collect_class(&mut self, c: &crate::ast::ClassDecl) {
        use crate::ast::ClassMember;
        if self.classes.contains_key(&c.name) || self.enums.contains_key(&c.name) {
            self.err(c.span, format!("type `{}` is already defined", c.name));
            return;
        }
        // Register the name first so members can reference the class type itself.
        self.classes.insert(
            c.name.clone(),
            ClassInfo {
                fields: HashMap::new(),
                methods: HashMap::new(),
                ctor: Vec::new(),
            },
        );
        use crate::ast::Modifier;
        let mut fields = HashMap::new();
        let mut methods = HashMap::new();
        let mut ctor = Vec::new();
        // Promoted ctor params (carrying a visibility modifier) also become fields,
        // matching the evaluator's runtime promotion (EV-4). Deferred to after the
        // member loop via `or_insert` so an explicit `Field` decl of the same name
        // stays authoritative regardless of member order.
        let mut promoted: Vec<(String, Ty)> = Vec::new();
        for m in &c.members {
            match m {
                ClassMember::Field { ty, name, .. } => {
                    let fty = self.resolve_type(ty);
                    fields.insert(name.clone(), fty);
                }
                ClassMember::Constructor { params, .. } => {
                    // Resolve each param type once; reuse for both the ctor signature
                    // and field promotion to avoid duplicate "unknown type" errors.
                    ctor = params
                        .iter()
                        .map(|p| {
                            let ty = self.resolve_type(&p.ty);
                            if p.modifiers.iter().any(|m| {
                                matches!(
                                    m,
                                    Modifier::Public | Modifier::Private | Modifier::Protected
                                )
                            }) {
                                promoted.push((p.name.clone(), ty.clone()));
                            }
                            ty
                        })
                        .collect();
                }
                ClassMember::Method(f) => {
                    let p = f.params.iter().map(|p| self.resolve_type(&p.ty)).collect();
                    let ret = match &f.ret {
                        Some(t) => self.resolve_type(t),
                        None => Ty::Unit,
                    };
                    methods.insert(f.name.clone(), FnSig { params: p, ret });
                }
            }
        }
        // Explicit field decls win: only insert a promoted field if not already declared.
        for (name, ty) in promoted {
            fields.entry(name).or_insert(ty);
        }
        let info = self.classes.get_mut(&c.name).unwrap();
        info.fields = fields;
        info.methods = methods;
        info.ctor = ctor;
    }

    /// Phase 2 — check every function/method body.
    fn check_program(&mut self, program: &Program) {
        use crate::ast::{ClassMember, Item};
        for item in &program.items {
            match item {
                Item::Function(f) => self.check_function(f),
                Item::Class(c) => {
                    let prev = self.cur_class.replace(c.name.clone());
                    for m in &c.members {
                        match m {
                            ClassMember::Method(f) => self.check_function(f),
                            ClassMember::Constructor { params, body, .. } => {
                                let prev_ret = std::mem::replace(&mut self.cur_ret, Ty::Unit);
                                self.push_scope();
                                // constructor params are in scope inside its body
                                let ctor = self
                                    .classes
                                    .get(&c.name)
                                    .map(|info| info.ctor.clone())
                                    .unwrap_or_default();
                                for (p, t) in params.iter().zip(ctor) {
                                    self.declare(&p.name, t);
                                }
                                for s in body {
                                    self.check_stmt(s);
                                }
                                self.pop_scope();
                                self.cur_ret = prev_ret;
                            }
                            ClassMember::Field { .. } => {}
                        }
                    }
                    self.cur_class = prev;
                }
                Item::Enum(_) | Item::Import { .. } => {}
            }
        }
    }

    /// Check one free function or method body. Seeds a fresh scope with params.
    fn check_function(&mut self, f: &crate::ast::FunctionDecl) {
        let ret = match &f.ret {
            Some(t) => self.resolve_type(t),
            None => Ty::Unit,
        };
        let prev_ret = std::mem::replace(&mut self.cur_ret, ret);
        self.push_scope();
        for p in &f.params {
            let pty = self.resolve_type(&p.ty);
            self.declare(&p.name, pty);
        }
        for s in &f.body {
            self.check_stmt(s);
        }
        self.pop_scope();
        self.cur_ret = prev_ret;
    }

    // ---- scopes ----
    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }
    fn pop_scope(&mut self) {
        self.scopes.pop();
    }
    fn declare(&mut self, name: &str, ty: Ty) {
        if let Some(top) = self.scopes.last_mut() {
            top.insert(name.to_string(), ty);
        }
    }
    fn lookup(&self, name: &str) -> Option<Ty> {
        for scope in self.scopes.iter().rev() {
            if let Some(t) = scope.get(name) {
                return Some(t.clone());
            }
        }
        // bare field reference inside a method
        if let Some(cls) = &self.cur_class {
            if let Some(info) = self.classes.get(cls) {
                if let Some(t) = info.fields.get(name) {
                    return Some(t.clone());
                }
            }
        }
        None
    }

    // ---- statements ----
    fn check_block(&mut self, stmts: &[crate::ast::Stmt]) {
        self.push_scope();
        for s in stmts {
            self.check_stmt(s);
        }
        self.pop_scope();
    }

    fn check_stmt(&mut self, stmt: &crate::ast::Stmt) {
        use crate::ast::Stmt;
        match stmt {
            Stmt::VarDecl {
                ty,
                name,
                init,
                span,
            } => {
                let declared = self.resolve_type(ty);
                let actual = self.check_expr(init);
                if !Ty::assignable(&actual, &declared) {
                    self.err(*span, format!("expected `{declared}`, found `{actual}`"));
                }
                self.declare(name, declared);
            }
            Stmt::Return { value, span } => {
                let actual = match value {
                    Some(e) => self.check_expr(e),
                    None => Ty::Unit,
                };
                let want = self.cur_ret.clone();
                if !Ty::assignable(&actual, &want) {
                    self.err(*span, format!("expected `{want}`, found `{actual}`"));
                }
            }
            Stmt::If {
                cond,
                then_block,
                else_block,
                span,
            } => {
                let c = self.check_expr(cond);
                if !Ty::assignable(&c, &Ty::Bool) {
                    self.err(*span, format!("`if` condition must be `bool`, found `{c}`"));
                }
                self.check_block(then_block);
                if let Some(eb) = else_block {
                    self.check_block(eb);
                }
            }
            Stmt::For { .. } => self.check_for(stmt), // implemented in Task 5
            Stmt::Block(stmts, _) => self.check_block(stmts),
            Stmt::Expr(e, _) => {
                self.check_expr(e);
            }
        }
    }

    // ---- expressions ----
    fn check_expr(&mut self, expr: &crate::ast::Expr) -> Ty {
        use crate::ast::Expr;
        match expr {
            Expr::Int(_, _) => Ty::Int,
            Expr::Float(_, _) => Ty::Float,
            Expr::Bool(_, _) => Ty::Bool,
            Expr::Null(span) => {
                self.err(*span, "null / optional values are not yet supported in M1")
            }
            Expr::Str(parts, _) => self.check_str(parts), // Task 7
            Expr::Ident(name, span) => match self.lookup(name) {
                Some(t) => t,
                None => self.err(*span, format!("unknown identifier `{name}`")),
            },
            Expr::This(span) => match &self.cur_class {
                Some(c) => Ty::Named(c.clone()),
                None => self.err(*span, "`this` is only valid inside a method"),
            },
            Expr::List(elems, span) => self.check_list(elems, *span), // Task 5
            Expr::Unary { op, expr, span } => self.check_unary(*op, expr, *span),
            Expr::Binary { op, lhs, rhs, span } => self.check_binary(*op, lhs, rhs, *span),
            Expr::Call { callee, args, span } => self.check_call(callee, args, *span), // Task 4
            Expr::Member { object, name, span } => self.check_member(object, name, *span), // Task 6
            Expr::Index {
                object,
                index,
                span,
            } => self.check_index(object, index, *span), // Task 5
            Expr::Match {
                scrutinee,
                arms,
                span,
            } => self.check_match(scrutinee, arms, *span), // Task 8
        }
    }

    fn check_unary(&mut self, op: crate::ast::UnaryOp, expr: &crate::ast::Expr, span: Span) -> Ty {
        use crate::ast::UnaryOp;
        let t = self.check_expr(expr);
        if t == Ty::Error {
            return Ty::Error;
        }
        match op {
            UnaryOp::Neg if t == Ty::Int || t == Ty::Float => t,
            UnaryOp::Neg => self.err(
                span,
                format!("unary `-` requires int or float, found `{t}`"),
            ),
            UnaryOp::Not if t == Ty::Bool => Ty::Bool,
            UnaryOp::Not => self.err(span, format!("unary `!` requires `bool`, found `{t}`")),
        }
    }

    fn check_binary(
        &mut self,
        op: crate::ast::BinaryOp,
        lhs: &crate::ast::Expr,
        rhs: &crate::ast::Expr,
        span: Span,
    ) -> Ty {
        use crate::ast::BinaryOp;
        let l = self.check_expr(lhs);
        let r = self.check_expr(rhs);
        if l == Ty::Error || r == Ty::Error {
            return match op {
                BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::Lt
                | BinaryOp::Gt
                | BinaryOp::Le
                | BinaryOp::Ge
                | BinaryOp::And
                | BinaryOp::Or
                | BinaryOp::Is => Ty::Bool,
                _ => Ty::Error,
            };
        }
        match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
                if (l == Ty::Int && r == Ty::Int) || (l == Ty::Float && r == Ty::Float) {
                    l
                } else {
                    self.err(span, format!("arithmetic requires matching int or float operands, found `{l}` and `{r}`"))
                }
            }
            BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge => {
                if (l == Ty::Int && r == Ty::Int) || (l == Ty::Float && r == Ty::Float) {
                    Ty::Bool
                } else {
                    self.err(span, format!("comparison requires matching int or float operands, found `{l}` and `{r}`"));
                    Ty::Bool
                }
            }
            BinaryOp::Eq | BinaryOp::NotEq => {
                if l != r {
                    self.err(
                        span,
                        format!(
                            "cross-type comparison requires explicit conversion (`{l}` vs `{r}`)"
                        ),
                    );
                }
                Ty::Bool
            }
            BinaryOp::And | BinaryOp::Or => {
                if l != Ty::Bool || r != Ty::Bool {
                    self.err(
                        span,
                        format!("`&&`/`||` require `bool` operands, found `{l}` and `{r}`"),
                    );
                }
                Ty::Bool
            }
            BinaryOp::Is => Ty::Bool,
            BinaryOp::Pipe => self.err(span, "the pipe operator `|>` is not yet supported in M1"),
        }
    }

    // ---- stubs replaced in later tasks ----
    fn check_str(&mut self, parts: &[crate::ast::StrPart]) -> Ty {
        use crate::ast::StrPart;
        for part in parts {
            if let StrPart::Expr(e) = part {
                let t = self.check_expr(e);
                let ok = matches!(t, Ty::Int | Ty::Float | Ty::Bool | Ty::String | Ty::Error);
                if !ok {
                    let sp = Self::expr_span(e);
                    self.err(sp, format!("type `{t}` cannot be interpolated into a string (only primitives auto-stringify in M1)"));
                }
            }
        }
        Ty::String
    }

    /// The source span of an expression (used to position errors precisely).
    fn expr_span(e: &crate::ast::Expr) -> Span {
        use crate::ast::Expr;
        match e {
            Expr::Int(_, s)
            | Expr::Float(_, s)
            | Expr::Bool(_, s)
            | Expr::Str(_, s)
            | Expr::Ident(_, s)
            | Expr::List(_, s) => *s,
            Expr::Null(s) | Expr::This(s) => *s,
            Expr::Unary { span, .. }
            | Expr::Binary { span, .. }
            | Expr::Call { span, .. }
            | Expr::Member { span, .. }
            | Expr::Index { span, .. }
            | Expr::Match { span, .. } => *span,
        }
    }
    fn check_list(&mut self, elems: &[crate::ast::Expr], span: Span) -> Ty {
        if elems.is_empty() {
            // empty list element type cannot be inferred without an expected type;
            // the §6 sample has no empty list (YAGNI to thread expected types now).
            return self.err(span, "cannot infer element type of empty list literal");
        }
        let first = self.check_expr(&elems[0]);
        for e in &elems[1..] {
            let t = self.check_expr(e);
            if !Ty::assignable(&t, &first) && !Ty::assignable(&first, &t) {
                self.err(
                    span,
                    format!("list elements must share one type; found `{first}` and `{t}`"),
                );
            }
        }
        Ty::List(Box::new(first))
    }
    fn check_index(
        &mut self,
        object: &crate::ast::Expr,
        index: &crate::ast::Expr,
        span: Span,
    ) -> Ty {
        let obj = self.check_expr(object);
        let idx = self.check_expr(index);
        match obj {
            Ty::List(elem) => {
                if !Ty::assignable(&idx, &Ty::Int) {
                    self.err(span, format!("list index must be `int`, found `{idx}`"));
                }
                *elem
            }
            Ty::Map(..) => self.err(span, "Map indexing is not yet supported in M1"),
            Ty::Error => Ty::Error,
            other => self.err(span, format!("type `{other}` cannot be indexed")),
        }
    }
    fn check_call(
        &mut self,
        callee: &crate::ast::Expr,
        args: &[crate::ast::Expr],
        span: Span,
    ) -> Ty {
        use crate::ast::Expr;
        match callee {
            Expr::Ident(name, _) => self.check_named_call(name, args, span),
            Expr::Member { object, name, .. } => self.check_method_call(object, name, args, span), // Task 6
            other => {
                for a in args {
                    self.check_expr(a);
                }
                let _ = other;
                self.err(span, "expression is not callable")
            }
        }
    }

    /// `name(args)` — a free function, enum-variant constructor (Task 5), or class
    /// constructor (Task 6). Free-function case here.
    fn check_named_call(&mut self, name: &str, args: &[crate::ast::Expr], span: Span) -> Ty {
        if let Some(t) = self.try_variant_or_class_call(name, args, span) {
            return t;
        }
        let sig = match self.funcs.get(name) {
            Some(s) => (s.params.clone(), s.ret.clone()),
            None => {
                for a in args {
                    self.check_expr(a);
                }
                return self.err(span, format!("unknown function `{name}`"));
            }
        };
        self.check_args(name, &sig.0, args, span);
        sig.1
    }

    /// Check call arguments against expected parameter types.
    fn check_args(&mut self, name: &str, params: &[Ty], args: &[crate::ast::Expr], span: Span) {
        if params.len() != args.len() {
            self.err(
                span,
                format!(
                    "`{name}` expects {} argument(s), found {}",
                    params.len(),
                    args.len()
                ),
            );
            for a in args {
                self.check_expr(a);
            }
            return;
        }
        for (i, (param, arg)) in params.iter().zip(args).enumerate() {
            let at = self.check_expr(arg);
            if !Ty::assignable(&at, param) {
                self.err(
                    span,
                    format!(
                        "`{name}` argument {} expects `{param}`, found `{at}`",
                        i + 1
                    ),
                );
            }
        }
    }

    /// Returns `Some(ret)` if `name` is an enum variant or class constructor.
    fn try_variant_or_class_call(
        &mut self,
        name: &str,
        args: &[crate::ast::Expr],
        span: Span,
    ) -> Option<Ty> {
        // enum variant constructor: find the (unique) enum that owns this variant name
        let owner = self
            .enums
            .iter()
            .find(|(_, info)| info.variants.contains_key(name))
            .map(|(enum_name, info)| (enum_name.clone(), info.variants[name].clone()));
        if let Some((enum_name, fields)) = owner {
            self.check_args(name, &fields, args, span);
            return Some(Ty::Named(enum_name));
        }
        // class constructor: `ClassName(args)`
        if let Some(info) = self.classes.get(name) {
            let ctor = info.ctor.clone();
            self.check_args(name, &ctor, args, span);
            return Some(Ty::Named(name.to_string()));
        }
        None
    }

    fn check_method_call(
        &mut self,
        object: &crate::ast::Expr,
        name: &str,
        args: &[crate::ast::Expr],
        span: Span,
    ) -> Ty {
        let obj = self.check_expr(object);
        match obj {
            Ty::Named(cls) => {
                let sig = self
                    .classes
                    .get(&cls)
                    .and_then(|info| info.methods.get(name))
                    .map(|s| (s.params.clone(), s.ret.clone()));
                match sig {
                    Some((params, ret)) => {
                        self.check_args(name, &params, args, span);
                        ret
                    }
                    None => {
                        for a in args {
                            self.check_expr(a);
                        }
                        self.err(span, format!("type `{cls}` has no method `{name}`"))
                    }
                }
            }
            Ty::Error => {
                for a in args {
                    self.check_expr(a);
                }
                Ty::Error
            }
            other => {
                for a in args {
                    self.check_expr(a);
                }
                self.err(span, format!("type `{other}` has no method `{name}`"))
            }
        }
    }
    fn check_member(&mut self, object: &crate::ast::Expr, name: &str, span: Span) -> Ty {
        let obj = self.check_expr(object);
        match obj {
            Ty::Named(cls) => {
                if let Some(info) = self.classes.get(&cls) {
                    if let Some(t) = info.fields.get(name) {
                        return t.clone();
                    }
                    return self.err(span, format!("type `{cls}` has no field `{name}`"));
                }
                self.err(span, format!("type `{cls}` has no field `{name}`"))
            }
            Ty::Error => Ty::Error,
            other => self.err(span, format!("type `{other}` has no field `{name}`")),
        }
    }
    fn check_for(&mut self, stmt: &crate::ast::Stmt) {
        if let crate::ast::Stmt::For {
            ty,
            name,
            iter,
            body,
            span,
        } = stmt
        {
            let declared = self.resolve_type(ty);
            let iter_ty = self.check_expr(iter);
            let elem = match iter_ty {
                Ty::List(e) => *e,
                Ty::Error => Ty::Error,
                other => {
                    self.err(
                        *span,
                        format!("`for`-`in` requires a List, found `{other}`"),
                    );
                    Ty::Error
                }
            };
            if !Ty::assignable(&elem, &declared) {
                self.err(
                    *span,
                    format!("loop variable `{name}` declared `{declared}` but iterating `{elem}`"),
                );
            }
            self.push_scope();
            self.declare(name, declared);
            for s in body {
                self.check_stmt(s);
            }
            self.pop_scope();
        }
    }
    fn check_match(
        &mut self,
        scrutinee: &crate::ast::Expr,
        arms: &[crate::ast::MatchArm],
        span: Span,
    ) -> Ty {
        use crate::ast::Pattern;
        let scrut = self.check_expr(scrutinee);

        let mut result: Option<Ty> = None;
        let mut covered: Vec<String> = Vec::new();
        let mut has_catch_all = false;

        for arm in arms {
            if matches!(arm.pattern, Pattern::Wildcard(_) | Pattern::Binding { .. }) {
                has_catch_all = true;
            }
            if let Pattern::Variant { name, .. } = &arm.pattern {
                covered.push(name.clone());
            }
            // each arm gets its own scope for pattern bindings
            self.push_scope();
            self.check_pattern(&arm.pattern, &scrut);
            let body_ty = self.check_expr(&arm.body);
            self.pop_scope();

            match &result {
                None => result = Some(body_ty),
                Some(first) => {
                    if !Ty::assignable(&body_ty, first) && !Ty::assignable(first, &body_ty) {
                        self.err(
                            span,
                            format!(
                                "match arms must share one type; found `{first}` and `{body_ty}`"
                            ),
                        );
                    }
                }
            }
        }

        // exhaustiveness
        if !has_catch_all {
            match &scrut {
                Ty::Named(enum_name) if self.enums.contains_key(enum_name) => {
                    let all: Vec<String> = self.enums[enum_name].variants.keys().cloned().collect();
                    let missing: Vec<String> =
                        all.into_iter().filter(|v| !covered.contains(v)).collect();
                    if !missing.is_empty() {
                        self.err(
                            span,
                            format!("non-exhaustive match: missing {}", missing.join(", ")),
                        );
                    }
                }
                Ty::Error => {}
                _ => {
                    self.err(
                        span,
                        "non-exhaustive match: add a `_` wildcard arm for non-enum scrutinees",
                    );
                }
            }
        }

        result.unwrap_or(Ty::Error)
    }

    /// Check a pattern against the scrutinee type, declaring its bindings into the
    /// current scope.
    fn check_pattern(&mut self, pat: &crate::ast::Pattern, scrut: &Ty) {
        use crate::ast::Pattern;
        match pat {
            Pattern::Wildcard(_) => {}
            Pattern::Binding { name, .. } => self.declare(name, scrut.clone()),
            Pattern::Int(_, span) => self.expect_prim(scrut, &Ty::Int, *span),
            Pattern::Float(_, span) => self.expect_prim(scrut, &Ty::Float, *span),
            Pattern::Str(_, span) => self.expect_prim(scrut, &Ty::String, *span),
            Pattern::Bool(_, span) => self.expect_prim(scrut, &Ty::Bool, *span),
            Pattern::Null(span) => {
                self.err(
                    *span,
                    "null patterns / optionals are not yet supported in M1",
                );
            }
            Pattern::Variant { name, fields, span } => {
                let enum_name = match scrut {
                    Ty::Named(n) if self.enums.contains_key(n) => n.clone(),
                    Ty::Error => return,
                    other => {
                        self.err(*span, format!("variant pattern `{name}` requires an enum scrutinee, found `{other}`"));
                        return;
                    }
                };
                let field_tys = match self.enums[&enum_name].variants.get(name) {
                    Some(f) => f.clone(),
                    None => {
                        self.err(*span, format!("enum `{enum_name}` has no variant `{name}`"));
                        return;
                    }
                };
                if field_tys.len() != fields.len() {
                    self.err(
                        *span,
                        format!(
                            "variant `{name}` expects {} field(s), found {}",
                            field_tys.len(),
                            fields.len()
                        ),
                    );
                    return;
                }
                for (fp, ft) in fields.iter().zip(field_tys) {
                    self.check_pattern(fp, &ft);
                }
            }
        }
    }

    fn expect_prim(&mut self, scrut: &Ty, want: &Ty, span: Span) {
        if *scrut != Ty::Error && scrut != want {
            self.err(
                span,
                format!("pattern of type `{want}` cannot match scrutinee of type `{scrut}`"),
            );
        }
    }
}

/// Type-check a whole program. `Ok(())` means it is well-typed; otherwise every
/// detected error is returned.
pub fn check(program: &Program) -> Result<(), Vec<TypeError>> {
    let mut c = Checker::new();
    c.collect(program);
    c.check_program(program);
    if c.errors.is_empty() {
        Ok(())
    } else {
        Err(c.errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    use crate::parser::Parser;

    /// Lex + parse `src` into a Program, panicking on lex/parse failure (tests here
    /// only care about type-checking).
    fn prog(src: &str) -> Program {
        let tokens = lex(src).expect("lex ok");
        Parser::new(tokens).parse_program().expect("parse ok")
    }

    /// Type-check `src` and return the errors (empty == well-typed).
    fn errors_of(src: &str) -> Vec<TypeError> {
        match check(&prog(src)) {
            Ok(()) => Vec::new(),
            Err(e) => e,
        }
    }

    #[test]
    fn empty_program_checks_ok() {
        assert!(errors_of("").is_empty());
    }

    #[test]
    fn resolve_maps_primitives_and_list() {
        use crate::ast::Type;
        use crate::token::Span;
        let sp = Span {
            start: 0,
            len: 1,
            line: 1,
            col: 1,
        };
        let mut c = Checker::new();
        assert_eq!(
            c.resolve_type(&Type::Named {
                name: "int".into(),
                args: vec![],
                span: sp
            }),
            Ty::Int
        );
        let list = Type::Named {
            name: "List".into(),
            args: vec![Type::Named {
                name: "int".into(),
                args: vec![],
                span: sp,
            }],
            span: sp,
        };
        assert_eq!(c.resolve_type(&list), Ty::List(Box::new(Ty::Int)));
        assert_eq!(c.errors.len(), 0);
    }

    #[test]
    fn unknown_type_in_var_decl_errors() {
        let errs = errors_of("function main() { Nope n = 0; }");
        assert!(
            errs.iter().any(|e| e.message.contains("unknown type")),
            "{errs:?}"
        );
    }

    #[test]
    fn optional_type_is_deferred_corner() {
        let errs = errors_of("function main() { int? n = 0; }");
        assert!(
            errs.iter()
                .any(|e| e.message.contains("optional types are not yet supported")),
            "{errs:?}"
        );
    }

    #[test]
    fn decimal_type_is_deferred_corner() {
        let errs = errors_of("function main() { decimal d = 0; }");
        assert!(
            errs.iter()
                .any(|e| e.message.contains("decimal") && e.message.contains("not yet supported")),
            "{errs:?}"
        );
    }

    #[test]
    fn var_decl_type_mismatch_errors() {
        let errs = errors_of("function main() { int n = true; }");
        assert!(
            errs.iter().any(|e| e.message.contains("expected `int`")),
            "{errs:?}"
        );
    }

    #[test]
    fn good_var_decl_and_arithmetic_ok() {
        assert!(errors_of("function main() { int a = 1; int b = a + 2; }").is_empty());
    }

    #[test]
    fn arithmetic_mixing_int_float_errors() {
        let errs = errors_of("function main() { float x = 1 + 2.0; }");
        assert!(!errs.is_empty(), "mixing int and float must error");
    }

    #[test]
    fn if_condition_must_be_bool() {
        let errs = errors_of("function main() { if (1) { } }");
        assert!(
            errs.iter()
                .any(|e| e.message.contains("condition must be `bool`")),
            "{errs:?}"
        );
    }

    #[test]
    fn equality_requires_same_type() {
        let errs = errors_of("function main() { bool b = 1 == true; }");
        assert!(
            errs.iter().any(|e| e.message.contains("cross-type")),
            "{errs:?}"
        );
    }

    #[test]
    fn unknown_identifier_errors() {
        let errs = errors_of("function main() { int n = missing; }");
        assert!(
            errs.iter()
                .any(|e| e.message.contains("unknown identifier")),
            "{errs:?}"
        );
    }

    #[test]
    fn block_scoping_pops_bindings() {
        let errs = errors_of("function main() { { int x = 1; } int y = x; }");
        assert!(
            errs.iter()
                .any(|e| e.message.contains("unknown identifier")),
            "{errs:?}"
        );
    }

    #[test]
    fn return_type_checked_against_signature() {
        let errs = errors_of("function f() -> int { return true; }");
        assert!(
            errs.iter().any(|e| e.message.contains("expected `int`")),
            "{errs:?}"
        );
    }

    #[test]
    fn function_call_arity_and_type_checked() {
        assert!(errors_of(
            "function inc(int n) -> int { return n + 1; } function main() { int x = inc(1); }"
        )
        .is_empty());
        let bad_arity = errors_of(
            "function inc(int n) -> int { return n; } function main() { int x = inc(1, 2); }",
        );
        assert!(
            bad_arity
                .iter()
                .any(|e| e.message.contains("expects 1 argument")),
            "{bad_arity:?}"
        );
        let bad_type = errors_of(
            "function inc(int n) -> int { return n; } function main() { int x = inc(true); }",
        );
        assert!(
            bad_type.iter().any(|e| e.message.contains("argument 1")),
            "{bad_type:?}"
        );
    }

    #[test]
    fn unknown_function_call_errors() {
        let errs = errors_of("function main() { nope(); }");
        assert!(
            errs.iter().any(|e| e.message.contains("unknown function")),
            "{errs:?}"
        );
    }

    #[test]
    fn duplicate_function_is_overloading_corner() {
        let errs = errors_of("function f() {} function f(int n) {}");
        assert!(
            errs.iter()
                .any(|e| e.message.contains("overloading is not yet supported")),
            "{errs:?}"
        );
    }

    #[test]
    fn println_accepts_string() {
        assert!(errors_of(r#"function main() { println("hi"); }"#).is_empty());
    }

    const SHAPE: &str = "enum Shape { Circle(float radius), Rect(float w, float h), }";

    #[test]
    fn variant_constructor_returns_enum() {
        let src = format!("{SHAPE} function main() {{ Shape s = Circle(2.0); }}");
        assert!(errors_of(&src).is_empty());
    }

    #[test]
    fn variant_constructor_arg_type_checked() {
        let src = format!("{SHAPE} function main() {{ Shape s = Circle(true); }}");
        let errs = errors_of(&src);
        assert!(
            errs.iter().any(|e| e.message.contains("argument 1")),
            "{errs:?}"
        );
    }

    #[test]
    fn list_literal_unifies_elements() {
        let src = format!(
            "{SHAPE} function main() {{ List<Shape> xs = [Circle(1.0), Rect(2.0, 3.0)]; }}"
        );
        assert!(errors_of(&src).is_empty());
    }

    #[test]
    fn list_literal_mixed_elements_error() {
        let errs = errors_of("function main() { List<int> xs = [1, true]; }");
        assert!(
            errs.iter().any(|e| e.message.contains("list elements")),
            "{errs:?}"
        );
    }

    #[test]
    fn for_in_binds_element_type() {
        let src = format!(
            "{SHAPE} function area(Shape s) -> float {{ return 0.0; }} \
             function main() {{ List<Shape> xs = [Circle(1.0)]; for (Shape s in xs) {{ float a = area(s); }} }}"
        );
        assert!(errors_of(&src).is_empty(), "{:?}", errors_of(&src));
    }

    #[test]
    fn for_in_requires_list() {
        let errs = errors_of("function main() { for (int i in 5) { } }");
        assert!(
            errs.iter()
                .any(|e| e.message.contains("`for`-`in` requires a List")),
            "{errs:?}"
        );
    }

    #[test]
    fn list_indexing_yields_element() {
        assert!(errors_of("function main() { List<int> xs = [1, 2]; int y = xs[0]; }").is_empty());
    }

    const GREETER: &str = "class Greeter { private string name; constructor(string name) {} function greet() -> string { return \"Hi\"; } }";

    #[test]
    fn constructor_call_and_method_call_ok() {
        let src = format!(
            "{GREETER} function main() {{ Greeter g = Greeter(\"Tak\"); string s = g.greet(); }}"
        );
        assert!(errors_of(&src).is_empty(), "{:?}", errors_of(&src));
    }

    #[test]
    fn constructor_arg_type_checked() {
        let src = format!("{GREETER} function main() {{ Greeter g = Greeter(123); }}");
        let errs = errors_of(&src);
        assert!(
            errs.iter().any(|e| e.message.contains("argument 1")),
            "{errs:?}"
        );
    }

    #[test]
    fn unknown_method_errors() {
        let src =
            format!("{GREETER} function main() {{ Greeter g = Greeter(\"x\"); g.missing(); }}");
        let errs = errors_of(&src);
        assert!(
            errs.iter()
                .any(|e| e.message.contains("no method `missing`")),
            "{errs:?}"
        );
    }

    #[test]
    fn field_access_typed() {
        let src = "class Box { public int n; constructor(int n) {} } function main() { Box b = Box(1); int x = b.n; }";
        assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
    }

    #[test]
    fn bare_field_visible_in_method() {
        let src = "class C { private string name; constructor(string name) {} function who() -> string { return name; } }";
        assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
    }

    #[test]
    fn this_outside_method_errors() {
        let errs = errors_of("function main() { string s = this; }");
        assert!(
            errs.iter().any(|e| e.message.contains("`this`")),
            "{errs:?}"
        );
    }

    #[test]
    fn interpolation_allows_primitives() {
        assert!(errors_of("function main() { float x = 1.5; string s = \"v = {x}\"; }").is_empty());
        assert!(errors_of("function main() { int n = 3; string s = \"n = {n}\"; }").is_empty());
    }

    #[test]
    fn interpolation_rejects_objects() {
        let src = "class C { private int n; constructor(int n) {} } function main() { C c = C(1); string s = \"{c}\"; }";
        let errs = errors_of(src);
        assert!(
            errs.iter()
                .any(|e| e.message.contains("cannot be interpolated")),
            "{errs:?}"
        );
    }

    #[test]
    fn match_over_enum_is_typed_and_exhaustive() {
        let src = format!(
            "{SHAPE} function area(Shape s) -> float {{ \
               return match s {{ Circle(r) => 3.14 * r * r, Rect(w, h) => w * h, }}; }}"
        );
        assert!(errors_of(&src).is_empty(), "{:?}", errors_of(&src));
    }

    #[test]
    fn non_exhaustive_match_errors() {
        let src = format!(
            "{SHAPE} function area(Shape s) -> float {{ \
               return match s {{ Circle(r) => 3.14 * r * r, }}; }}"
        );
        let errs = errors_of(&src);
        assert!(
            errs.iter()
                .any(|e| e.message.contains("non-exhaustive") && e.message.contains("Rect")),
            "{errs:?}"
        );
    }

    #[test]
    fn wildcard_makes_match_exhaustive() {
        let src = format!(
            "{SHAPE} function area(Shape s) -> float {{ \
               return match s {{ Circle(r) => r, _ => 0.0, }}; }}"
        );
        assert!(errors_of(&src).is_empty(), "{:?}", errors_of(&src));
    }

    #[test]
    fn match_arm_type_mismatch_errors() {
        let src = format!(
            "{SHAPE} function f(Shape s) -> float {{ \
               return match s {{ Circle(r) => r, Rect(w, h) => true, }}; }}"
        );
        let errs = errors_of(&src);
        assert!(
            errs.iter().any(|e| e.message.contains("match arms")),
            "{errs:?}"
        );
    }

    #[test]
    fn variant_pattern_arity_checked() {
        let src = format!(
            "{SHAPE} function f(Shape s) -> float {{ \
               return match s {{ Circle(r, x) => r, Rect(w, h) => w, }}; }}"
        );
        let errs = errors_of(&src);
        assert!(
            errs.iter().any(|e| e.message.contains("expects 1 field")),
            "{errs:?}"
        );
    }

    #[test]
    fn unknown_variant_pattern_errors() {
        let src = format!(
            "{SHAPE} function f(Shape s) -> float {{ \
               return match s {{ Circle(r) => r, Triangle(x) => x, Rect(w,h) => w, }}; }}"
        );
        let errs = errors_of(&src);
        assert!(
            errs.iter()
                .any(|e| e.message.contains("no variant `Triangle`")),
            "{errs:?}"
        );
    }

    #[test]
    fn promoted_ctor_param_is_field() {
        // Constructor promotion alone (no explicit `private int total;`) must type-check:
        // the promoted param becomes an instance field, matching the evaluator (EV-4).
        let errs = errors_of(
            "class C { constructor(private int total) {} \
               function add(int n) -> int { return total + n; } }",
        );
        assert!(errs.is_empty(), "promoted field should resolve: {errs:?}");
    }

    #[test]
    fn explicit_field_decl_wins_over_promotion_type() {
        // Explicit field decl is authoritative regardless of member order; a promoted
        // param of the same name does not override its declared type.
        let errs = errors_of(
            "class C { private int total; constructor(private int total) {} \
               function add(int n) -> int { return total + n; } }",
        );
        assert!(
            errs.is_empty(),
            "redundant explicit+promoted (matching type) is fine: {errs:?}"
        );
    }

    #[test]
    fn unmodified_ctor_param_is_not_a_field() {
        // A plain ctor param (no visibility modifier) is NOT promoted, so referencing it
        // bare in a method is still an unknown identifier — matches the evaluator.
        let errs = errors_of(
            "class C { constructor(int total) {} \
               function add(int n) -> int { return total + n; } }",
        );
        assert!(
            errs.iter()
                .any(|e| e.message.contains("unknown identifier")),
            "{errs:?}"
        );
    }
}
