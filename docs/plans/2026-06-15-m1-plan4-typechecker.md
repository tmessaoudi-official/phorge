# M1 Plan 4 — Type-Checker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a sound static type-checker that validates the parsed AST (`Program`) and rejects ill-typed programs before the Plan 5 evaluator runs, with the frozen §6 sample type-checking cleanly.

**Architecture:** A single `check(&Program)` pass in two sub-phases — *collect* (hoist all top-level functions/enums/classes + a builtin prelude into a symbol table) then *check* (walk each function/method body, typing statements and expressions against a block-scoped environment). The checker is a validation gate returning `Result<(), Vec<TypeError>>` (collect-all); it does not transform the AST.

**Tech Stack:** Rust (stable 1.96.0, edition 2021), `cargo test` / `cargo clippy --all-targets`, no external crates. Std `HashMap` only.

**Design spec:** `docs/specs/2026-06-15-m1-typechecker-design.md` (sections referenced as §N below).

---

## Conventions for every task

- **Toolchain:** `export PATH=/stack/tools/cargo/bin:$PATH` once per shell. `cargo test` text is reformatted by the rtk wrapper to `cargo test: N passed` — trust the exit code / that summary, not raw `test result:` lines.
- **Run a single test:** `cargo test --lib checker::tests::<name>` (unit) or `cargo test --test typecheck_integration` (integration).
- **TDD loop per task:** write the failing test → run it, confirm it fails for the expected reason → add the minimal impl → run, confirm pass → `cargo clippy --all-targets` clean → commit.
- **Commits:** one per task (the project commits each step; see `b89112f`, `79b3d51`). Message prefix `feat(typeck):` for code, `test(typeck):` if test-only.
- **No new syntax** is added in this plan — only `src/types.rs` (new), `src/checker.rs` (new), `src/lib.rs` (2 lines). The lexer/parser/AST are untouched.

---

## File Structure

| File | Responsibility | Action |
|---|---|---|
| `src/types.rs` | resolved `Ty` enum, `Display`, `assignable()` | Create |
| `src/checker.rs` | `TypeError`, symbol table, scope stack, the pass, `pub fn check` | Create (grows task-by-task) |
| `src/lib.rs` | module declarations | Modify (add 2 lines) |
| `tests/typecheck_integration.rs` | §6 sample + broken-variant integration tests | Create (Task 9) |

---

## Task 1: Scaffold — `Ty`, `assignable`, and an empty-program gate

**Files:**
- Create: `src/types.rs`
- Create: `src/checker.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Add modules to `src/lib.rs`**

```rust
pub mod token;
pub mod lexer;
pub mod ast;
pub mod parser;
pub mod types;
pub mod checker;
```

- [ ] **Step 2: Create `src/types.rs` with the `Ty` enum, `Display`, and `assignable`**

```rust
//! Resolved (internal) type representation, distinct from the AST's `Type`.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    Int,
    Float,
    Bool,
    String,
    Unit,
    /// A nominal enum or class type, by name.
    Named(String),
    List(Box<Ty>),
    Map(Box<Ty>, Box<Ty>),
    Set(Box<Ty>),
    /// Poison type: a failed sub-expression yields this. Assignable both ways so a
    /// single error does not cascade into many.
    Error,
}

impl Ty {
    /// `from` may be used where `to` is expected. `Error` unifies with anything to
    /// suppress cascade errors. No numeric widening (spec §3: no implicit coercion).
    pub fn assignable(from: &Ty, to: &Ty) -> bool {
        *from == Ty::Error || *to == Ty::Error || from == to
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ty::Int => write!(f, "int"),
            Ty::Float => write!(f, "float"),
            Ty::Bool => write!(f, "bool"),
            Ty::String => write!(f, "string"),
            Ty::Unit => write!(f, "unit"),
            Ty::Named(n) => write!(f, "{n}"),
            Ty::List(e) => write!(f, "List<{e}>"),
            Ty::Map(k, v) => write!(f, "Map<{k}, {v}>"),
            Ty::Set(e) => write!(f, "Set<{e}>"),
            Ty::Error => write!(f, "<error>"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assignable_is_equality_plus_error() {
        assert!(Ty::assignable(&Ty::Int, &Ty::Int));
        assert!(!Ty::assignable(&Ty::Int, &Ty::Float)); // no widening
        assert!(Ty::assignable(&Ty::Error, &Ty::Int)); // poison unifies
        assert!(Ty::assignable(&Ty::Int, &Ty::Error));
        assert!(Ty::assignable(
            &Ty::List(Box::new(Ty::Int)),
            &Ty::List(Box::new(Ty::Int))
        ));
        assert!(!Ty::assignable(
            &Ty::List(Box::new(Ty::Int)),
            &Ty::List(Box::new(Ty::Float))
        ));
    }

    #[test]
    fn display_renders_generics() {
        assert_eq!(Ty::List(Box::new(Ty::Named("Shape".into()))).to_string(), "List<Shape>");
    }
}
```

- [ ] **Step 3: Create `src/checker.rs` with `TypeError`, the `Checker` skeleton, and `check`**

```rust
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
}
```

- [ ] **Step 4: Add empty `collect` / `check_program` so it compiles**

Add to the `impl Checker` block in `src/checker.rs`:

```rust
    /// Phase 1 — hoist all top-level declarations and the builtin prelude.
    fn collect(&mut self, _program: &Program) {
        // Prelude + decl collection added in Task 2 & Task 5.
    }

