# M1 Plan 5 — Tree-Walking Evaluator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (inline)
> or superpowers:subagent-driven-development to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax.

**Goal:** Programs run. Walk the untyped AST against runtime values and execute
`main`; the §6 sample prints `Hello Tak` / `area = 12.56636` / `area = 12`.

**Architecture:** Two-phase like the checker — `collect` hoists funcs/enums/classes
into global tables, then `interpret` locates `main` and calls it. One recursive
walker. Owned `Value` + `Clone` (no `Rc`). `return`/runtime-errors carried by a
`Signal` enum in `Result::Err`. Spec:
`docs/specs/2026-06-15-m1-plan5-evaluator-design.md`.

**Tech Stack:** Rust (stable 1.96.0, edition 2021), std only. Run tests with
`export PATH=/stack/tools/cargo/bin:$PATH && cargo test`.

> **Toolchain notes (from prior plans):** the rtk tee wrapper prints
> `cargo test: N passed` and swallows the raw summary on success — trust the count
> and exit code. `grep -c` with 0 matches exits 1 (guard with `|| echo 0`). Use
> plain `rm` (rtk `find` rejects compound predicates).

---

## File Structure

- `src/value.rs` — `Value`, `Instance`, `EnumVal`, `HKey`; `as_display`,
  `type_name`, `eq_val`. (Task 1)
- `src/interpreter.rs` — `Interp`, `Frame`, `Signal`, `RuntimeError`, the walker,
  `pub fn interpret`. (Task 2)
- `src/lib.rs` — add `pub mod value;` (Task 1) and `pub mod interpreter;` (Task 2).
- `tests/run_integration.rs` — §6 sample + error cases. (Task 3)

---

## Task 1: Runtime values (`src/value.rs`)

**Files:**
- Create: `src/value.rs`
- Modify: `src/lib.rs` (add `pub mod value;` after `pub mod types;`)

- [ ] **Step 1: Write `src/value.rs` with its test module**

