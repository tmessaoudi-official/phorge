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
                        return self.err(*span, format!("Map expects 2 type arguments, got {}", args.len()));
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
            self.err(span, format!("{name} expects 1 type argument, got {}", args.len()));
            Ty::Error
        } else {
            self.resolve_type(&args[0])
        }
    }

    /// Register builtin functions available without explicit user definition.
    fn register_prelude(&mut self) {
        self.funcs.insert(
            "println".into(),
            FnSig { params: vec![Ty::String], ret: Ty::Unit },
        );
    }

    /// Phase 1 — hoist all top-level declarations and the builtin prelude.
    fn collect(&mut self, program: &Program) {
        self.register_prelude();
        // user decl collection added in Task 4 & Task 5.
        let _ = program;
    }

    /// Phase 2 — check every function/method body.
    fn check_program(&mut self, program: &Program) {
        use crate::ast::Item;
        for item in &program.items {
            if let Item::Function(f) = item {
                self.check_function(f);
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
            Stmt::VarDecl { ty, name, init, span } => {
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
            Stmt::If { cond, then_block, else_block, span } => {
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
            Expr::Null(span) => self.err(*span, "null / optional values are not yet supported in M1"),
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
            Expr::Index { object, index, span } => self.check_index(object, index, *span), // Task 5
            Expr::Match { scrutinee, arms, span } => self.check_match(scrutinee, arms, *span), // Task 8
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
            UnaryOp::Neg => self.err(span, format!("unary `-` requires int or float, found `{t}`")),
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
                BinaryOp::Eq | BinaryOp::NotEq | BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le
                | BinaryOp::Ge | BinaryOp::And | BinaryOp::Or | BinaryOp::Is => Ty::Bool,
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
                    self.err(span, format!("cross-type comparison requires explicit conversion (`{l}` vs `{r}`)"));
                }
                Ty::Bool
            }
            BinaryOp::And | BinaryOp::Or => {
                if l != Ty::Bool || r != Ty::Bool {
                    self.err(span, format!("`&&`/`||` require `bool` operands, found `{l}` and `{r}`"));
                }
                Ty::Bool
            }
            BinaryOp::Is => Ty::Bool,
            BinaryOp::Pipe => self.err(span, "the pipe operator `|>` is not yet supported in M1"),
        }
    }

    // ---- stubs replaced in later tasks ----
    fn check_str(&mut self, _parts: &[crate::ast::StrPart]) -> Ty {
        Ty::String // refined in Task 7
    }
    fn check_list(&mut self, _elems: &[crate::ast::Expr], span: Span) -> Ty {
        let _ = span;
        Ty::Error // implemented in Task 5
    }
    fn check_index(&mut self, _o: &crate::ast::Expr, _i: &crate::ast::Expr, span: Span) -> Ty {
        self.err(span, "indexing is not yet supported in M1") // refined in Task 5
    }
    fn check_call(&mut self, _c: &crate::ast::Expr, _a: &[crate::ast::Expr], span: Span) -> Ty {
        self.err(span, "calls not yet supported") // implemented in Task 4
    }
    fn check_member(&mut self, _o: &crate::ast::Expr, _n: &str, span: Span) -> Ty {
        self.err(span, "member access not yet supported") // implemented in Task 6
    }
    fn check_for(&mut self, _stmt: &crate::ast::Stmt) {
        // implemented in Task 5
    }
    fn check_match(&mut self, _s: &crate::ast::Expr, _a: &[crate::ast::MatchArm], span: Span) -> Ty {
        self.err(span, "match not yet supported") // implemented in Task 8
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
        let sp = Span { start: 0, len: 1, line: 1, col: 1 };
        let mut c = Checker::new();
        assert_eq!(c.resolve_type(&Type::Named { name: "int".into(), args: vec![], span: sp }), Ty::Int);
        let list = Type::Named {
            name: "List".into(),
            args: vec![Type::Named { name: "int".into(), args: vec![], span: sp }],
            span: sp,
        };
        assert_eq!(c.resolve_type(&list), Ty::List(Box::new(Ty::Int)));
        assert_eq!(c.errors.len(), 0);
    }

    #[test]
    fn unknown_type_in_var_decl_errors() {
        let errs = errors_of("function main() { Nope n = 0; }");
        assert!(errs.iter().any(|e| e.message.contains("unknown type")), "{errs:?}");
    }

    #[test]
    fn optional_type_is_deferred_corner() {
        let errs = errors_of("function main() { int? n = 0; }");
        assert!(
            errs.iter().any(|e| e.message.contains("optional types are not yet supported")),
            "{errs:?}"
        );
    }

    #[test]
    fn decimal_type_is_deferred_corner() {
        let errs = errors_of("function main() { decimal d = 0; }");
        assert!(
            errs.iter().any(|e| e.message.contains("decimal") && e.message.contains("not yet supported")),
            "{errs:?}"
        );
    }

    #[test]
    fn var_decl_type_mismatch_errors() {
        let errs = errors_of("function main() { int n = true; }");
        assert!(errs.iter().any(|e| e.message.contains("expected `int`")), "{errs:?}");
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
        assert!(errs.iter().any(|e| e.message.contains("condition must be `bool`")), "{errs:?}");
    }

    #[test]
    fn equality_requires_same_type() {
        let errs = errors_of("function main() { bool b = 1 == true; }");
        assert!(errs.iter().any(|e| e.message.contains("cross-type")), "{errs:?}");
    }

    #[test]
    fn unknown_identifier_errors() {
        let errs = errors_of("function main() { int n = missing; }");
        assert!(errs.iter().any(|e| e.message.contains("unknown identifier")), "{errs:?}");
    }

    #[test]
    fn block_scoping_pops_bindings() {
        let errs = errors_of("function main() { { int x = 1; } int y = x; }");
        assert!(errs.iter().any(|e| e.message.contains("unknown identifier")), "{errs:?}");
    }

    #[test]
    fn return_type_checked_against_signature() {
        let errs = errors_of("function f() -> int { return true; }");
        assert!(errs.iter().any(|e| e.message.contains("expected `int`")), "{errs:?}");
    }
}