    /// Phase 2 — check every function/method body.
    fn check_program(&mut self, _program: &Program) {
        // Body walking added in Task 3 onward.
    }
```

- [ ] **Step 5: Run tests and clippy**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --lib types:: && cargo test --lib checker:: && cargo clippy --all-targets`
Expected: types tests pass (2), checker `empty_program_checks_ok` passes, clippy clean.
(Expect `dead_code` warnings on unused struct fields/helpers at this stage — silence them by prefixing the still-unused private items with `#[allow(dead_code)]` *only if clippy fails the build*; they are all consumed by Task 6. Prefer leaving them if clippy only warns.)

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/types.rs src/checker.rs
git commit -m "feat(typeck): scaffold Ty, assignable, Checker skeleton, check() gate"
```

---

## Task 2: Type resolution + builtin prelude

Resolve AST `Type` → `Ty`, register prelude (`println`), and emit clean deferred-corner errors for `decimal`/sized ints/`Optional`. (§7, §6 prelude, §2 deferred corners.)

**Files:**
- Modify: `src/checker.rs`

- [ ] **Step 1: Write failing tests** (add into `mod tests` in `src/checker.rs`)

```rust
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
```

> These need `VarDecl` checking, which arrives in Task 3. To test resolution alone now, instead add the direct unit test below and defer the three above to run green after Task 3. Add this one now:

```rust
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
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib checker::tests::resolve_maps_primitives_and_list`
Expected: FAIL — `no method named resolve_type`.

- [ ] **Step 3: Implement `resolve_type` and prelude registration**

Add to `impl Checker`:

```rust
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
```

Wire the prelude into `collect`:

```rust
    fn collect(&mut self, program: &Program) {
        self.register_prelude();
        // user decl collection added in Task 5.
        let _ = program;
    }
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib checker::tests::resolve_maps_primitives_and_list` then `cargo clippy --all-targets`
Expected: PASS; clippy clean.

- [ ] **Step 5: Commit**

```bash
git add src/checker.rs
git commit -m "feat(typeck): type resolution, prelude (println), deferred numeric/optional corners"
```

---

## Task 3: Statements + scopes + expression literals/operators

Walk function bodies: scopes, `VarDecl`, `Return`, `If`, `Block`, `Expr`-stmt, plus expression typing for literals, `Ident`, `Unary`, and `Binary`. (§8, §9 literals/Ident/Unary/Binary.)

**Files:**
- Modify: `src/checker.rs`

- [ ] **Step 1: Write failing tests**

```rust
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
        // `x` declared in an inner block is not visible after it.
        let errs = errors_of("function main() { { int x = 1; } int y = x; }");
        assert!(errs.iter().any(|e| e.message.contains("unknown identifier")), "{errs:?}");
    }

    #[test]
    fn return_type_checked_against_signature() {
        let errs = errors_of("function f() -> int { return true; }");
        assert!(errs.iter().any(|e| e.message.contains("expected `int`")), "{errs:?}");
    }
```

(The three deferred tests from Task 2 — `unknown_type_in_var_decl_errors`, `optional_type_is_deferred_corner`, `decimal_type_is_deferred_corner` — also become runnable now; keep them.)

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --lib checker::tests`
Expected: the new tests FAIL (bodies not yet walked).

- [ ] **Step 3: Implement scope helpers, statement walking, and expression typing**

Add to `impl Checker`:

```rust
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
            // still validate logical/relational shape lazily; just poison
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
            BinaryOp::Pipe => {
                self.err(span, "the pipe operator `|>` is not yet supported in M1")
            }
        }
    }
```

Add temporary stubs so it compiles (real versions land in later tasks). Put these in `impl Checker`:

```rust
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
```

Wire body-walking into `check_program` for free functions only (methods in Task 6):

```rust
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
```

- [ ] **Step 4: Run all checker tests + clippy**

