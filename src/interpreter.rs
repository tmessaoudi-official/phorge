//! M1 tree-walking evaluator. Walks the untyped AST against runtime `Value`s and
//! executes `main`. The type-checker (`crate::checker`) is the gate; this stage
//! assumes type-correct input and never panics on the faults types can't catch —
//! those become a runtime `Diagnostic`. See design spec `2026-06-15-m1-plan5-evaluator-design.md`.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::ast::{
    BinaryOp, ClassDecl, ClassMember, Expr, FunctionDecl, Item, LambdaBody, MatchArm, Modifier,
    Param, Pattern, Program, Stmt, StrPart, UnaryOp,
};
use crate::diagnostic::Diagnostic;
use crate::value::{ClosureData, EnumVal, Instance, Value};
use std::rc::Rc;

/// Non-local control flow threaded through `Result::Err` (EV-3). The runtime fault carries a
/// unified [`Diagnostic`] (stage `Runtime`); the tree-walker tracks no source position, so the
/// diagnostic has none (line 0) — the VM, which knows `Chunk.lines`, is the backend that locates
/// runtime faults.
enum Signal {
    Return(Value),
    /// `break;` — unwinds to the innermost loop, which catches it and stops (M-mut.3).
    Break,
    /// `continue;` — unwinds to the innermost loop, which catches it and starts the next iteration.
    Continue,
    Runtime(Diagnostic),
}

type R<T> = Result<T, Signal>;

/// The source line of a statement, for runtime trace frames (error-handling slice 1).
fn stmt_line(s: &Stmt) -> u32 {
    match s {
        Stmt::VarDecl { span, .. }
        | Stmt::Assign { span, .. }
        | Stmt::Return { span, .. }
        | Stmt::If { span, .. }
        | Stmt::For { span, .. }
        | Stmt::While { span, .. }
        | Stmt::CFor { span, .. }
        | Stmt::Throw { span, .. }
        | Stmt::Try { span, .. } => span.line,
        Stmt::Break(s) | Stmt::Continue(s) | Stmt::Block(_, s) | Stmt::Expr(_, s) => s.line,
    }
}

fn rt<T>(msg: impl Into<String>) -> R<T> {
    Err(Signal::Runtime(Diagnostic::runtime(msg)))
}

/// Flatten a runtime `Signal` to its message body for the higher-order-native callback boundary (a
/// [`crate::native::ClosureInvoker`] returns `Result<_, String>`, the backend-shared fault contract).
/// A `Return` escaping `call_closure` would be an interpreter bug — a closure's return value is
/// consumed inside the call, never surfaced — so it maps to a defensive internal-error string.
fn signal_msg(sig: Signal) -> String {
    match sig {
        Signal::Runtime(d) => d.message,
        Signal::Return(_) => "internal error: closure return escaped".to_string(),
        Signal::Break | Signal::Continue => "internal error: loop control escaped".to_string(),
    }
}

/// The literal text of a fault intrinsic's string-literal message argument (M-faults 2a). The checker
/// guarantees it is a single `StrPart::Literal`; defaults to empty (e.g. a bare `assert(cond)`).
fn lit_msg(e: Option<&Expr>) -> String {
    if let Some(Expr::Str(parts, _)) = e {
        if let [crate::ast::StrPart::Literal(s)] = &parts[..] {
            return s.clone();
        }
    }
    String::new()
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
    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }
    fn pop_scope(&mut self) {
        self.scopes.pop();
    }
    fn declare(&mut self, name: &str, v: Value) {
        self.scopes
            .last_mut()
            .expect("scope stack always has a base scope")
            .insert(name.to_string(), v);
    }
    fn lookup(&self, name: &str) -> Option<&Value> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }
    /// Overwrite an existing binding in the innermost scope that declares it (M-mut.1
    /// reassignment). Returns `false` if no scope holds `name` (defensive — the checker guarantees
    /// the binding exists and is `mutable`). Does NOT create a new binding.
    fn assign(&mut self, name: &str, v: Value) -> bool {
        for s in self.scopes.iter_mut().rev() {
            if let Some(slot) = s.get_mut(name) {
                *slot = v;
                return true;
            }
        }
        false
    }
}

pub struct Interp {
    funcs: HashMap<String, FunctionDecl>,
    classes: HashMap<String, ClassDecl>,
    /// Transitively-flattened interface set each class implements — the `instanceof` table, built
    /// once via [`crate::ast::class_implements`] and shared verbatim with the checker + VM so the
    /// runtime test never diverges (M-RT S2). Interfaces themselves are erased: there are no
    /// interface values, only this lookup.
    class_implements: std::collections::BTreeMap<String, Vec<String>>,
    /// variant name -> (enum name, arity)
    variants: HashMap<String, (String, usize)>,
    /// Program-lifetime `static` field storage (M-mut.7), keyed by `(class, field)`. Seeded once at
    /// load from each static's literal-const initializer; read/written via `ClassName.field`.
    statics: HashMap<(String, String), Value>,
    frame: CallScopes,
    this: Option<Value>,
    out: String,
    /// Logical call stack for runtime stack traces (error-handling slice 1). A frame is pushed at each
    /// `run_call` entry (function name + current line) and popped **only on success** — an error path
    /// skips the pop, so at the top-level catch the stack still holds every active frame to snapshot.
    /// Names mirror the VM's compiled `Function.name` (`main`, `Class::method`, `Class::new`,
    /// `Class::name$set`) so `run`-traces are byte-identical to `runvm`-traces.
    trace_stack: Vec<crate::diagnostic::Frame>,
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
        class_implements: std::collections::BTreeMap::new(),
        variants: HashMap::new(),
        statics: HashMap::new(),
        frame: CallScopes::new(),
        this: None,
        out: String::new(),
        trace_stack: Vec::new(),
        depth: 0,
    };
    interp.collect(program);
    let main = match interp.funcs.get("main") {
        Some(f) => f.clone(),
        None => return Err(Diagnostic::runtime("no `main` function")),
    };
    let names: Vec<String> = main.params.iter().map(|p| p.name.clone()).collect();
    match interp.run_call("main", &names, &main.body, vec![], None) {
        Ok(_) => Ok(interp.out),
        Err(Signal::Return(_)) => Ok(interp.out),
        Err(Signal::Runtime(e)) => Err(e.with_frames(interp.snapshot_frames())),
        // Checker-unreachable: `break`/`continue` are rejected outside a loop and caught by their
        // enclosing loop, so they never escape `main`'s body. Defensive (EV-7 parity).
        Err(Signal::Break | Signal::Continue) => {
            Err(Diagnostic::runtime("internal error: loop control escaped"))
        }
    }
}

