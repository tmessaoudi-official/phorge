//! M1 tree-walking evaluator. Walks the untyped AST against runtime `Value`s and
//! executes `main`. The type-checker (`crate::checker`) is the gate; this stage
//! assumes type-correct input and never panics on the faults types can't catch —
//! those become a runtime `Diagnostic`. See design spec `2026-06-15-m1-plan5-evaluator-design.md`.

use std::collections::HashMap;

use crate::ast::{
    BinaryOp, ClassDecl, ClassMember, Expr, FunctionDecl, Item, MatchArm, Modifier, Pattern,
    Program, Stmt, StrPart, UnaryOp,
};
use crate::diagnostic::Diagnostic;
use crate::value::{EnumVal, Instance, Value};

/// Non-local control flow threaded through `Result::Err` (EV-3). The runtime fault carries a
/// unified [`Diagnostic`] (stage `Runtime`); the tree-walker tracks no source position, so the
/// diagnostic has none (line 0) — the VM, which knows `Chunk.lines`, is the backend that locates
/// runtime faults.
enum Signal {
    Return(Value),
    Runtime(Diagnostic),
}

type R<T> = Result<T, Signal>;

fn rt<T>(msg: impl Into<String>) -> R<T> {
    Err(Signal::Runtime(Diagnostic::runtime(msg)))
}

fn as_bool(v: &Value) -> R<bool> {
    match v {
        Value::Bool(b) => Ok(*b),
        other => rt(format!("expected bool, got {}", other.type_name())),
    }
}

/// The lexical block-scope stack of the *currently executing* call — a `Vec` of scopes
/// (innermost last), pushed/popped as the tree-walker enters and leaves blocks. No closures in
/// M1, so it captures no enclosing environment. NB despite the holding field being named `frame`,
/// this is **not** a call frame: it is the opposite concept from `vm::Frame`
/// (`{func, ip, slot_base}`, a reified call record). The tree-walker keeps its call records on the
/// native Rust stack, so the only per-call state it reifies is this scope chain.
struct CallScopes {
    scopes: Vec<HashMap<String, Value>>,
}