Run: `cargo test --lib checker:: && cargo clippy --all-targets`
Expected: all Task 2 + Task 3 tests pass; clippy clean.

- [ ] **Step 5: Commit**

```bash
git add src/checker.rs
git commit -m "feat(typeck): statements, block scopes, literal/unary/binary expression typing"
```

---

## Task 4: Collect free functions + call checking + overloading guard

Hoist user `function` declarations into the symbol table and type `Call` to a free function by arity + exact type. Duplicate names → the overloading deferred-corner. (§6 functions, §9 Call, §2 deferred corners.)

**Files:**
- Modify: `src/checker.rs`

- [ ] **Step 1: Write failing tests**

```rust
    #[test]
    fn function_call_arity_and_type_checked() {
        assert!(errors_of("function inc(int n) -> int { return n + 1; } function main() { int x = inc(1); }").is_empty());
        let bad_arity = errors_of("function inc(int n) -> int { return n; } function main() { int x = inc(1, 2); }");
        assert!(bad_arity.iter().any(|e| e.message.contains("expects 1 argument")), "{bad_arity:?}");
        let bad_type = errors_of("function inc(int n) -> int { return n; } function main() { int x = inc(true); }");
        assert!(bad_type.iter().any(|e| e.message.contains("argument 1")), "{bad_type:?}");
    }

    #[test]
    fn unknown_function_call_errors() {
        let errs = errors_of("function main() { nope(); }");
        assert!(errs.iter().any(|e| e.message.contains("unknown function")), "{errs:?}");
    }

    #[test]
    fn duplicate_function_is_overloading_corner() {
        let errs = errors_of("function f() {} function f(int n) {}");
        assert!(errs.iter().any(|e| e.message.contains("overloading is not yet supported")), "{errs:?}");
    }

    #[test]
    fn println_accepts_string() {
        assert!(errors_of(r#"function main() { println("hi"); }"#).is_empty());
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib checker::tests::function_call_arity_and_type_checked`
Expected: FAIL (calls stubbed).

- [ ] **Step 3: Implement function collection and real `check_call`**

In `collect`, after `register_prelude()`, add function hoisting:

```rust
    fn collect(&mut self, program: &Program) {
        use crate::ast::Item;
        self.register_prelude();
        for item in &program.items {
            match item {
                Item::Function(f) => self.collect_function(f),
                Item::Enum(_) => {} // Task 5
                Item::Class(_) => {} // Task 6
                Item::Import { .. } => {} // module resolution deferred; prelude covers println
            }
        }
    }

    fn collect_function(&mut self, f: &crate::ast::FunctionDecl) {
        if self.funcs.contains_key(&f.name) {
            self.err(f.span, format!("function overloading is not yet supported in M1 (`{}` already defined)", f.name));
            return;
        }
        let params = f.params.iter().map(|p| self.resolve_type(&p.ty)).collect();
        let ret = match &f.ret {
            Some(t) => self.resolve_type(t),
            None => Ty::Unit,
        };
        self.funcs.insert(f.name.clone(), FnSig { params, ret });
    }
```

Replace the `check_call` stub with the real implementation (free-function form; member-call form added in Task 6):

```rust
    fn check_call(&mut self, callee: &crate::ast::Expr, args: &[crate::ast::Expr], span: Span) -> Ty {
        use crate::ast::Expr;
        match callee {
            Expr::Ident(name, _) => self.check_named_call(name, args, span),
            Expr::Member { object, name, .. } => self.check_method_call(object, name, args, span), // Task 6
            other => {
                // type the args anyway to surface their errors
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
        // enum variant / class constructor dispatch is layered in by Task 5 / Task 6.
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
            self.err(span, format!("`{name}` expects {} argument(s), found {}", params.len(), args.len()));
            for a in args {
                self.check_expr(a);
            }
            return;
        }
        for (i, (param, arg)) in params.iter().zip(args).enumerate() {
            let at = self.check_expr(arg);
            if !Ty::assignable(&at, param) {
                self.err(span, format!("`{name}` argument {} expects `{param}`, found `{at}`", i + 1));
            }
        }
    }
```

Add a stub for the variant/class dispatch so it compiles (filled in Task 5/6):

```rust
    /// Returns `Some(ret)` if `name` is an enum variant or class constructor.
    fn try_variant_or_class_call(
        &mut self,
        _name: &str,
        _args: &[crate::ast::Expr],
        _span: Span,
    ) -> Option<Ty> {
        None // enum variants: Task 5; class constructors: Task 6
    }

    fn check_method_call(
        &mut self,
        _object: &crate::ast::Expr,
        _name: &str,
        _args: &[crate::ast::Expr],
        span: Span,
    ) -> Ty {
        self.err(span, "method calls not yet supported") // Task 6
    }
```