/// Call a single named top-level function with pre-built `args`, returning its value plus the
/// captured stdout. The serve runtime (M6 W3, `crate::serve`) uses this to invoke
/// `respond(bytes) -> bytes` once per request — the one entry the socket bridge needs. The
/// interpreter is the reference backend; `run` ≡ `runvm` (the differential harness) guarantees the
/// VM would compute identical bytes, so the spike does not need a VM `call_named` (deferred — the
/// VM has no return-value capture today). Mirrors [`interpret`] exactly, but enters an arbitrary
/// named function with caller-supplied arguments instead of an argument-less `main`.
pub fn call_named(
    program: &Program,
    name: &str,
    args: Vec<Value>,
) -> Result<(Value, String), Diagnostic> {
    let mut interp = Interp {
        funcs: HashMap::new(),
        classes: HashMap::new(),
        class_implements: std::collections::BTreeMap::new(),
        variants: HashMap::new(),
        statics: HashMap::new(),
        frame: CallScopes::new(),
        this: None,
        out: String::new(),
        trace_stack: Vec::new(),
        depth: 0,
    };
    interp.collect(program);
    let f = match interp.funcs.get(name) {
        Some(f) => f.clone(),
        None => return Err(Diagnostic::runtime(format!("no `{name}` function"))),
    };
    if f.params.len() != args.len() {
        return Err(Diagnostic::runtime(format!(
            "`{name}` expects {} argument(s), got {}",
            f.params.len(),
            args.len()
        )));
    }
    let names: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
    match interp.run_call(&f.name, &names, &f.body, args, None) {
        Ok(v) => Ok((v, interp.out)),
        Err(Signal::Return(v)) => Ok((v, interp.out)),
        Err(Signal::Runtime(e)) => Err(e.with_frames(interp.snapshot_frames())),
        Err(Signal::Break | Signal::Continue) => {
            Err(Diagnostic::runtime("internal error: loop control escaped"))
        }
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
                    // Seed `static` field storage once at load from the literal-const initializer
                    // (M-mut.7) — the same `const_literal` kernel the VM's `static_inits` uses (F3).
                    for m in &c.members {
                        if let crate::ast::ClassMember::Field {
                            modifiers,
                            name,
                            init,
                            ..
                        } = m
                        {
                            if modifiers.contains(&crate::ast::Modifier::Static) {
                                let v = init
                                    .as_ref()
                                    .and_then(crate::value::const_literal)
                                    .unwrap_or(Value::Unit);
                                self.statics.insert((c.name.clone(), name.clone()), v);
                            }
                        }
                    }
                    self.classes.insert(c.name.clone(), c.clone());
                }
                // Interfaces have no runtime instances; they contribute only to the
                // `class_implements` table built below (used by `instanceof`).
                Item::Interface(_) => {}
                Item::Import { .. } => {}
                // Aliases are expanded out of the AST before any backend runs (checker::
                // expand_aliases); this arm only satisfies the exhaustive match.
                Item::TypeAlias { .. } => {}
            }
        }
        // The single shared interface table (same algorithm as the checker + VM, no divergence).
        self.class_implements = crate::ast::class_implements(program);
    }

    /// Run a callable body in a fresh frame: bind `args` to `names` in the base
    /// scope, set `this`, execute, restore caller state. A `Return` becomes the
    /// value; falling off the end yields `Unit`.
    fn run_call(
        &mut self,
        fn_name: &str,
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
        // Push a trace frame (line is filled in by `exec_stmt` as the body runs). Popped only on the
        // success arms below — an error leaves it on `trace_stack` for the top-level snapshot.
        self.trace_stack.push(crate::diagnostic::Frame {
            function: fn_name.to_string(),
            file: None,
            line: 0,
            col: 0,
        });
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
            Ok(()) => {
                self.trace_stack.pop();
                Ok(Value::Unit)
            }
            Err(Signal::Return(v)) => {
                self.trace_stack.pop();
                Ok(v)
            }
            Err(other) => Err(other),
        }
    }

    /// Snapshot the live trace stack as ordered frames (innermost → outermost) for a fault diagnostic.
    fn snapshot_frames(&self) -> Vec<crate::diagnostic::Frame> {
        self.trace_stack.iter().rev().cloned().collect()
    }

    fn exec_stmts(&mut self, stmts: &[Stmt]) -> R<()> {
        for s in stmts {
            self.exec_stmt(s)?;
        }
        Ok(())
    }

    fn exec_scoped(&mut self, stmts: &[Stmt]) -> R<()> {
        self.frame.push_scope();
        let r = self.exec_stmts(stmts);
        self.frame.pop_scope();
        r
    }

    fn exec_stmt(&mut self, s: &Stmt) -> R<()> {
        // Track the current source line on the active trace frame, so a fault reports the right line
        // (and a non-top frame reports its call-site line — the call statement currently executing).
        if let Some(fr) = self.trace_stack.last_mut() {
            fr.line = stmt_line(s);
        }
        match s {
            Stmt::VarDecl { name, init, .. } => {
                let v = self.eval(init)?;
                self.frame.declare(name, v);
                Ok(())
            }
            Stmt::Assign { target, value, .. } => match target {
                Expr::Ident(name, _) => {
                    let v = self.eval(value)?;
                    if self.frame.assign(name, v) {
                        Ok(())
                    } else {
                        rt(format!("undefined variable `{name}`"))
                    }
                }
                // Value-type element set `xs[i] = e` / `m[k] = e` (M-mut.5). Copy-on-write: clone the
                // container's `Rc` only if another binding shares it (`Rc::make_mut`), mutate, write
                // back to the local. Eval order matches the VM: index, then value.
                Expr::Index { object, index, .. } => {
                    let name = match &**object {
                        Expr::Ident(n, _) => n,
                        _ => unreachable!("checker restricts index-assign to a local container"),
                    };
                    let idx_val = self.eval(index)?;
                    let new_val = self.eval(value)?;
                    let mut container = self.frame.lookup(name).cloned().ok_or_else(|| {
                        Signal::Runtime(Diagnostic::runtime(format!("undefined variable `{name}`")))
                    })?;
                    match &mut container {
                        Value::List(xs) => {
                            let idx = match idx_val {
                                Value::Int(n) => n,
                                v => {
                                    return rt(format!(
                                        "expected int index, found {}",
                                        v.type_name()
                                    ))
                                }
                            };
                            crate::value::list_set(Rc::make_mut(xs).as_mut_slice(), idx, new_val)
                                .map_err(|m| Signal::Runtime(Diagnostic::runtime(m)))?;
                        }
                        Value::Map(m) => {
                            crate::value::map_set(Rc::make_mut(m), &idx_val, new_val)
                                .map_err(|e| Signal::Runtime(Diagnostic::runtime(e)))?;
                        }
                        v => return rt(format!("cannot index-assign {}", v.type_name())),
                    }
                    self.frame.assign(name, container);
                    Ok(())
                }
                // Shared-mutable instance field set `o.f = e` / `this.f = e` (M-mut.6). Eval the
                // object to its shared `Rc<Instance>`, then the value, then write the field in place
                // — visible through every binding (handle semantics). The `borrow_mut` is taken only
                // after the value is fully evaluated, so no borrow is held across a nested `eval`.
                Expr::Member { object, name, .. } => {
                    // Static write `ClassName.field = e` (M-mut.7): head is a class name, not a local.
                    if let Expr::Ident(cls, _) = &**object {
                        if self.frame.lookup(cls).is_none() && self.classes.contains_key(cls) {
                            let v = self.eval(value)?;
                            self.statics.insert((cls.clone(), name.clone()), v);
                            return Ok(());
                        }
                    }
                    let recv = self.eval(object)?;
                    let v = self.eval(value)?;
                    match recv {
                        Value::Instance(inst) => {
                            // A property hook (M-mut.7b) resolves before a stored field: bind `v` to
                            // the assigned value and run its `set` block with `this` = the receiver.
                            // The checker guarantees a hook assigned here has a `set` (E-HOOK-NO-SET).
                            if let Some((p, body)) = self.hook_set(&inst.class, name) {
                                // Mirror the VM's synthetic hook-setter name for trace parity.
                                let setter = format!("{}::{name}$set", inst.class);
                                self.run_call(
                                    &setter,
                                    &[p.name],
                                    &body,
                                    vec![v],
                                    Some(Value::Instance(inst)),
                                )?;
                                return Ok(());
                            }
                            inst.fields.borrow_mut().insert(name.clone(), v);
                            Ok(())
                        }
                        other => rt(format!("cannot set `.{name}` on {}", other.type_name())),
                    }
                }
                _ => unreachable!("checker rejects other assignment targets"),
            },
            Stmt::Return { value, .. } => {
                let v = match value {
                    Some(e) => self.eval(e)?,
                    None => Value::Unit,
                };
                Err(Signal::Return(v))
            }
            Stmt::If {
                cond,
                bind,
                then_block,
                else_block,
                ..
            } => {
                if let Some(name) = bind {
                    // `if (var name = cond)`: evaluate the optional; run the then-block with `name`
                    // bound to the (non-null) value only when present, else the else-block.
                    let v = self.eval(cond)?;
                    if !matches!(v, Value::Null) {
                        self.frame.push_scope();
                        self.frame.declare(name, v);
                        let r = self.exec_stmts(then_block);
                        self.frame.pop_scope();
                        r
                    } else if let Some(eb) = else_block {
                        self.exec_scoped(eb)
                    } else {
                        Ok(())
                    }
                } else if as_bool(&self.eval(cond)?)? {
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
                for item in items.iter() {
                    self.frame.push_scope();
                    self.frame.declare(name, item.clone());
                    let r = self.exec_stmts(body);
                    self.frame.pop_scope();
                    match r {
                        Ok(()) => {}
                        Err(Signal::Break) => break,
                        Err(Signal::Continue) => continue,
                        Err(other) => return Err(other),
                    }
                }
                Ok(())
            }
            Stmt::While {
                cond,
                body,
                post_cond,
                ..
            } => self.exec_while(cond, body, *post_cond),
            Stmt::CFor {
                init,
                cond,
                step,
                body,
                ..
            } => self.exec_cfor(init.as_deref(), cond.as_ref(), step.as_deref(), body),
            Stmt::Break(_) => Err(Signal::Break),
            Stmt::Continue(_) => Err(Signal::Continue),
            Stmt::Block(stmts, _) => self.exec_scoped(stmts),
            Stmt::Expr(e, _) => {
                self.eval(e)?;
                Ok(())
            }
            // M-faults 2b: native unwinding lands in Task 2b.4. The checker rejects throw/try in
            // 2b.1 by not yet wiring enforcement, and no example/test exercises them, so this arm is
            // unreachable in the current build.
            Stmt::Throw { .. } | Stmt::Try { .. } => {
                unreachable!("throw/try evaluation not yet implemented (M-faults 2b.4)")
            }
        }
    }

    /// `while (cond) { .. }` / `do { .. } while (cond);` (M-mut.3). Each iteration runs the body in
    /// its own scope; a `Signal::Break` stops the loop, `Continue` proceeds to the next test, both
    /// consumed here. while-let is desugared by the parser, so it arrives as a plain `while (true)`.
    fn exec_while(&mut self, cond: &Expr, body: &[Stmt], post_cond: bool) -> R<()> {
        // do-while runs the body once before the first test; a plain while tests first.
        if post_cond {
            loop {
                self.frame.push_scope();
                let r = self.exec_stmts(body);
                self.frame.pop_scope();
                match r {
                    Ok(()) | Err(Signal::Continue) => {}
                    Err(Signal::Break) => break,
                    Err(other) => return Err(other),
                }
                if !as_bool(&self.eval(cond)?)? {
                    break;
                }
            }
            return Ok(());
        }
        while as_bool(&self.eval(cond)?)? {
            self.frame.push_scope();
            let r = self.exec_stmts(body);
            self.frame.pop_scope();
            match r {
                Ok(()) | Err(Signal::Continue) => {}
                Err(Signal::Break) => break,
                Err(other) => return Err(other),
            }
        }
        Ok(())
    }

    /// C-style `for (init; cond; step) { body }` (M-mut.3). `init`'s binding lives in the loop's own
    /// scope (popped on exit); `continue` skips to `step`, `break` exits.
    fn exec_cfor(
        &mut self,
        init: Option<&Stmt>,
        cond: Option<&Expr>,
        step: Option<&Stmt>,
        body: &[Stmt],
    ) -> R<()> {
        self.frame.push_scope();
        let mut result = match init {
            Some(s) => self.exec_stmt(s),
            None => Ok(()),
        };
        if result.is_ok() {
            result = self.cfor_loop(cond, step, body);
        }
        self.frame.pop_scope();
        result
    }

    fn cfor_loop(&mut self, cond: Option<&Expr>, step: Option<&Stmt>, body: &[Stmt]) -> R<()> {
        loop {
            if let Some(c) = cond {
                if !as_bool(&self.eval(c)?)? {
                    break;
                }
            }
            self.frame.push_scope();
            let r = self.exec_stmts(body);
            self.frame.pop_scope();
            match r {
                Ok(()) | Err(Signal::Continue) => {}
                Err(Signal::Break) => break,
                Err(other) => return Err(other),
            }
            if let Some(s) = step {
                self.exec_stmt(s)?;
            }
        }
        Ok(())
    }

    fn eval(&mut self, e: &Expr) -> R<Value> {
        match e {
            Expr::Int(n, _) => Ok(Value::Int(*n)),
            Expr::Float(x, _) => Ok(Value::Float(*x)),
            Expr::Bool(b, _) => Ok(Value::Bool(*b)),
            Expr::Null(_) => Ok(Value::Null),
            Expr::Str(parts, _) => self.eval_str(parts),
            Expr::Bytes(b, _) => Ok(Value::Bytes(Rc::new(b.clone()))),
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
                Ok(Value::List(Rc::new(out)))
            }
            Expr::Map(pairs, _) => {
                // Evaluate key then value (source order — matches the compiler's emit and the VM's
                // pop order, so side effects fire identically), then build via the shared kernel so
                // dedup is byte-identical to `Op::MakeMap` (M-RT S3).
                let mut evaled = Vec::with_capacity(pairs.len());
                for (k, v) in pairs {
                    let kv = self.eval(k)?;
                    let vv = self.eval(v)?;
                    evaled.push((kv, vv));
                }
                match crate::value::build_map(evaled) {
                    Ok(m) => Ok(Value::Map(Rc::new(m))),
                    Err(e) => rt(e),
                }
            }
            Expr::Unary { op, expr, .. } => self.eval_unary(*op, expr),
            Expr::Binary { op, lhs, rhs, .. } => self.eval_binary(*op, lhs, rhs),
            Expr::InstanceOf {
                value, type_name, ..
            } => {
                // Runtime type test (M-RT S1; interfaces added S2): true iff `value` is an instance
                // whose class equals `type_name` OR whose class implements interface `type_name`
                // (via the shared `class_implements` table). A non-instance value is `false` (never a
                // fault) — matching PHP's `instanceof`. The class name is single-sourced on
                // `Value::Instance` (P4-4), so all three backends agree.
                let v = self.eval(value)?;
                let is = matches!(&v, Value::Instance(inst)
                    if inst.class == *type_name
                        || self
                            .class_implements
                            .get(&inst.class)
                            .is_some_and(|ifaces| ifaces.iter().any(|i| i == type_name)));
                Ok(Value::Bool(is))
            }
            Expr::Call { callee, args, .. } => self.eval_call(callee, args),
            Expr::Member {
                object, name, safe, ..
            } => {
                // Static read `ClassName.field` (M-mut.7): head is a class name, not a local.
                if !*safe {
                    if let Expr::Ident(cls, _) = &**object {
                        if self.frame.lookup(cls).is_none() && self.classes.contains_key(cls) {
                            return match self.statics.get(&(cls.clone(), name.clone())) {
                                Some(v) => Ok(v.clone()),
                                None => rt(format!("no static field `{name}` on `{cls}`")),
                            };
                        }
                    }
                }
                let recv = self.eval(object)?;
                if *safe && matches!(recv, Value::Null) {
                    Ok(Value::Null) // `o?.field` on a null receiver short-circuits to null
                } else {
                    match recv {
                        Value::Instance(inst) => {
                            // A property hook (M-mut.7b) resolves before a stored field: run its
                            // `get` with `this` bound to the receiver. The checker guarantees a hook
                            // that is read here has a `get` (E-HOOK-NO-GET otherwise).
                            if let Some(get) = self.hook_get(&inst.class, name) {
                                return self.run_hook_get(Value::Instance(inst), &get);
                            }
                            // Clone the value out and drop the borrow (handle semantics: the shared
                            // cell stays available for later mutation).
                            match inst.fields.borrow().get(name).cloned() {
                                Some(v) => Ok(v),
                                None => rt(format!("no field `{name}` on `{}`", inst.class)),
                            }
                        }
                        other => rt(format!("cannot read `.{name}` on {}", other.type_name())),
                    }
                }
            }
            Expr::Index { object, index, .. } => {
                // Evaluate the object before the index (matches the compiler's emit order and the
                // VM's pop order, so any side effects fire in the same sequence — byte-identity).
                // Polymorphic (M-RT S3): a list bounds-checks an int index; a map looks the key up.
                let obj = self.eval(object)?;
                let idx = self.eval(index)?;
                match obj {
                    Value::List(list) => {
                        let i = match idx {
                            Value::Int(n) => n,
                            v => return rt(format!("expected int index, found {}", v.type_name())),
                        };
                        // Bounds-checked: an out-of-range read faults with the *same* body the VM
                        // emits (`vm.rs` `Op::Index`), so `agree_err` classifies both as `IndexOob`.
                        match usize::try_from(i).ok().filter(|i| *i < list.len()) {
                            Some(i) => Ok(list[i].clone()),
                            None => rt("list index out of range"),
                        }
                    }
                    // Key lookup via the shared kernel — a missing key faults with the same body as
                    // the VM (`map_index`), so the two backends agree.
                    Value::Map(m) => match crate::value::map_index(&m, &idx) {
                        Ok(v) => Ok(v),
                        Err(e) => rt(e),
                    },
                    v => rt(format!("cannot index {}", v.type_name())),
                }
            }
            Expr::Force { inner, .. } => {
                // `inner!`: a present optional unwraps to its value; a `null` is a clean fault whose
                // body matches the VM's `Op::Fault(ForceUnwrapNull)`, so `agree_err` classifies both
                // as the same `FaultKind` (D-L8 / error-parity).
                let v = self.eval(inner)?;
                if matches!(v, Value::Null) {
                    rt("force-unwrap of null")
                } else {
                    Ok(v)
                }
            }
            Expr::Propagate { inner, .. } => {
                // `expr?` (M-faults 2a): a `Result`-shaped enum — `Ok(v)` unwraps to `v`; `Err(_)`
                // early-returns the whole `Err` value from the enclosing function (the checker
                // guarantees the function returns the same Result type, so this is well-typed). The
                // `Signal::Return` mirrors the VM's mid-expression `Op::Return`.
                let v = self.eval(inner)?;
                match &v {
                    Value::Enum(e) if e.variant == "Ok" => Ok(e.payload[0].clone()),
                    Value::Enum(e) if e.variant == "Err" => Err(Signal::Return(v.clone())),
                    other => rt(format!(
                        "`?` requires a Result value, got {}",
                        other.type_name()
                    )),
                }
            }
            Expr::CloneWith { object, fields, .. } => {
                // `obj with { f = e }` (M-mut.4a): a fresh instance copying `obj`'s fields with the
                // named ones overridden — the constructor is NOT run. The source `Rc` is untouched
                // (we clone its field map), so other bindings to `obj` still see the old values.
                let base = match self.eval(object)? {
                    Value::Instance(rc) => rc,
                    other => {
                        return rt(format!(
                            "`with` requires a class instance, got {}",
                            other.type_name()
                        ))
                    }
                };
                let mut new_fields = base.fields.borrow().clone();
                for (name, e) in fields {
                    let v = self.eval(e)?;
                    new_fields.insert(name.clone(), v);
                }
                Ok(Value::Instance(Rc::new(Instance {
                    class: base.class.clone(),
                    fields: RefCell::new(new_fields),
                })))
            }
            Expr::Match {
                scrutinee, arms, ..
            } => self.eval_match(scrutinee, arms),
            Expr::Range {
                start,
                end,
                inclusive,
                ..
            } => {
                // Evaluate start before end (matches the compiler's emit order, for side-effect
                // parity), then materialize via the same native ranges the VM uses.
                let s = match self.eval(start)? {
                    Value::Int(n) => n,
                    v => return rt(format!("range start must be int, found {}", v.type_name())),
                };
                let e = match self.eval(end)? {
                    Value::Int(n) => n,
                    v => return rt(format!("range end must be int, found {}", v.type_name())),
                };
                // Shared size-guarded materialization (P1-#9): a range too wide to fit faults
                // `"range too large"` on both backends instead of OOM-aborting (EV-7).
                match crate::value::build_range(s, e, *inclusive) {
                    Ok(list) => Ok(Value::List(Rc::new(list))),
                    Err(msg) => rt(msg),
                }
            }
            Expr::If {
                cond,
                then_expr,
                else_expr,
                ..
            } => {
                if as_bool(&self.eval(cond)?)? {
                    self.eval(then_expr)
                } else {
                    self.eval(else_expr)
                }
            }
            // Capture the free variables from the current scope and package them with the lambda
            // syntax tree into a `Value::Closure(Tree)`.  Names that resolve to a global function,
            // class, or variant are NOT captured (they are available globally at call time).
            Expr::Lambda {
                params, ret, body, ..
            } => {
                let free = crate::ast::free_vars(params, body);
                let env: Vec<(String, Value)> = free
                    .into_iter()
                    .filter(|name| {
                        // Skip names that are global declarations — they are always reachable at
                        // call time and must not be captured as a snapshot value.
                        !self.funcs.contains_key(name.as_str())
                            && !self.classes.contains_key(name.as_str())
                            && !self.variants.contains_key(name.as_str())
                    })
                    .filter_map(|name| self.frame.lookup(&name).map(|v| (name, v.clone())))
                    .collect();
                Ok(Value::Closure(Rc::new(ClosureData::Tree {
                    params: params.clone(),
                    ret: ret.clone(),
                    body: body.clone(),
                    env,
                })))
            }
            // `html"…"` literals are erased to `html.concat([…])` kernel calls by
            // `checker::resolve_html` before any backend runs; the interpreter never sees one.
            Expr::Html(..) => unreachable!("html literal not resolved before interpretation"),
        }
    }

    fn eval_ident(&mut self, name: &str) -> R<Value> {
        if let Some(v) = self.frame.lookup(name) {
            return Ok(v.clone());
        }
        // bare field reference inside a method body (mirrors checker scope seeding)
        if let Some(Value::Instance(inst)) = &self.this {
            if let Some(v) = inst.fields.borrow().get(name).cloned() {
                return Ok(v);
            }
        }
        // A4: bare named-function reference in value position (e.g. passing `f` to a higher-order
        // function that takes `(int)->int`).  The checker already verified the type; the interpreter
        // wraps it in a `Named` closure so `eval_call` can dispatch it uniformly.
        if self.funcs.contains_key(name) {
            return Ok(Value::Closure(Rc::new(ClosureData::Named(
                name.to_string(),
            ))));
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
        if matches!(op, Coalesce) {
            // `a ?? b`: evaluate `b` only when `a` is null (short-circuit).
            let l = self.eval(lhs)?;
            return if matches!(l, Value::Null) {
                self.eval(rhs)
            } else {
                Ok(l)
            };
        }
        let l = self.eval(lhs)?;
        let r = self.eval(rhs)?;
        match op {
            Add | Sub | Mul | Div | Rem => arith(op, l, r),
            Eq => Ok(Value::Bool(l.eq_val(&r))),
            NotEq => Ok(Value::Bool(!l.eq_val(&r))),
            Lt | Gt | Le | Ge => compare(op, l, r),
            Pipe => unreachable!("`|>` is lowered to a call in the parser"),
            And | Or | Coalesce => unreachable!("handled above"),
        }
    }

    fn eval_call(&mut self, callee: &Expr, args: &[Expr]) -> R<Value> {
        // method call: `object.name(args)`
        if let Expr::Member {
            object, name, safe, ..
        } = callee
        {
            // Namespaced native call: `console.println(x)` — a member call whose head is an imported
            // module qualifier, not a value (M3 Wave 1). Locals-first: an identifier bound as a
            // variable is a method receiver; only an *unbound* identifier can be a qualifier, and the
            // checker has already enforced the import + native, so `index_of_by_leaf` is unambiguous.
            // The native's `eval` is shared verbatim with the VM (structural parity).
            if !*safe {
                if let Expr::Ident(q, _) = &**object {
                    if self.frame.lookup(q).is_none() {
                        if let Some(idx) = crate::native::index_of_by_leaf(q, name) {
                            let argv = self.eval_args(args)?;
                            // The native reports failures as a plain `String` (the backend-shared
                            // contract); lift it into the interpreter's runtime `Signal`. A
                            // higher-order native (`List.map`/etc.) is handed an invoker that runs a
                            // closure argument via `call_closure` — the same body the VM drives with
                            // its re-entrant `call_closure_value` (structural parity, M-RT S7b-3).
                            let result = match crate::native::registry()[idx].eval {
                                crate::native::NativeEval::Pure(f) => f(&argv, &mut self.out),
                                crate::native::NativeEval::HigherOrder(f) => {
                                    let mut invoke = |fv: &Value, cargs: Vec<Value>| match fv {
                                        Value::Closure(rc) => {
                                            self.call_closure(rc.clone(), cargs).map_err(signal_msg)
                                        }
                                        v => Err(format!(
                                            "cannot call {} as a function",
                                            v.type_name()
                                        )),
                                    };
                                    f(&argv, &mut invoke)
                                }
                            };
                            return match result {
                                Ok(v) => Ok(v),
                                Err(msg) => rt(msg),
                            };
                        }
                    }
                }
            }
            let recv = self.eval(object)?;
            if *safe && matches!(recv, Value::Null) {
                // `o?.m(args)` on a null receiver short-circuits: args are NOT evaluated.
                return Ok(Value::Null);
            }
            let argv = self.eval_args(args)?;
            return self.call_method(recv, name, argv);
        }
        if let Expr::Ident(name, _) = callee {
            // Fault intrinsics (M-faults 2a) — `panic`/`todo`/`unreachable` always fault; `assert`
            // faults iff its condition is false. The message is single-sourced on `FaultMsg::message`
            // so it is byte-identical to the VM's `Op::Fault`.
            use crate::chunk::FaultMsg;
            match name.as_str() {
                "panic" => return rt(FaultMsg::Panic(lit_msg(args.first())).message()),
                "todo" => return rt(FaultMsg::Todo.message()),
                "unreachable" => return rt(FaultMsg::Unreachable.message()),
                "assert" => {
                    let cond = self.eval(&args[0])?;
                    if !matches!(cond, Value::Bool(true)) {
                        return rt(FaultMsg::Assert(lit_msg(args.get(1))).message());
                    }
                    return Ok(Value::Unit);
                }
                _ => {}
            }
            let argv = self.eval_args(args)?;
            if let Some(f) = self.funcs.get(name).cloned() {
                if argv.len() != f.params.len() {
                    return rt(format!(
                        "`{name}` expects {} args, got {}",
                        f.params.len(),
                        argv.len()
                    ));
                }
                let names: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
                return self.run_call(&f.name, &names, &f.body, argv, None);
            }
            if let Some((enum_name, arity)) = self.variants.get(name).cloned() {
                if argv.len() != arity {
                    return rt(format!(
                        "variant `{name}` expects {arity} args, got {}",
                        argv.len()
                    ));
                }
                return Ok(Value::Enum(Rc::new(EnumVal {
                    ty: enum_name,
                    variant: name.clone(),
                    payload: argv,
                })));
            }
            if self.classes.contains_key(name) {
                return self.construct(name, argv);
            }
            // The name might be a local variable holding a closure value (e.g. `var f = fn…`).
            if let Some(Value::Closure(rc)) = self.frame.lookup(name).cloned() {
                return self.call_closure(rc, argv);
            }
            return rt(format!("`{name}` is not a function, variant, or class"));
        }
        // Generic callee: evaluate the callee expression and dispatch on the resulting value.  This
        // path handles complex callee expressions (e.g. a method returning a closure).  Callee is
        // evaluated first (matching normal evaluation order), then arguments.
        let callee_val = self.eval(callee)?;
        let argv = self.eval_args(args)?;
        match callee_val {
            Value::Closure(rc) => self.call_closure(rc, argv),
            other => rt(format!("cannot call a value of type {}", other.type_name())),
        }
    }

    /// Execute a closure value with the supplied arguments.
    fn call_closure(&mut self, closure: Rc<ClosureData>, args: Vec<Value>) -> R<Value> {
        match &*closure {
            ClosureData::Tree {
                params, body, env, ..
            } => {
                if args.len() != params.len() {
                    return rt(format!(
                        "lambda expects {} arg(s), got {}",
                        params.len(),
                        args.len()
                    ));
                }
                self.call_tree_closure(params, body, env, args)
            }
            ClosureData::Named(name) => {
                let f = match self.funcs.get(name).cloned() {
                    Some(f) => f,
                    None => return rt(format!("function `{name}` not found")),
                };
                if args.len() != f.params.len() {
                    return rt(format!(
                        "`{name}` expects {} args, got {}",
                        f.params.len(),
                        args.len()
                    ));
                }
                let names: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
                self.run_call(&f.name, &names, &f.body, args, None)
            }
            ClosureData::Byte { .. } => {
                // A VM-compiled closure that somehow ended up in the tree-walker is a compiler
                // bug — surface a clean runtime error rather than panicking.
                rt("internal error: VM closure reached the tree-walking interpreter")
            }
        }
    }

    /// Core tree-closure call: saves the current frame, populates captured env + params,
    /// runs the body, then restores the frame.  `this` is always `None` for lambdas.
    fn call_tree_closure(
        &mut self,
        params: &[Param],
        body: &LambdaBody,
        env: &[(String, Value)],
        args: Vec<Value>,
    ) -> R<Value> {
        if self.depth >= crate::limits::MAX_CALL_DEPTH {
            return rt("stack overflow");
        }
        self.depth += 1;
        let saved_frame = std::mem::replace(&mut self.frame, CallScopes::new());
        let saved_this = self.this.take();
        // Inject captured environment first so params can shadow captures of the same name.
        for (k, v) in env {
            self.frame.declare(k, v.clone());
        }
        for (p, a) in params.iter().zip(args) {
            self.frame.declare(&p.name, a);
        }
        let result = match body {
            // Expression body: the evaluated result IS the return value.
            LambdaBody::Expr(e) => self.eval(e),
            LambdaBody::Block(stmts) => {
                // Statement-body lambdas land in Task 6; for now the parser rejects them, but the
                // enum variant exists.  Guard here so a future `Byte` path can't hit it silently.
                let r = self.exec_stmts(stmts);
                match r {
                    Ok(()) => Ok(Value::Unit),
                    Err(Signal::Return(v)) => Ok(v),
                    Err(other) => Err(other),
                }
            }
        };
        self.frame = saved_frame;
        self.this = saved_this;
        self.depth -= 1;
        result
    }

    fn eval_args(&mut self, args: &[Expr]) -> R<Vec<Value>> {
        let mut out = Vec::with_capacity(args.len());
        for a in args {
            out.push(self.eval(a)?);
        }
        Ok(out)
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
            fields: RefCell::new(HashMap::new()),
        };
        let Some((params, body)) = ctor else {
            if !args.is_empty() {
                return rt(format!("`{class_name}` has no constructor but got args"));
            }
            return Ok(Value::Instance(Rc::new(inst)));
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
                // We still solely own `inst` (not yet shared), so `get_mut` skips the runtime borrow.
                inst.fields.get_mut().insert(p.name.clone(), a.clone());
            }
        }
        // Run the body for side effects with `this` + params in scope. In M1 the
        // body cannot mutate fields (no reassignment), so the promoted instance is
        // the result regardless of the body's return.
        let names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
        // Share one `Rc` between the `this` receiver and the returned instance (M2 P5a): the body
        // can't mutate fields (immutable), so both observe the same value — a refcount bump, not a
        // deep `HashMap` clone of the whole instance.
        let rc = Rc::new(inst);
        let ctor = format!("{}::new", rc.class);
        self.run_call(
            &ctor,
            &names,
            &body,
            args,
            Some(Value::Instance(rc.clone())),
        )?;
        Ok(Value::Instance(rc))
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
        let mname = format!("{}::{name}", inst.class);
        self.run_call(&mname, &names, &f.body, args, Some(Value::Instance(inst)))
    }

    /// The cloned `get` expression of a property hook `class.name`, if the class declares one with a
    /// `get` (M-mut.7b). `None` if there is no such hook or it is write-only.
    fn hook_get(&self, class: &str, name: &str) -> Option<Expr> {
        self.classes
            .get(class)?
            .members
            .iter()
            .find_map(|m| match m {
                ClassMember::Hook {
                    name: n,
                    get: Some(g),
                    ..
                } if n == name => Some(g.clone()),
                _ => None,
            })
    }

    /// The cloned `set` parameter + block of a property hook `class.name`, if the class declares one
    /// with a `set` (M-mut.7b). `None` if there is no such hook or it is read-only.
    fn hook_set(&self, class: &str, name: &str) -> Option<(crate::ast::Param, Vec<Stmt>)> {
        self.classes
            .get(class)?
            .members
            .iter()
            .find_map(|m| match m {
                ClassMember::Hook {
                    name: n,
                    set: Some(s),
                    ..
                } if n == name => Some(s.clone()),
                _ => None,
            })
    }

    /// Evaluate a property hook's `get` expression with `this` bound to the receiver, in a fresh
    /// frame (M-mut.7b) — the value-returning analogue of `run_call`, but for an expression body.
    fn run_hook_get(&mut self, recv: Value, get: &Expr) -> R<Value> {
        if self.depth >= crate::limits::MAX_CALL_DEPTH {
            return rt("stack overflow");
        }
        self.depth += 1;
        let saved_frame = std::mem::replace(&mut self.frame, CallScopes::new());
        let saved_this = self.this.replace(recv);
        let result = self.eval(get);
        self.frame = saved_frame;
        self.this = saved_this;
        self.depth -= 1;
        result
    }

    fn eval_match(&mut self, scrutinee: &Expr, arms: &[MatchArm]) -> R<Value> {
        let value = self.eval(scrutinee)?;
        for arm in arms {
            let mut bindings = Vec::new();
            if match_pattern(&arm.pattern, &value, &self.class_implements, &mut bindings) {
                self.frame.push_scope();
                for (n, v) in bindings {
                    self.frame.declare(&n, v);
                }
                let r = self.eval(&arm.body);
                self.frame.pop_scope();
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

/// Try to match `pat` against `value`, pushing any bindings. Returns whether it matched. `implements`
/// is the shared `class_implements` table (needed by a type pattern to test an interface RHS — the
/// same data the `instanceof` evaluation uses, so the two can't diverge).
#[allow(clippy::float_cmp)] // intentional: literal float patterns match exactly
fn match_pattern(
    pat: &Pattern,
    value: &Value,
    implements: &std::collections::BTreeMap<String, Vec<String>>,
    out: &mut Vec<(String, Value)>,
) -> bool {
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
        Pattern::Null(_) => matches!(value, Value::Null), // M3 S2.6: `null` arm over a `T?`
        Pattern::Variant { name, fields, .. } => {
            if let Value::Enum(ev) = value {
                if &ev.variant == name && ev.payload.len() == fields.len() {
                    return fields
                        .iter()
                        .zip(&ev.payload)
                        .all(|(fp, fv)| match_pattern(fp, fv, implements, out));
                }
            }
            false
        }
        // M-RT S4 type pattern: matches iff `value` is an instance whose class equals `type_name` or
        // implements interface `type_name` — exactly the `instanceof` test (`eval` arm above), so the
        // backends agree. Binds the matched value (if a binder is present).
        Pattern::Type {
            type_name, binding, ..
        } => {
            let is = matches!(value, Value::Instance(inst)
                if inst.class == *type_name
                    || implements
                        .get(&inst.class)
                        .is_some_and(|ifaces| ifaces.iter().any(|i| i == type_name)));
            if is {
                if let Some(name) = binding {
                    out.push((name.clone(), value.clone()));
                }
            }
            is
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    use crate::parser::Parser;

    /// Lex + parse + interpret; return captured stdout or the runtime error. Auto-prepends the
    /// reserved `package main;` (M5 S1) so existing test programs need no per-case edit; the
    /// segment carries no newline, preserving line numbers.
    fn run(src: &str) -> Result<String, Diagnostic> {
        let src = with_pkg(src);
        let tokens = lex(&src).expect("lex ok");
        let prog = Parser::new(tokens).parse_program().expect("parse ok");
        interpret(&prog)
    }

    #[test]
    fn interpreter_fault_carries_call_stack() {
        let err = run(
            "function f() -> int { var xs = [1]; return xs[5]; }\nfunction main() { var r = f(); }",
        )
        .unwrap_err();
        assert_eq!(err.frames.len(), 2, "callee + main: {:?}", err.frames);
        assert_eq!(err.frames[0].function, "f");
        assert_eq!(err.frames[1].function, "main");
    }

    fn with_pkg(src: &str) -> String {
        if src.trim_start().starts_with("package ") {
            src.to_string()
        } else {
            format!("package main; {src}")
        }
    }

    fn out(src: &str) -> String {
        run(src).expect("run ok")
    }

    #[test]
    fn prints_a_literal_string() {
        assert_eq!(
            out(r#"import Core.Console;
function main() { Console.println("hi"); }"#),
            "hi\n"
        );
    }

    #[test]
    fn integer_arithmetic_in_interpolation() {
        assert_eq!(
            out(r#"import Core.Console;
function main() { Console.println("{1 + 2 * 3}"); }"#),
            "7\n"
        );
    }

    #[test]
    fn float_arithmetic() {
        assert_eq!(
            out(r#"import Core.Console;
function main() { Console.println("{3.0 * 4.0}"); }"#),
            "12\n"
        );
    }

    #[test]
    fn division_by_zero_is_runtime_error() {
        let e = run(r#"import Core.Console;
function main() { Console.println("{1 / 0}"); }"#)
        .unwrap_err();
        assert!(e.message.contains("division by zero"), "{}", e.message);
    }

    #[test]
    fn comparison_and_logical_short_circuit() {
        assert_eq!(
            out(r#"import Core.Console;
function main() { Console.println("{1 < 2 && 3 >= 3}"); }"#),
            "true\n"
        );
        assert_eq!(
            out(r#"import Core.Console;
function main() { Console.println("{1 > 2 || false}"); }"#),
            "false\n"
        );
    }

    #[test]
    fn unary_negation_and_not() {
        assert_eq!(
            out(r#"import Core.Console;
function main() { Console.println("{-5}"); Console.println("{!true}"); }"#),
            "-5\nfalse\n"
        );
    }

    #[test]
    fn var_decl_and_use() {
        assert_eq!(
            out(r#"import Core.Console;
function main() { int x = 10; Console.println("{x + 5}"); }"#),
            "15\n"
        );
    }

    #[test]
    fn if_else_picks_branch() {
        let src = r#"import Core.Console;
function main() { if (1 < 2) { Console.println("yes"); } else { Console.println("no"); } }"#;
        assert_eq!(out(src), "yes\n");
    }

    #[test]
    fn function_call_and_return() {
        let src = r#"import Core.Console;

            function dbl(int n) -> int { return n * 2; }
            function main() { Console.println("{dbl(21)}"); }
        "#;
        assert_eq!(out(src), "42\n");
    }

    #[test]
    fn recursion_works() {
        let src = r#"import Core.Console;

            function fac(int n) -> int {
                if (n <= 1) { return 1; }
                return n * fac(n - 1);
            }
            function main() { Console.println("{fac(5)}"); }
        "#;
        assert_eq!(out(src), "120\n");
    }

    #[test]
    fn enum_variant_and_match() {
        let src = r#"import Core.Console;

            enum Shape { Circle(float r), Rect(float w, float h), }
            function area(Shape s) -> float {
                return match s {
                    Circle(r) => r * r,
                    Rect(w, h) => w * h,
                };
            }
            function main() { Console.println("{area(Rect(3.0, 4.0))}"); }
        "#;
        assert_eq!(out(src), "12\n");
    }

    #[test]
    fn match_wildcard_is_catch_all() {
        // The `_` arm catches the Rect case (sample-faithful: payload variants).
        let src = r#"import Core.Console;

            enum Shape { Circle(float r), Rect(float w, float h), }
            function kind(Shape s) -> int { return match s { Circle(r) => 1, _ => 2, }; }
            function main() { Console.println("{kind(Rect(1.0, 2.0))}"); }
        "#;
        assert_eq!(out(src), "2\n");
    }

    #[test]
    fn class_construction_promotion_and_method() {
        let src = r#"import Core.Console;

            class Greeter {
                private string name;
                constructor(private string name) {}
                function greet() -> string { return "Hi {name}"; }
            }
            function main() { Greeter g = Greeter("Tak"); Console.println(g.greet()); }
        "#;
        assert_eq!(out(src), "Hi Tak\n");
    }

    #[test]
    fn for_loop_over_list() {
        let src = r#"import Core.Console;

            function main() {
                List<int> xs = [1, 2, 3];
                for (int x in xs) { Console.println("{x}"); }
            }
        "#;
        assert_eq!(out(src), "1\n2\n3\n");
    }

    #[test]
    fn indexing_reads_elements() {
        assert_eq!(
            out(r#"import Core.Console;
function main() { List<int> xs = [7, 8, 9]; Console.println("{xs[0]} {xs[2]}"); }"#),
            "7 9\n"
        );
    }

    #[test]
    fn indexing_out_of_range_is_runtime_error() {
        let e = run(r#"import Core.Console;
function main() { List<int> xs = [1]; Console.println("{xs[3]}"); }"#)
        .unwrap_err();
        assert!(
            e.message.contains("list index out of range"),
            "{}",
            e.message
        );
    }

    #[test]
    fn ranges_iterate_like_lists() {
        assert_eq!(
            out(r#"import Core.Console;
function main() { for (int i in 0..3) { Console.println("{i}"); } }"#),
            "0\n1\n2\n"
        );
        assert_eq!(
            out(r#"import Core.Console;
function main() { for (int i in 1..=3) { Console.println("{i}"); } }"#),
            "1\n2\n3\n"
        );
        // empty range (start >= end): body never runs
        assert_eq!(
            out(r#"import Core.Console;
function main() { for (int i in 5..2) { Console.println("{i}"); } Console.println("done"); }"#),
            "done\n"
        );
    }

    #[test]
    fn expression_if_picks_branch_value() {
        assert_eq!(
            out(r#"import Core.Console;
function main() { var x = if (1 < 2) { 7 } else { 9 }; Console.println("{x}"); }"#),
            "7\n"
        );
        assert_eq!(
            out(r#"import Core.Console;
function main() { var x = if (1 > 2) { 7 } else { 9 }; Console.println("{x}"); }"#),
            "9\n"
        );
    }

    #[test]
    fn integer_overflow_is_runtime_error_not_panic() {
        let src = r#"import Core.Console;
function main() { Console.println("{9223372036854775807 + 1}"); }"#;
        let e = run(src).unwrap_err();
        assert!(e.message.contains("overflow"), "{}", e.message);
    }

    #[test]
    fn missing_main_is_runtime_error() {
        let e = run(r#"function other() {}"#).unwrap_err();
        assert!(e.message.contains("main"), "{}", e.message);
    }

    // ---- lambda tests (M3 S3, Task 3 — interpreter-only) ----

    /// Lex + parse + type-check `src`; return the error diagnostics (empty = well-typed).
    /// Auto-prepends `package main;` if absent. Used to test checker rejections without
    /// running the interpreter.
    fn check_errs(src: &str) -> Vec<crate::diagnostic::Diagnostic> {
        let src = with_pkg(src);
        let tokens = lex(&src).expect("lex ok");
        let prog = Parser::new(tokens).parse_program().expect("parse ok");
        match crate::checker::check(&prog) {
            Ok(_warnings) => Vec::new(),
            Err(e) => e,
        }
    }

    #[test]
    fn lambda_value_call_interpreter() {
        let out = out(r#"package main;
import Core.Console;
function main() {
    var double = fn(int x) => x * 2;
    Console.println("{double(5)}");
}"#);
        assert_eq!(out, "10\n");
    }

    #[test]
    fn lambda_captures_two_vars_interpreter() {
        let out = out(r#"package main;
import Core.Console;
function main() {
    var a = 10;
    var b = 100;
    var f = fn(int x) => x + a + b;
    Console.println("{f(1)}");
}"#);
        assert_eq!(out, "111\n");
    }

    #[test]
    fn higher_order_user_function_interpreter() {
        let out = out(r#"package main;
import Core.Console;
function twice(int x, (int) -> int f) -> int { return f(f(x)); }
function main() {
    Console.println("{twice(3, fn(int n) => n + 1)}");
}"#);
        assert_eq!(out, "5\n");
    }

    #[test]
    fn lambda_cannot_reference_this() {
        let errs = check_errs(
            r#"package main;
class C { constructor(public int x) {}
  function method() -> (int) -> int { return fn(int n) => n + this.x; } }
function main() { }"#,
        );
        assert!(
            errs.iter().any(|e| e.message.contains("`this`")),
            "{errs:?}"
        );
    }

    #[test]
    fn interpolating_an_object_errors() {
        let src = r#"import Core.Console;

            class C { constructor() {} }
            function main() { C c = C(); Console.println("{c}"); }
        "#;
        let e = run(src).unwrap_err();
        assert!(
            e.message.contains("interpolate") || e.message.contains("print"),
            "{}",
            e.message
        );
    }
}