```rust
//! Runtime values for the M1 tree-walking evaluator. Owned + `Clone` (no `Rc`):
//! M1 has no reassignment or post-construction mutation (Plan 3), so shared
//! mutability is unneeded. See design spec EV-1.

use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Unit,
    List(Vec<Value>),
    /// Constructible in principle; the M1 sample never builds or indexes one.
    Map(HashMap<HKey, Value>),
    Set(HashSet<HKey>),
    Instance(Box<Instance>),
    Enum(Box<EnumVal>),
}

#[derive(Debug, Clone)]
pub struct Instance {
    pub class: String,
    pub fields: HashMap<String, Value>,
}

#[derive(Debug, Clone)]
pub struct EnumVal {
    pub ty: String,
    pub variant: String,
    pub payload: Vec<Value>,
}

/// Hashable key subset for `Map`/`Set` (`Value` can't derive `Hash`/`Eq`: it
/// holds `f64`). Unused by the M1 sample but required by the value-type signatures.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HKey {
    Int(i64),
    Bool(bool),
    Str(String),
}

impl Value {
    /// Short name for diagnostics. Composite types fold to a constant so the
    /// return can stay `&'static str`.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Bool(_) => "bool",
            Value::Str(_) => "string",
            Value::Unit => "unit",
            Value::List(_) => "list",
            Value::Map(_) => "map",
            Value::Set(_) => "set",
            Value::Instance(_) => "instance",
            Value::Enum(_) => "enum",
        }
    }

    /// Render a *primitive* value for interpolation / `println`. `None` for a
    /// composite value (the caller turns that into a `RuntimeError`). Floats use
    /// Rust `{}` formatting (EV-6): `12.0` -> `"12"`.
    pub fn as_display(&self) -> Option<String> {
        match self {
            Value::Int(n) => Some(n.to_string()),
            Value::Float(x) => Some(format!("{x}")),
            Value::Bool(b) => Some(b.to_string()),
            Value::Str(s) => Some(s.clone()),
            Value::Unit => Some("unit".to_string()),
            _ => None,
        }
    }

    /// Structural value equality for `==` / `!=` / `is`.
    #[allow(clippy::float_cmp)] // intentional: language-level float equality
    pub fn eq_val(&self, other: &Value) -> bool {
        use Value::*;
        match (self, other) {
            (Int(a), Int(b)) => a == b,
            (Float(a), Float(b)) => a == b,
            (Bool(a), Bool(b)) => a == b,
            (Str(a), Str(b)) => a == b,
            (Unit, Unit) => true,
            (List(a), List(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.eq_val(y))
            }
            (Enum(a), Enum(b)) => {
                a.ty == b.ty
                    && a.variant == b.variant
                    && a.payload.len() == b.payload.len()
                    && a.payload.iter().zip(&b.payload).all(|(x, y)| x.eq_val(y))
            }
            (Instance(a), Instance(b)) => {
                a.class == b.class
                    && a.fields.len() == b.fields.len()
                    && a.fields.iter().all(|(k, v)| {
                        b.fields.get(k).is_some_and(|bv| v.eq_val(bv))
                    })
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_display_renders_primitives() {
        assert_eq!(Value::Int(42).as_display().as_deref(), Some("42"));
        assert_eq!(Value::Float(12.0).as_display().as_deref(), Some("12"));
        assert_eq!(Value::Float(12.56636).as_display().as_deref(), Some("12.56636"));
        assert_eq!(Value::Bool(true).as_display().as_deref(), Some("true"));
        assert_eq!(Value::Str("hi".into()).as_display().as_deref(), Some("hi"));
    }

    #[test]
    fn as_display_is_none_for_composite() {
        let inst = Value::Instance(Box::new(Instance {
            class: "Greeter".into(),
            fields: HashMap::new(),
        }));
        assert!(inst.as_display().is_none());
    }

    #[test]
    fn eq_val_matches_by_value() {
        assert!(Value::Int(1).eq_val(&Value::Int(1)));
        assert!(!Value::Int(1).eq_val(&Value::Int(2)));
        assert!(!Value::Int(1).eq_val(&Value::Float(1.0))); // no cross-type eq
        let a = Value::Enum(Box::new(EnumVal {
            ty: "Shape".into(),
            variant: "Circle".into(),
            payload: vec![Value::Float(2.0)],
        }));
        let b = a.clone();
        assert!(a.eq_val(&b));
    }

    #[test]
    fn type_name_is_stable() {
        assert_eq!(Value::Unit.type_name(), "unit");
        assert_eq!(Value::List(vec![]).type_name(), "list");
    }
}
```

- [ ] **Step 2: Wire the module in `src/lib.rs`**

Add the line `pub mod value;` immediately after `pub mod types;`:

```rust
pub mod token;
pub mod lexer;
pub mod ast;
pub mod parser;
pub mod types;
pub mod value;
pub mod checker;
```

- [ ] **Step 3: Run the tests**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test value::`
Expected: the 4 `value` unit tests pass; total suite still green.

- [ ] **Step 4: Clippy**

Run: `cargo clippy --all-targets 2>&1 | tail -5`
Expected: exit 0, no warnings.

- [ ] **Step 5: Commit**

```bash
git add src/value.rs src/lib.rs
git commit -m "feat(eval): runtime Value type (M1 Plan 5 Task 1)"
```

---

## Task 2: The interpreter (`src/interpreter.rs`)

This is the walker. It is cohesive (eval ↔ call ↔ construct ↔ match are mutually
recursive), so it lands as one file with a comprehensive test module driven
through a `run()` helper (lex → parse → interpret → assert captured stdout),
mirroring the checker's integration-style unit tests.

**Files:**
- Create: `src/interpreter.rs`
- Modify: `src/lib.rs` (add `pub mod interpreter;` after `pub mod checker;`)

- [ ] **Step 1: Write the test module first (it won't compile — that's the red)**

Put this at the **bottom** of the new `src/interpreter.rs`. Write it first,
mentally, so the implementation that follows satisfies it:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    use crate::parser::Parser;

    /// Lex + parse + interpret; return captured stdout or the runtime error.
    fn run(src: &str) -> Result<String, RuntimeError> {
        let tokens = lex(src).expect("lex ok");
        let prog = Parser::new(tokens).parse_program().expect("parse ok");
        interpret(&prog)
    }

    fn out(src: &str) -> String {
        run(src).expect("run ok")
    }

    #[test]
    fn prints_a_literal_string() {
        assert_eq!(out(r#"function main() { println("hi"); }"#), "hi\n");
    }

    #[test]
    fn integer_arithmetic_in_interpolation() {
        assert_eq!(out(r#"function main() { println("{1 + 2 * 3}"); }"#), "7\n");
    }

    #[test]
    fn float_arithmetic() {
        assert_eq!(out(r#"function main() { println("{3.0 * 4.0}"); }"#), "12\n");
    }

    #[test]
    fn division_by_zero_is_runtime_error() {
        let e = run(r#"function main() { println("{1 / 0}"); }"#).unwrap_err();
        assert!(e.message.contains("division by zero"), "{}", e.message);
    }

    #[test]
    fn comparison_and_logical_short_circuit() {
        assert_eq!(out(r#"function main() { println("{1 < 2 && 3 >= 3}"); }"#), "true\n");
        assert_eq!(out(r#"function main() { println("{1 > 2 || false}"); }"#), "false\n");
    }

    #[test]
    fn unary_negation_and_not() {
        assert_eq!(out(r#"function main() { println("{-5}"); println("{!true}"); }"#), "-5\nfalse\n");
    }

    #[test]
    fn var_decl_and_use() {
        assert_eq!(out(r#"function main() { int x = 10; println("{x + 5}"); }"#), "15\n");
    }

    #[test]
    fn if_else_picks_branch() {
        let src = r#"function main() { if (1 < 2) { println("yes"); } else { println("no"); } }"#;
        assert_eq!(out(src), "yes\n");
    }

    #[test]
    fn function_call_and_return() {
        let src = r#"
            function dbl(int n) -> int { return n * 2; }
            function main() { println("{dbl(21)}"); }
        "#;
        assert_eq!(out(src), "42\n");
    }

    #[test]
    fn recursion_works() {
        let src = r#"
            function fac(int n) -> int {
                if (n <= 1) { return 1; }
                return n * fac(n - 1);
            }
            function main() { println("{fac(5)}"); }
        "#;
        assert_eq!(out(src), "120\n");
    }

    #[test]
    fn enum_variant_and_match() {
        let src = r#"
            enum Shape { Circle(float r), Rect(float w, float h), }
            function area(Shape s) -> float {
                return match s {
                    Circle(r) => r * r,
                    Rect(w, h) => w * h,
                };
            }
            function main() { println("{area(Rect(3.0, 4.0))}"); }
        "#;
        assert_eq!(out(src), "12\n");
    }

    #[test]
    fn match_wildcard_is_catch_all() {
        let src = r#"
            enum E { A, B, }
            function f(E e) -> int { return match e { A => 1, _ => 2, }; }
            function main() { println("{f(B)}"); }
        "#;
        assert_eq!(out(src), "2\n");
    }

    #[test]
    fn class_construction_promotion_and_method() {
        let src = r#"
            class Greeter {
                private string name;
                constructor(private string name) {}
                function greet() -> string { return "Hi {name}"; }
            }
            function main() { Greeter g = Greeter("Tak"); println(g.greet()); }
        "#;
        assert_eq!(out(src), "Hi Tak\n");
    }

    #[test]
    fn for_loop_over_list() {
        let src = r#"
            function main() {
                List<int> xs = [1, 2, 3];
                for (int x in xs) { println("{x}"); }
            }
        "#;
        assert_eq!(out(src), "1\n2\n3\n");
    }

    #[test]
    fn missing_main_is_runtime_error() {
        let e = run(r#"function other() {}"#).unwrap_err();
        assert!(e.message.contains("main"), "{}", e.message);
    }

    #[test]
    fn interpolating_an_object_errors() {
        let src = r#"
            class C { constructor() {} }
            function main() { C c = C(); println("{c}"); }
        "#;
        let e = run(src).unwrap_err();
        assert!(e.message.contains("interpolate") || e.message.contains("print"), "{}", e.message);
    }
}
```

- [ ] **Step 2: Write the implementation at the top of `src/interpreter.rs`**

```rust
//! M1 tree-walking evaluator. Walks the untyped AST against runtime `Value`s and
//! executes `main`. The type-checker (`crate::checker`) is the gate; this stage
//! assumes type-correct input and never panics on the faults types can't catch —
//! those become `RuntimeError`. See design spec `2026-06-15-m1-plan5-evaluator-design.md`.

use std::collections::HashMap;

use crate::ast::{
    BinaryOp, ClassDecl, ClassMember, Expr, FunctionDecl, Item, MatchArm, Modifier, Pattern,
    Program, Stmt, StrPart, UnaryOp,
};
use crate::value::{EnumVal, Instance, Value};

/// A runtime fault surfaced to the caller of `interpret`.
#[derive(Debug)]
pub struct RuntimeError {
    pub message: String,
}

/// Non-local control flow threaded through `Result::Err` (EV-3).
enum Signal {
    Return(Value),
    Runtime(RuntimeError),
}

type R<T> = Result<T, Signal>;

fn rt<T>(msg: impl Into<String>) -> R<T> {
    Err(Signal::Runtime(RuntimeError { message: msg.into() }))
}

fn as_bool(v: &Value) -> R<bool> {
    match v {
        Value::Bool(b) => Ok(*b),
        other => rt(format!("expected bool, got {}", other.type_name())),
    }
}

/// One function/method call's block-scope stack (no closures in M1, so a frame
/// captures no enclosing environment).
struct Frame {
    scopes: Vec<HashMap<String, Value>>,
}

impl Frame {
    fn new() -> Self {
        Frame { scopes: vec![HashMap::new()] }
    }
    fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }
    fn pop(&mut self) {
        self.scopes.pop();
    }
    fn declare(&mut self, name: &str, v: Value) {
        self.scopes
            .last_mut()
            .expect("frame always has a base scope")
            .insert(name.to_string(), v);
    }
    fn lookup(&self, name: &str) -> Option<&Value> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }
}

pub struct Interp {
    funcs: HashMap<String, FunctionDecl>,
    classes: HashMap<String, ClassDecl>,
    /// variant name -> (enum name, arity)
    variants: HashMap<String, (String, usize)>,
    frame: Frame,
    this: Option<Value>,
    out: String,
}

/// Run a whole program: collect declarations, locate `main`, call it, and return
/// the captured stdout buffer (the Plan 6 CLI prints it to real stdout).
pub fn interpret(program: &Program) -> Result<String, RuntimeError> {
    let mut interp = Interp {
        funcs: HashMap::new(),
        classes: HashMap::new(),
        variants: HashMap::new(),
        frame: Frame::new(),
        this: None,
        out: String::new(),
    };
    interp.collect(program);
    let main = match interp.funcs.get("main") {
        Some(f) => f.clone(),
        None => return Err(RuntimeError { message: "no `main` function".to_string() }),
    };
    let names: Vec<String> = main.params.iter().map(|p| p.name.clone()).collect();
    match interp.run_call(&names, &main.body, vec![], None) {
        Ok(_) => Ok(interp.out),
        Err(Signal::Return(_)) => Ok(interp.out),
        Err(Signal::Runtime(e)) => Err(e),
    }
}

impl Interp {
    fn collect(&mut self, program: &Program) {
        for item in &program.items {
            match item {
                Item::Function(f) => {
                    self.funcs.insert(f.name.clone(), f.clone());
                }
                Item::Enum(e) => {
                    for v in &e.variants {
                        self.variants
                            .insert(v.name.clone(), (e.name.clone(), v.fields.len()));
                    }
                }
                Item::Class(c) => {
                    self.classes.insert(c.name.clone(), c.clone());
                }
                Item::Import { .. } => {}
            }
        }
    }

    /// Run a callable body in a fresh frame: bind `args` to `names` in the base
    /// scope, set `this`, execute, restore caller state. A `Return` becomes the
    /// value; falling off the end yields `Unit`.
    fn run_call(
        &mut self,
        names: &[String],
        body: &[Stmt],
        args: Vec<Value>,
        this: Option<Value>,
    ) -> R<Value> {
        let saved_frame = std::mem::replace(&mut self.frame, Frame::new());
        let saved_this = std::mem::replace(&mut self.this, this);
        for (n, a) in names.iter().zip(args.into_iter()) {
            self.frame.declare(n, a);
        }
        let result = self.exec_stmts(body);
        self.frame = saved_frame;
        self.this = saved_this;
        match result {
            Ok(()) => Ok(Value::Unit),
            Err(Signal::Return(v)) => Ok(v),
            Err(other) => Err(other),
        }
    }

    fn exec_stmts(&mut self, stmts: &[Stmt]) -> R<()> {
        for s in stmts {
            self.exec_stmt(s)?;
        }
        Ok(())
    }

    fn exec_scoped(&mut self, stmts: &[Stmt]) -> R<()> {
        self.frame.push();
        let r = self.exec_stmts(stmts);
        self.frame.pop();
        r
    }

    fn exec_stmt(&mut self, s: &Stmt) -> R<()> {
        match s {
            Stmt::VarDecl { name, init, .. } => {
                let v = self.eval(init)?;
                self.frame.declare(name, v);
                Ok(())
            }
            Stmt::Return { value, .. } => {
                let v = match value {
                    Some(e) => self.eval(e)?,
                    None => Value::Unit,
                };
                Err(Signal::Return(v))
            }
            Stmt::If { cond, then_block, else_block, .. } => {
                if as_bool(&self.eval(cond)?)? {
                    self.exec_scoped(then_block)
                } else if let Some(eb) = else_block {
                    self.exec_scoped(eb)
                } else {
                    Ok(())
                }
            }
            Stmt::For { name, iter, body, .. } => {
                let items = match self.eval(iter)? {
                    Value::List(items) => items,
                    other => {
                        return rt(format!("cannot iterate over {}", other.type_name()))
                    }
                };
                for item in items {
                    self.frame.push();
                    self.frame.declare(name, item);
                    let r = self.exec_stmts(body);
                    self.frame.pop();
                    r?;
                }
                Ok(())
            }
            Stmt::Block(stmts, _) => self.exec_scoped(stmts),
            Stmt::Expr(e, _) => {
                self.eval(e)?;
                Ok(())
            }
        }
    }

    fn eval(&mut self, e: &Expr) -> R<Value> {
        match e {
            Expr::Int(n, _) => Ok(Value::Int(*n)),
            Expr::Float(x, _) => Ok(Value::Float(*x)),
            Expr::Bool(b, _) => Ok(Value::Bool(*b)),
            Expr::Null(_) => rt("null values are not supported in M1"),
            Expr::Str(parts, _) => self.eval_str(parts),
            Expr::Ident(name, _) => self.eval_ident(name),
            Expr::This(_) => match &self.this {
                Some(v) => Ok(v.clone()),
                None => rt("`this` used outside a method"),
            },
            Expr::List(items, _) => {
                let mut out = Vec::with_capacity(items.len());
                for it in items {
                    out.push(self.eval(it)?);
                }
                Ok(Value::List(out))
            }
            Expr::Unary { op, expr, .. } => self.eval_unary(*op, expr),
            Expr::Binary { op, lhs, rhs, .. } => self.eval_binary(*op, lhs, rhs),
            Expr::Call { callee, args, .. } => self.eval_call(callee, args),
            Expr::Member { object, name, .. } => {
                match self.eval(object)? {
                    Value::Instance(inst) => match inst.fields.get(name) {
                        Some(v) => Ok(v.clone()),
                        None => rt(format!("no field `{name}` on `{}`", inst.class)),
                    },
                    other => {
                        rt(format!("cannot read `.{name}` on {}", other.type_name()))
                    }
                }
            }
            Expr::Index { .. } => rt("indexing is not yet supported in M1"),
            Expr::Match { scrutinee, arms, .. } => self.eval_match(scrutinee, arms),
        }
    }

    fn eval_ident(&mut self, name: &str) -> R<Value> {
        if let Some(v) = self.frame.lookup(name) {
            return Ok(v.clone());
        }
        // bare field reference inside a method body (mirrors checker scope seeding)
        if let Some(Value::Instance(inst)) = &self.this {
            if let Some(v) = inst.fields.get(name) {
                return Ok(v.clone());
            }
        }
        rt(format!("undefined variable `{name}`"))
    }

    fn eval_str(&mut self, parts: &[StrPart]) -> R<Value> {
        let mut s = String::new();
        for part in parts {
            match part {
                StrPart::Literal(lit) => s.push_str(lit),
                StrPart::Expr(e) => {
                    let v = self.eval(e)?;
                    match v.as_display() {
                        Some(text) => s.push_str(&text),
                        None => {
                            return rt(format!(
                                "cannot interpolate {} into a string",
                                v.type_name()
                            ))
                        }
                    }
                }
            }
        }
        Ok(Value::Str(s))
    }

    fn eval_unary(&mut self, op: UnaryOp, expr: &Expr) -> R<Value> {
        let v = self.eval(expr)?;
        match (op, v) {
            (UnaryOp::Neg, Value::Int(n)) => Ok(Value::Int(-n)),
            (UnaryOp::Neg, Value::Float(x)) => Ok(Value::Float(-x)),
            (UnaryOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
            (op, v) => rt(format!("cannot apply {op:?} to {}", v.type_name())),
        }
    }

    fn eval_binary(&mut self, op: BinaryOp, lhs: &Expr, rhs: &Expr) -> R<Value> {
        use BinaryOp::*;
        if matches!(op, And | Or) {
            let l = as_bool(&self.eval(lhs)?)?;
            return match op {
                And if !l => Ok(Value::Bool(false)),
                Or if l => Ok(Value::Bool(true)),
                _ => Ok(Value::Bool(as_bool(&self.eval(rhs)?)?)),
            };
        }
        let l = self.eval(lhs)?;
        let r = self.eval(rhs)?;
        match op {
            Add | Sub | Mul | Div | Rem => arith(op, l, r),
            Eq => Ok(Value::Bool(l.eq_val(&r))),
            NotEq => Ok(Value::Bool(!l.eq_val(&r))),
            Is => Ok(Value::Bool(l.eq_val(&r))),
            Lt | Gt | Le | Ge => compare(op, l, r),
            Pipe => rt("the `|>` pipe operator is not yet supported in M1"),
            And | Or => unreachable!("handled above"),
        }
    }

    fn eval_call(&mut self, callee: &Expr, args: &[Expr]) -> R<Value> {
        // method call: `object.name(args)`
        if let Expr::Member { object, name, .. } = callee {
            let recv = self.eval(object)?;
            let argv = self.eval_args(args)?;
            return self.call_method(recv, name, argv);
        }
        if let Expr::Ident(name, _) = callee {
            let argv = self.eval_args(args)?;
            if name == "println" {
                return self.builtin_println(argv);
            }
            if let Some(f) = self.funcs.get(name).cloned() {
                if argv.len() != f.params.len() {
                    return rt(format!(
                        "`{name}` expects {} args, got {}",
                        f.params.len(),
                        argv.len()
                    ));
                }
                let names: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
                return self.run_call(&names, &f.body, argv, None);
            }
            if let Some((enum_name, arity)) = self.variants.get(name).cloned() {
                if argv.len() != arity {
                    return rt(format!(
                        "variant `{name}` expects {arity} args, got {}",
                        argv.len()
                    ));
                }
                return Ok(Value::Enum(Box::new(EnumVal {
                    ty: enum_name,
                    variant: name.clone(),
                    payload: argv,
                })));
            }
            if self.classes.contains_key(name) {
                return self.construct(name, argv);
            }
            return rt(format!("`{name}` is not a function, variant, or class"));
        }
        rt("unsupported call target")
    }

    fn eval_args(&mut self, args: &[Expr]) -> R<Vec<Value>> {
        let mut out = Vec::with_capacity(args.len());
        for a in args {
            out.push(self.eval(a)?);
        }
        Ok(out)
    }

    fn builtin_println(&mut self, args: Vec<Value>) -> R<Value> {
        let mut line = String::new();
        for (i, a) in args.iter().enumerate() {
            if i > 0 {
                line.push(' ');
            }
            match a.as_display() {
                Some(t) => line.push_str(&t),
                None => return rt(format!("println cannot print {}", a.type_name())),
            }
        }
        self.out.push_str(&line);
        self.out.push('\n');
        Ok(Value::Unit)
    }

    /// Construct a class instance. Applies constructor *promotion* at runtime
    /// (EV-4): each promoted ctor param (carrying a visibility modifier) becomes a
    /// field. Required for the §6 empty-body constructor to populate `name`.
    fn construct(&mut self, class_name: &str, args: Vec<Value>) -> R<Value> {
        let class = self
            .classes
            .get(class_name)
            .cloned()
            .expect("caller checked the class exists");
        let ctor = class.members.iter().find_map(|m| match m {
            ClassMember::Constructor { params, body, .. } => Some((params.clone(), body.clone())),
            _ => None,
        });
        let mut inst = Instance { class: class_name.to_string(), fields: HashMap::new() };
        let Some((params, body)) = ctor else {
            if !args.is_empty() {
                return rt(format!("`{class_name}` has no constructor but got args"));
            }
            return Ok(Value::Instance(Box::new(inst)));
        };
        if args.len() != params.len() {
            return rt(format!(
                "constructor of `{class_name}` expects {} args, got {}",
                params.len(),
                args.len()
            ));
        }
        for (p, a) in params.iter().zip(args.iter()) {
            let promoted = p
                .modifiers
                .iter()
                .any(|m| matches!(m, Modifier::Public | Modifier::Private | Modifier::Protected));
            if promoted {
                inst.fields.insert(p.name.clone(), a.clone());
            }
        }
        // Run the body for side effects with `this` + params in scope. In M1 the
        // body cannot mutate fields (no reassignment), so the promoted instance is
        // the result regardless of the body's return.
        let names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
        let this = Value::Instance(Box::new(inst.clone()));
        self.run_call(&names, &body, args, Some(this))?;
        Ok(Value::Instance(Box::new(inst)))
    }

    fn call_method(&mut self, recv: Value, name: &str, args: Vec<Value>) -> R<Value> {
        let inst = match recv {
            Value::Instance(inst) => inst,
            other => return rt(format!("cannot call `.{name}()` on {}", other.type_name())),
        };
        let class = match self.classes.get(&inst.class).cloned() {
            Some(c) => c,
            None => return rt(format!("unknown class `{}`", inst.class)),
        };
        let method = class.members.iter().find_map(|m| match m {
            ClassMember::Method(f) if f.name == name => Some(f.clone()),
            _ => None,
        });
        let f = match method {
            Some(f) => f,
            None => return rt(format!("no method `{name}` on `{}`", inst.class)),
        };
        if args.len() != f.params.len() {
            return rt(format!(
                "method `{name}` expects {} args, got {}",
                f.params.len(),
                args.len()
            ));
        }
        let names: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
        self.run_call(&names, &f.body, args, Some(Value::Instance(inst)))
    }

    fn eval_match(&mut self, scrutinee: &Expr, arms: &[MatchArm]) -> R<Value> {
        let value = self.eval(scrutinee)?;
        for arm in arms {
            let mut bindings = Vec::new();
            if match_pattern(&arm.pattern, &value, &mut bindings) {
                self.frame.push();
                for (n, v) in bindings {
                    self.frame.declare(&n, v);
                }
                let r = self.eval(&arm.body);
                self.frame.pop();
                return r;
            }
        }
        rt("non-exhaustive match at runtime")
    }
}

fn arith(op: BinaryOp, l: Value, r: Value) -> R<Value> {
    use BinaryOp::*;
    match (l, r) {
        (Value::Int(a), Value::Int(b)) => {
            let v = match op {
                Add => a + b,
                Sub => a - b,
                Mul => a * b,
                Div => {
                    if b == 0 {
                        return rt("division by zero");
                    }
                    a / b
                }
                Rem => {
                    if b == 0 {
                        return rt("modulo by zero");
                    }
                    a % b
                }
                _ => unreachable!("arith only called with +-*/%"),
            };
            Ok(Value::Int(v))
        }
        (Value::Float(a), Value::Float(b)) => {
            let v = match op {
                Add => a + b,
                Sub => a - b,
                Mul => a * b,
                Div => a / b,
                Rem => a % b,
                _ => unreachable!("arith only called with +-*/%"),
            };
            Ok(Value::Float(v))
        }
        (l, r) => rt(format!(
            "cannot apply {op:?} to {} and {}",
            l.type_name(),
            r.type_name()
        )),
    }
}

fn compare(op: BinaryOp, l: Value, r: Value) -> R<Value> {
    use BinaryOp::*;
    let ord = match (&l, &r) {
        (Value::Int(a), Value::Int(b)) => a.partial_cmp(b),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
        _ => {
            return rt(format!(
                "cannot compare {} and {}",
                l.type_name(),
                r.type_name()
            ))
        }
    };
    let res = match ord {
        Some(o) => match op {
            Lt => o.is_lt(),
            Gt => o.is_gt(),
            Le => o.is_le(),
            Ge => o.is_ge(),
            _ => unreachable!("compare only called with < > <= >="),
        },
        None => false, // NaN compares false
    };
    Ok(Value::Bool(res))
}

/// Try to match `pat` against `value`, pushing any bindings. Returns whether it
/// matched. (Free function: no interpreter state needed.)
#[allow(clippy::float_cmp)] // intentional: literal float patterns match exactly
fn match_pattern(pat: &Pattern, value: &Value, out: &mut Vec<(String, Value)>) -> bool {
    match pat {
        Pattern::Wildcard(_) => true,
        Pattern::Binding { name, .. } => {
            out.push((name.clone(), value.clone()));
            true
        }
        Pattern::Int(n, _) => matches!(value, Value::Int(v) if v == n),
        Pattern::Float(x, _) => matches!(value, Value::Float(v) if v == x),
        Pattern::Str(s, _) => matches!(value, Value::Str(v) if v == s),
        Pattern::Bool(b, _) => matches!(value, Value::Bool(v) if v == b),
        Pattern::Null(_) => false, // no null values in M1
        Pattern::Variant { name, fields, .. } => {
            if let Value::Enum(ev) = value {
                if &ev.variant == name && ev.payload.len() == fields.len() {
                    return fields
                        .iter()
                        .zip(&ev.payload)
                        .all(|(fp, fv)| match_pattern(fp, fv, out));
                }
            }
            false
        }
    }
}
```

- [ ] **Step 3: Wire the module in `src/lib.rs`**

Add `pub mod interpreter;` after `pub mod checker;`:

```rust
pub mod checker;
pub mod interpreter;
```

- [ ] **Step 4: Run the tests**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test interpreter::`
Expected: all interpreter unit tests pass; full suite green.

- [ ] **Step 5: Clippy**

Run: `cargo clippy --all-targets 2>&1 | tail -8`
Expected: exit 0, no warnings.

- [ ] **Step 6: Commit**

```bash
git add src/interpreter.rs src/lib.rs
git commit -m "feat(eval): tree-walking interpreter (M1 Plan 5 Task 2)"
```

---

## Task 3: End-to-end integration (`tests/run_integration.rs`)

Run the verbatim §6 sample and assert the exact printed output, plus the two
error paths.

**Files:**
- Create: `tests/run_integration.rs`

- [ ] **Step 1: Write the integration test**

```rust
use phorge::interpreter::interpret;
use phorge::lexer::lex;
use phorge::parser::Parser;

/// The complete sample program from the language design spec (§6), verbatim.
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

fn run(src: &str) -> Result<String, phorge::interpreter::RuntimeError> {
    let tokens = lex(src).expect("lex ok");
    let prog = Parser::new(tokens).parse_program().expect("parse ok");
    interpret(&prog)
}

#[test]
fn sample_program_runs_and_prints_expected_output() {
    let out = run(SAMPLE).expect("sample should run clean");
    assert_eq!(out, "Hello Tak\narea = 12.56636\narea = 12\n");
}

#[test]
fn program_without_main_errors() {
    let e = run(r#"function helper() -> int { return 1; }"#).unwrap_err();
    assert!(e.message.contains("main"), "{}", e.message);
}

#[test]
fn division_by_zero_does_not_panic() {
    let e = run(r#"function main() { println("{1 / 0}"); }"#).unwrap_err();
    assert!(e.message.contains("division by zero"), "{}", e.message);
}
```

- [ ] **Step 2: Run the tests**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --test run_integration`
Expected: 3 tests pass.

- [ ] **Step 3: Full suite + clippy**

Run: `cargo test && cargo clippy --all-targets 2>&1 | tail -5`
Expected: full suite green (95 prior + new), clippy exit 0.

- [ ] **Step 4: Commit**

```bash
git add tests/run_integration.rs
git commit -m "test(eval): §6 sample runs end-to-end (M1 Plan 5 Task 3)"
```

---

## Completion Gate (run before declaring done)

| Dimension | Evidence to produce |
|---|---|
| **Coverage** | Paste `cargo test` count: Task 1 value tests + Task 2 interpreter tests + Task 3 integration. Every eval rule has a test. |
| **Docs** | New public API `phorge::interpreter::{interpret, RuntimeError}` and `phorge::value::Value`; spec doc records EV-1..EV-8. Update `handoff.md` (Plan 5 done, 5/6). |
| **Config** | No config impact (pure library addition) — state it. |
| **Blast radius** | `git diff --stat` — only `src/value.rs`, `src/interpreter.rs`, `src/lib.rs`, `tests/run_integration.rs`. Confirm parser/lexer/checker untouched. Panic-probe optional: malformed-but-type-correct programs should error, not panic. |

## Self-Review (run after writing, before execution)

- **Spec coverage:** §1 goal (sample output) → Task 3. §5 values → Task 1. §6 env
  → `Frame`/`run_call`. §7 signals → `Signal`/`R`. §8 rules → `eval`/`exec_stmt`.
  §9 promotion → `construct` (EV-4). §10 match → `eval_match`/`match_pattern`.
  §11 API → `interpret`. §12 testing → all three task test modules. ✓
- **Placeholder scan:** no TBD/TODO; every step has complete code. ✓
- **Type consistency:** `run_call(names, body, args, this)` signature is identical
  at every call site (interpret, function call, ctor, method). `Value`/`Instance`/
  `EnumVal` field names match Task 1. AST node names verified against `src/ast.rs`
  (`Stmt::VarDecl{ty,name,init}`, `Expr::Call{callee,args}`, `ClassMember::Constructor{params,body}`,
  `CtorParam.modifiers`, `Pattern::Variant{name,fields}`). ✓