- [ ] **Step 4: Run + clippy**

Run: `cargo test --lib checker:: && cargo clippy --all-targets`
Expected: all pass; clippy clean.

- [ ] **Step 5: Commit**

```bash
git add src/checker.rs
git commit -m "feat(typeck): collect functions, call arity/type checking, overloading guard"
```

---

## Task 5: Enums, variant constructors, list literals, for-in, indexing

Hoist enums, type variant constructor calls, type list literals with element unification, `for`-`in` over `List`, and `List` indexing. (§6 enums/list/for, §9 List/Index, §10 variant types.)

**Files:**
- Modify: `src/checker.rs`

- [ ] **Step 1: Write failing tests**

```rust
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
        assert!(errs.iter().any(|e| e.message.contains("argument 1")), "{errs:?}");
    }

    #[test]
    fn list_literal_unifies_elements() {
        let src = format!("{SHAPE} function main() {{ List<Shape> xs = [Circle(1.0), Rect(2.0, 3.0)]; }}");
        assert!(errors_of(&src).is_empty());
    }

    #[test]
    fn list_literal_mixed_elements_error() {
        let errs = errors_of("function main() { List<int> xs = [1, true]; }");
        assert!(errs.iter().any(|e| e.message.contains("list elements")), "{errs:?}");
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
        assert!(errs.iter().any(|e| e.message.contains("`for`-`in` requires a List")), "{errs:?}");
    }

    #[test]
    fn list_indexing_yields_element() {
        assert!(errors_of("function main() { List<int> xs = [1, 2]; int y = xs[0]; }").is_empty());
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib checker::tests::variant_constructor_returns_enum`
Expected: FAIL.

- [ ] **Step 3: Implement enum collection, variant call, list, for-in, indexing**

In `collect`, handle `Item::Enum`:

```rust
                Item::Enum(e) => self.collect_enum(e),
```

Add to `impl Checker`:

```rust
    fn collect_enum(&mut self, e: &crate::ast::EnumDecl) {
        if self.enums.contains_key(&e.name) || self.classes.contains_key(&e.name) {
            self.err(e.span, format!("type `{}` is already defined", e.name));
            return;
        }
        // Register the name first so variant field types can reference the enum itself.
        self.enums.insert(e.name.clone(), EnumInfo { variants: HashMap::new() });
        let mut variants = HashMap::new();
        for v in &e.variants {
            let fields = v.fields.iter().map(|p| self.resolve_type(&p.ty)).collect();
            variants.insert(v.name.clone(), fields);
        }
        self.enums.get_mut(&e.name).unwrap().variants = variants;
    }
```

Replace the `try_variant_or_class_call` stub's enum half:

```rust
    fn try_variant_or_class_call(
        &mut self,
        name: &str,
        args: &[crate::ast::Expr],
        span: Span,
    ) -> Option<Ty> {
        // class constructors are layered in by Task 6
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
        None
    }
```

Replace the `check_list` stub:

```rust
    fn check_list(&mut self, elems: &[crate::ast::Expr], span: Span) -> Ty {
        if elems.is_empty() {
            // empty list element type comes from the VarDecl expected type; without it
            // we cannot infer. (VarDecl passes expectation via assignable against the
            // declared List<E>; an unconstrained [] is an error.)
            return self.err(span, "cannot infer element type of empty list literal");
        }
        let first = self.check_expr(&elems[0]);
        for e in &elems[1..] {
            let t = self.check_expr(e);
            if !Ty::assignable(&t, &first) && !Ty::assignable(&first, &t) {
                self.err(span, format!("list elements must share one type; found `{first}` and `{t}`"));
            }
        }
        Ty::List(Box::new(first))
    }
```

> **Empty-list note:** with the gate design, `List<int> xs = [];` would report "cannot infer element type of empty list literal". That is acceptable for M1 (the §6 sample has no empty list). A future task can thread the `VarDecl` declared type as an expected type into `check_list`; not needed now (YAGNI).

Replace the `check_index` stub:

```rust
    fn check_index(&mut self, object: &crate::ast::Expr, index: &crate::ast::Expr, span: Span) -> Ty {
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
```

Replace the `check_for` stub:

```rust
    fn check_for(&mut self, stmt: &crate::ast::Stmt) {
        if let crate::ast::Stmt::For { ty, name, iter, body, span } = stmt {
            let declared = self.resolve_type(ty);
            let iter_ty = self.check_expr(iter);
            let elem = match iter_ty {
                Ty::List(e) => *e,
                Ty::Error => Ty::Error,
                other => {
                    self.err(*span, format!("`for`-`in` requires a List, found `{other}`"));
                    Ty::Error
                }
            };
            if !Ty::assignable(&elem, &declared) {
                self.err(*span, format!("loop variable `{name}` declared `{declared}` but iterating `{elem}`"));
            }
            self.push_scope();
            self.declare(name, declared);
            for s in body {
                self.check_stmt(s);
            }
            self.pop_scope();
        }
    }
```