impl CallScopes {
    fn new() -> Self {
        CallScopes {
            scopes: vec![HashMap::new()],
        }
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
    frame: CallScopes,
    this: Option<Value>,
    out: String,
    /// Live call-frame depth, checked against [`crate::limits::MAX_CALL_DEPTH`] in `run_call`.
    /// Converts unbounded recursion into a clean `"stack overflow"` fault instead of a native
    /// stack abort — and uses the *same* limit as the VM, keeping the backends parity-identical.
    depth: usize,
}

/// Run a whole program: collect declarations, locate `main`, call it, and return
/// the captured stdout buffer (the Plan 6 CLI prints it to real stdout).
///
/// The tree-walker recurses on the native Rust stack, so deep recursion needs a generous stack for
/// the `run_call` depth guard (not a native abort) to be what stops it. That stack is supplied by
/// the caller — `cli::cmd_run` runs the whole pipeline on a 256 MB worker thread — keeping this
/// function a plain recursive walk.
pub fn interpret(program: &Program) -> Result<String, Diagnostic> {
    let mut interp = Interp {
        funcs: HashMap::new(),
        classes: HashMap::new(),
        variants: HashMap::new(),
        frame: CallScopes::new(),
        this: None,
        out: String::new(),
        depth: 0,
    };
    interp.collect(program);
    let main = match interp.funcs.get("main") {
        Some(f) => f.clone(),
        None => return Err(Diagnostic::runtime("no `main` function")),
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
        // Mirror the VM's frame cap: past the shared limit, fault cleanly instead of letting
        // native recursion abort the process. Checked before incrementing, so the guard path
        // leaves `depth` untouched and every non-guard exit below decrements exactly once.
        if self.depth >= crate::limits::MAX_CALL_DEPTH {
            return rt("stack overflow");
        }
        self.depth += 1;
        let saved_frame = std::mem::replace(&mut self.frame, CallScopes::new());
        let saved_this = std::mem::replace(&mut self.this, this);
        for (n, a) in names.iter().zip(args) {
            self.frame.declare(n, a);
        }
        let result = self.exec_stmts(body);
        self.frame = saved_frame;
        self.this = saved_this;
        self.depth -= 1;
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
            Stmt::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                if as_bool(&self.eval(cond)?)? {
                    self.exec_scoped(then_block)
                } else if let Some(eb) = else_block {
                    self.exec_scoped(eb)
                } else {
                    Ok(())
                }
            }
            Stmt::For {
                name, iter, body, ..
            } => {
                let items = match self.eval(iter)? {
                    Value::List(items) => items,
                    other => return rt(format!("cannot iterate over {}", other.type_name())),
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
            Expr::Member { object, name, .. } => match self.eval(object)? {
                Value::Instance(inst) => match inst.fields.get(name) {
                    Some(v) => Ok(v.clone()),
                    None => rt(format!("no field `{name}` on `{}`", inst.class)),
                },
                other => rt(format!("cannot read `.{name}` on {}", other.type_name())),
            },
            Expr::Index { .. } => rt("indexing is not yet supported in M1"),
            Expr::Match {
                scrutinee, arms, ..
            } => self.eval_match(scrutinee, arms),
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
            (UnaryOp::Neg, Value::Int(n)) => match crate::value::int_neg(n) {
                Ok(v) => Ok(Value::Int(v)),
                Err(msg) => rt(msg),
            },
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
        let mut inst = Instance {
            class: class_name.to_string(),
            fields: HashMap::new(),
        };
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
            let promoted = p.modifiers.iter().any(|m| {
                matches!(
                    m,
                    Modifier::Public | Modifier::Private | Modifier::Protected
                )
            });
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
            // Checked ops via the single-sourced `value` kernels: overflow / div-zero / mod-zero are
            // faults the type system can't catch, so they become a Diagnostic, never a panic
            // (EV-7). The VM dispatches into the *same* kernels, so the fault path can't diverge.
            let v = match op {
                Add => crate::value::int_add(a, b),
                Sub => crate::value::int_sub(a, b),
                Mul => crate::value::int_mul(a, b),
                Div => crate::value::int_div(a, b),
                Rem => crate::value::int_rem(a, b),
                _ => unreachable!("arith only called with +-*/%"),
            };
            match v {
                Ok(n) => Ok(Value::Int(n)),
                Err(msg) => rt(msg),
            }
        }
        (Value::Float(a), Value::Float(b)) => {
            let v = match op {
                Add => crate::value::float_add(a, b),
                Sub => crate::value::float_sub(a, b),
                Mul => crate::value::float_mul(a, b),
                Div => crate::value::float_div(a, b),
                Rem => crate::value::float_rem(a, b),
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
    // The ordering + comparability fault is single-sourced in `value::compare_ord` (the VM calls the
    // same fn); only the op→bool projection below is backend-local (the op enums differ).
    let ord = match crate::value::compare_ord(&l, &r) {
        Ok(o) => o,
        Err(msg) => return rt(msg),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    use crate::parser::Parser;

    /// Lex + parse + interpret; return captured stdout or the runtime error.
    fn run(src: &str) -> Result<String, Diagnostic> {
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
        assert_eq!(
            out(r#"function main() { println("{3.0 * 4.0}"); }"#),
            "12\n"
        );
    }

    #[test]
    fn division_by_zero_is_runtime_error() {
        let e = run(r#"function main() { println("{1 / 0}"); }"#).unwrap_err();
        assert!(e.message.contains("division by zero"), "{}", e.message);
    }

    #[test]
    fn comparison_and_logical_short_circuit() {
        assert_eq!(
            out(r#"function main() { println("{1 < 2 && 3 >= 3}"); }"#),
            "true\n"
        );
        assert_eq!(
            out(r#"function main() { println("{1 > 2 || false}"); }"#),
            "false\n"
        );
    }

    #[test]
    fn unary_negation_and_not() {
        assert_eq!(
            out(r#"function main() { println("{-5}"); println("{!true}"); }"#),
            "-5\nfalse\n"
        );
    }

    #[test]
    fn var_decl_and_use() {
        assert_eq!(
            out(r#"function main() { int x = 10; println("{x + 5}"); }"#),
            "15\n"
        );
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
        // The `_` arm catches the Rect case (sample-faithful: payload variants).
        let src = r#"
            enum Shape { Circle(float r), Rect(float w, float h), }
            function kind(Shape s) -> int { return match s { Circle(r) => 1, _ => 2, }; }
            function main() { println("{kind(Rect(1.0, 2.0))}"); }
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
    fn integer_overflow_is_runtime_error_not_panic() {
        let src = r#"function main() { println("{9223372036854775807 + 1}"); }"#;
        let e = run(src).unwrap_err();
        assert!(e.message.contains("overflow"), "{}", e.message);
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
        assert!(
            e.message.contains("interpolate") || e.message.contains("print"),
            "{}",
            e.message
        );
    }
}