- [ ] **Step 4: Run + clippy**

Run: `cargo test --lib checker:: && cargo clippy --all-targets`
Expected: all pass; clippy clean.

- [ ] **Step 5: Commit**

```bash
git add src/checker.rs
git commit -m "feat(typeck): enums, variant constructors, list literals, for-in, indexing"
```

---

## Task 6: Classes — fields, methods, constructors, member access, `this`

Hoist classes (fields + methods + constructor param types), type constructor calls `ClassName(args)`, member field access, method calls, and check method bodies with `this` and bare field refs in scope. (§6 class, §9 Member/Call, §11 method scope, §2 no constructor promotion.)

**Files:**
- Modify: `src/checker.rs`

- [ ] **Step 1: Write failing tests**

```rust
    const GREETER: &str = "class Greeter { private string name; constructor(string name) {} function greet() -> string { return \"Hi\"; } }";

    #[test]
    fn constructor_call_and_method_call_ok() {
        let src = format!("{GREETER} function main() {{ Greeter g = Greeter(\"Tak\"); string s = g.greet(); }}");
        assert!(errors_of(&src).is_empty(), "{:?}", errors_of(&src));
    }

    #[test]
    fn constructor_arg_type_checked() {
        let src = format!("{GREETER} function main() {{ Greeter g = Greeter(123); }}");
        let errs = errors_of(&src);
        assert!(errs.iter().any(|e| e.message.contains("argument 1")), "{errs:?}");
    }

    #[test]
    fn unknown_method_errors() {
        let src = format!("{GREETER} function main() {{ Greeter g = Greeter(\"x\"); g.missing(); }}");
        let errs = errors_of(&src);
        assert!(errs.iter().any(|e| e.message.contains("no method `missing`")), "{errs:?}");
    }

    #[test]
    fn field_access_typed() {
        let src = "class Box { public int n; constructor(int n) {} } function main() { Box b = Box(1); int x = b.n; }";
        assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
    }

    #[test]
    fn bare_field_visible_in_method() {
        // `name` referenced without `this.` inside a method resolves to the field.
        let src = "class C { private string name; constructor(string name) {} function who() -> string { return name; } }";
        assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
    }

    #[test]
    fn this_outside_method_errors() {
        let errs = errors_of("function main() { string s = this; }");
        assert!(errs.iter().any(|e| e.message.contains("`this`")), "{errs:?}");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib checker::tests::constructor_call_and_method_call_ok`
Expected: FAIL.

- [ ] **Step 3: Implement class collection, member access, method/constructor calls, method body checking**

In `collect`, handle `Item::Class`:

```rust
                Item::Class(c) => self.collect_class(c),
```

Add to `impl Checker`:

```rust
    fn collect_class(&mut self, c: &crate::ast::ClassDecl) {
        use crate::ast::ClassMember;
        if self.classes.contains_key(&c.name) || self.enums.contains_key(&c.name) {
            self.err(c.span, format!("type `{}` is already defined", c.name));
            return;
        }
        // Register the name first so members can reference the class type itself.
        self.classes.insert(
            c.name.clone(),
            ClassInfo { fields: HashMap::new(), methods: HashMap::new(), ctor: Vec::new() },
        );
        let mut fields = HashMap::new();
        let mut methods = HashMap::new();
        let mut ctor = Vec::new();
        for m in &c.members {
            match m {
                // Constructor promotion is NOT modeled in M1 (§2): ctor params do not
                // create fields. Fields come only from explicit field declarations.
                ClassMember::Field { ty, name, .. } => {
                    let fty = self.resolve_type(ty);
                    fields.insert(name.clone(), fty);
                }
                ClassMember::Constructor { params, .. } => {
                    ctor = params.iter().map(|p| self.resolve_type(&p.ty)).collect();
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
        let info = self.classes.get_mut(&c.name).unwrap();
        info.fields = fields;
        info.methods = methods;
        info.ctor = ctor;
    }
```

Extend `check_program` to also check method bodies:

```rust
    fn check_program(&mut self, program: &Program) {
        use crate::ast::{Item, ClassMember};
        for item in &program.items {
            match item {
                Item::Function(f) => self.check_function(f),
                Item::Class(c) => {
                    let prev = self.cur_class.replace(c.name.clone());
                    for m in &c.members {
                        match m {
                            ClassMember::Method(f) => self.check_function(f),
                            ClassMember::Constructor { body, .. } => {
                                let prev_ret = std::mem::replace(&mut self.cur_ret, Ty::Unit);
                                self.push_scope();
                                // constructor params are in scope inside its body
                                if let Some(info) = self.classes.get(&c.name) {
                                    let ctor = info.ctor.clone();
                                    if let ClassMember::Constructor { params, .. } = m {
                                        for (p, t) in params.iter().zip(ctor) {
                                            self.declare(&p.name, t);
                                        }
                                    }
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
```

> `check_function` already seeds params into a fresh scope; when `cur_class` is set, `lookup` falls back to class fields, so bare field references resolve inside methods (§11).

Replace the `check_member` stub:

```rust
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
```

Replace the `check_method_call` stub:

```rust
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
```

Extend `try_variant_or_class_call` to also dispatch class constructors (add before the enum lookup return, after the existing enum block — full replacement):

```rust
    fn try_variant_or_class_call(
        &mut self,
        name: &str,
        args: &[crate::ast::Expr],
        span: Span,
    ) -> Option<Ty> {
        // enum variant constructor
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
```

- [ ] **Step 4: Run + clippy**

Run: `cargo test --lib checker:: && cargo clippy --all-targets`
Expected: all pass; clippy clean. (The `field_access_typed` test needs `b.n` where field `n` is `public int` — confirm parser accepts `public int n;` as a field; it does per Plan 3 `ClassMember::Field{modifiers,...}`.)

- [ ] **Step 5: Commit**

```bash
git add src/checker.rs
git commit -m "feat(typeck): classes, fields, methods, constructors, member access, this"
```

---

## Task 7: String interpolation typing

Typed string parts: embedded expressions must be primitives (auto-stringify); objects/enums error. (§5 interpolation, §9 Str.)

**Files:**
- Modify: `src/checker.rs`

- [ ] **Step 1: Write failing tests**

```rust
    #[test]
    fn interpolation_allows_primitives() {
        // float interpolation is required by the §6 sample
        assert!(errors_of("function main() { float x = 1.5; string s = \"v = {x}\"; }").is_empty());
        assert!(errors_of("function main() { int n = 3; string s = \"n = {n}\"; }").is_empty());
    }

    #[test]
    fn interpolation_rejects_objects() {
        let src = "class C { private int n; constructor(int n) {} } function main() { C c = C(1); string s = \"{c}\"; }";
        let errs = errors_of(src);
        assert!(errs.iter().any(|e| e.message.contains("cannot be interpolated")), "{errs:?}");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib checker::tests::interpolation_rejects_objects`
Expected: FAIL (stub returns `String` unconditionally).

- [ ] **Step 3: Implement real `check_str`**

The AST has no span on `StrPart::Expr`'s outer position beyond the inner `Expr`, so report errors at the embedded expression's span via an `expr_span` helper.

Replace the `check_str` stub and add `expr_span`:

```rust
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
```

- [ ] **Step 4: Run + clippy**

Run: `cargo test --lib checker:: && cargo clippy --all-targets`
Expected: all pass; clippy clean.

- [ ] **Step 5: Commit**

```bash
git add src/checker.rs
git commit -m "feat(typeck): string interpolation typing (primitives auto-stringify)"
```

---

## Task 8: Match — exhaustiveness + pattern checking (crown jewel)

Type `match` expressions: scrutinee + per-arm pattern checks with bindings, arm-body unification, and exhaustiveness over enum variants. (§10.)

**Files:**
- Modify: `src/checker.rs`

- [ ] **Step 1: Write failing tests**

```rust
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
        assert!(errs.iter().any(|e| e.message.contains("non-exhaustive") && e.message.contains("Rect")), "{errs:?}");
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
        assert!(errs.iter().any(|e| e.message.contains("match arms")), "{errs:?}");
    }

    #[test]
    fn variant_pattern_arity_checked() {
        let src = format!(
            "{SHAPE} function f(Shape s) -> float {{ \
               return match s {{ Circle(r, x) => r, Rect(w, h) => w, }}; }}"
        );
        let errs = errors_of(&src);
        assert!(errs.iter().any(|e| e.message.contains("expects 1 field")), "{errs:?}");
    }

    #[test]
    fn unknown_variant_pattern_errors() {
        let src = format!(
            "{SHAPE} function f(Shape s) -> float {{ \
               return match s {{ Circle(r) => r, Triangle(x) => x, Rect(w,h) => w, }}; }}"
        );
        let errs = errors_of(&src);
        assert!(errs.iter().any(|e| e.message.contains("no variant `Triangle`")), "{errs:?}");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib checker::tests::match_over_enum_is_typed_and_exhaustive`
Expected: FAIL (match stubbed).

- [ ] **Step 3: Implement `check_match` and `check_pattern`**

Replace the `check_match` stub:

```rust
    fn check_match(&mut self, scrutinee: &crate::ast::Expr, arms: &[crate::ast::MatchArm], span: Span) -> Ty {
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
                        self.err(span, format!("match arms must share one type; found `{first}` and `{body_ty}`"));
                    }
                }
            }
        }

        // exhaustiveness
        if !has_catch_all {
            match &scrut {
                Ty::Named(enum_name) if self.enums.contains_key(enum_name) => {
                    let all: Vec<String> = self.enums[enum_name].variants.keys().cloned().collect();
                    let missing: Vec<String> = all.into_iter().filter(|v| !covered.contains(v)).collect();
                    if !missing.is_empty() {
                        self.err(span, format!("non-exhaustive match: missing {}", missing.join(", ")));
                    }
                }
                Ty::Error => {}
                _ => {
                    self.err(span, "non-exhaustive match: add a `_` wildcard arm for non-enum scrutinees");
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
                self.err(*span, "null patterns / optionals are not yet supported in M1");
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
                    self.err(*span, format!("variant `{name}` expects {} field(s), found {}", field_tys.len(), fields.len()));
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
            self.err(span, format!("pattern of type `{want}` cannot match scrutinee of type `{scrut}`"));
        }
    }
```

- [ ] **Step 4: Run + clippy**

Run: `cargo test --lib checker:: && cargo clippy --all-targets`
Expected: all pass; clippy clean.

- [ ] **Step 5: Commit**

```bash
git add src/checker.rs
git commit -m "feat(typeck): match exhaustiveness + pattern checking with bindings"
```

---

## Task 9: Integration — §6 sample + broken variants, final verification

End-to-end: the verbatim §6 sample type-checks clean; broken variants produce precise errors. Then run the full Completion Gate.

**Files:**
- Create: `tests/typecheck_integration.rs`

- [ ] **Step 1: Write the integration tests**

```rust
use phorge::checker::check;
use phorge::lexer::lex;
use phorge::parser::Parser;

/// The complete sample program from the design spec (§6), verbatim.
const SAMPLE: &str = r#"
import std.io;

enum Shape {
    Circle(float radius),
    Rect(float w, float h),
}

function area(Shape s) -> float {
    return match s {
        Circle(r)  => 3.14159 * r * r,
        Rect(w, h) => w * h,
    };
}

class Greeter {
    private string name;

    constructor(private string name) {}

    function greet() -> string {
        return "Hello {name}";
    }
}

function main() {
    Greeter g = Greeter("Tak");
    println(g.greet());

    List<Shape> shapes = [Circle(2.0), Rect(3.0, 4.0)];
    for (Shape s in shapes) {
        println("area = {area(s)}");
    }
}
"#;

fn check_src(src: &str) -> Result<(), Vec<phorge::checker::TypeError>> {
    let tokens = lex(src).expect("lex ok");
    let prog = Parser::new(tokens).parse_program().expect("parse ok");
    check(&prog)
}

#[test]
fn sample_program_type_checks_clean() {
    let result = check_src(SAMPLE);
    assert!(result.is_ok(), "expected clean type-check, got: {result:?}");
}

#[test]
fn non_exhaustive_match_in_full_program_errors() {
    let broken = SAMPLE.replace("        Rect(w, h) => w * h,\n", "");
    let errs = check_src(&broken).expect_err("should be non-exhaustive");
    assert!(errs.iter().any(|e| e.message.contains("non-exhaustive")), "{errs:?}");
}

#[test]
fn wrong_constructor_arg_in_full_program_errors() {
    let broken = SAMPLE.replace(r#"Greeter("Tak")"#, "Greeter(123)");
    let errs = check_src(&broken).expect_err("should be a type error");
    assert!(errs.iter().any(|e| e.message.contains("argument 1")), "{errs:?}");
}

#[test]
fn loop_variable_type_mismatch_errors() {
    let broken = SAMPLE.replace("for (Shape s in shapes)", "for (int s in shapes)");
    let errs = check_src(&broken).expect_err("should be a type error");
    assert!(!errs.is_empty());
}
```

- [ ] **Step 2: Run the integration tests**

Run: `cargo test --test typecheck_integration`
Expected: all 4 pass. If `sample_program_type_checks_clean` fails, read the returned errors — they pinpoint the gap.

- [ ] **Step 3: Full suite + clippy + warnings check**

Run: `cargo test && cargo clippy --all-targets 2>&1 | tail -5 && cargo build 2>&1 | grep -c warning`
Expected: all suites green (Task-1..8 unit tests + 4 integration + the existing 51); clippy clean; `0` build warnings.

- [ ] **Step 4: Throwaway panic-probe (don't commit)**

Create `tests/_probe.rs` with ~15 malformed-but-parseable programs (e.g. `function f() -> int { return; }`, deeply nested matches, `function main() { int x = y + z; }`), each asserting `check(...)` returns `Err` (or `Ok`) **without panicking**. Run `cargo test --test _probe`, confirm no panic, then delete it with plain `rm tests/_probe.rs` (NOT `find -delete` — the rtk `find` rejects compound predicates).

```rust
// tests/_probe.rs — throwaway; delete after running, do not commit.
use phorge::{checker::check, lexer::lex, parser::Parser};
fn run(src: &str) {
    if let Ok(toks) = lex(src) {
        if let Ok(p) = Parser::new(toks).parse_program() {
            let _ = check(&p); // must not panic
        }
    }
}
#[test]
fn probes_do_not_panic() {
    for s in [
        "function f() -> int { return; }",
        "function main() { int x = y; }",
        "function main() { for (int i in 5) {} }",
        "enum E { A, } function f(E e) -> int { return match e { } ; }",
        "class C {} function main() { C c = C(); int n = c.x; }",
        "function main() { string s = \"{undefined}\"; }",
        "function main() { int x = 1 + true; }",
        "function f() {} function f() {}",
        "function main() { int? n = 0; }",
        "function main() { decimal d = 0; }",
        "function main() { List<int> xs = []; }",
        "function main() { int x = [1,2]; }",
        "function main() { match 1 { 1 => 2, } ; }",
        "function main() { this; }",
        "function main() { 1 |> 2; }",
    ] {
        run(s);
    }
}
```

- [ ] **Step 5: Commit (code only — probe already deleted)**

```bash
git add tests/typecheck_integration.rs
git commit -m "test(typeck): §6 sample type-checks clean + broken-variant integration tests"
```

---

## Final Verification — Completion Gate

Produce the four-dimension evidence table before declaring complete:

| Dimension | Evidence to capture |
|---|---|
| **Coverage** | `cargo test` summary: existing 51 + new checker unit tests (~30) + 4 integration = all green. Paste the `cargo test: N passed` line. |
| **Docs** | Public surface added: `phorge::types::Ty`, `phorge::checker::{check, TypeError}`. Confirm design spec §1–§14 matches the implementation; update the handoff `Next` to Plan 5. |
| **Config** | No config impact (no new env vars / flags / CLAUDE.md routing). State this explicitly. |
| **Blast radius** | `grep -rn "pub fn check\|pub mod checker\|pub mod types" src/` — confirm only `lib.rs` wires the modules and nothing else depended on these names before. Parser/lexer/AST untouched: `git diff --stat` shows only `src/types.rs`, `src/checker.rs`, `src/lib.rs`, `tests/typecheck_integration.rs`. |

**Acceptance criteria (all must hold):**
- §6 sample ⇒ `check()` returns `Ok`.
- Each deferred corner (optional, null, decimal/sized int, pipe, overloading, Map index) ⇒ a specific "not yet supported in M1" error.
- Non-exhaustive match, cross-type compare, arity/type mismatch, bool-condition, unknown ident/type ⇒ precise errors.
- `cargo test` all green; `cargo clippy --all-targets` clean; `0` build warnings.
- Panic-probe: malformed-but-parseable programs ⇒ no panic.

---

## Plan Self-Review (completed)

- **Spec coverage:** §3 architecture → Task 1; §5 Ty/assignable → Task 1; §6 prelude/resolution → Task 2; §7 resolution corners → Task 2; §8 statements → Task 3; §9 expressions → Tasks 3–7; §10 match/patterns → Task 8; §11 method scope → Task 6; §12 error API → Task 1; §13 testing → every task + Task 9; §2 deferred corners → Tasks 2/3/4/5 + probe. No gaps.
- **Placeholder scan:** no TBD/TODO; every code step shows complete code; stubs are explicit, named, and each is replaced in a named later task (call→T4, list/index/for→T5, member/method→T6, str→T7, match→T8).
- **Type consistency:** `Ty`, `FnSig{params,ret}`, `EnumInfo{variants}`, `ClassInfo{fields,methods,ctor}`, `TypeError{message,line,col}`, `check_expr`/`check_stmt`/`check_args`/`resolve_type`/`try_variant_or_class_call`/`check_method_call`/`expr_span` names are used identically across all tasks. Constructor-promotion-not-modeled is consistent between §2, Task 6 `collect_class`, and the §6 `Greeter` (explicit `name` field).
