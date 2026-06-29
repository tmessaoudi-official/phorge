//! Tree-walking interpreter — expr (M-Decomp W4). See mod.rs for the struct + core + entry points.

use super::*;

impl Interp {
    /// Build the zero-arg thunk closure `() => call` a `spawn` defers/invokes (M6 W4): a
    /// `ClosureData::Tree` whose body is the spawned call, capturing the call's free locals by value
    /// (and `this`, if used) at spawn time — exactly as a `fn() => call` lambda would. Single-sources
    /// the capture logic with the `Expr::Lambda` arm.
    fn make_spawn_thunk(&self, call: &Expr) -> Rc<ClosureData> {
        let body = LambdaBody::Expr(Box::new(call.clone()));
        let env: Vec<(String, Value)> = crate::ast::free_vars(&[], &body)
            .into_iter()
            .filter(|name| {
                !self.funcs.contains_key(name.as_str())
                    && !self.classes.contains_key(name.as_str())
                    && !self.variants.contains_key(name.as_str())
            })
            .filter_map(|name| self.frame.lookup(&name).map(|v| (name, v.clone())))
            .collect();
        let this_capture = if crate::ast::lambda_uses_this(&body) {
            self.this.clone()
        } else {
            None
        };
        Rc::new(ClosureData::Tree {
            params: Vec::new(),
            ret: None,
            body,
            env,
            this_capture,
        })
    }

    pub(super) fn eval(&mut self, e: &Expr) -> R<Value> {
        match e {
            Expr::Int(n, _) => Ok(Value::Int(*n)),
            Expr::Float(x, _) => Ok(Value::Float(*x)),
            Expr::Decimal {
                unscaled, scale, ..
            } => Ok(Value::Decimal {
                unscaled: *unscaled,
                scale: *scale,
            }),
            Expr::Bool(b, _) => Ok(Value::Bool(*b)),
            // `spawn <call>` (M6 W4): step-2 synchronous-degenerate — run the call now and wrap its
            // result in a completed `Task`. Step 4 will enqueue a coroutine via `green::sched` instead.
            Expr::Spawn { call, .. } => {
                // Build a zero-arg thunk closure `() => call` (capturing the call's free locals by
                // value at spawn time), then — eager path — invoke it now and store the result by a
                // fresh task id. The cooperative cutover defers the thunk as a coroutine instead.
                let thunk = self.make_spawn_thunk(call);
                let result = self.call_closure(thunk, Vec::new())?;
                let id = self.coop.borrow_mut().sched.spawn();
                self.coop.borrow_mut().results.insert(id, result);
                Ok(Value::Task(id))
            }
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
                // Runtime type test (M-RT S1; interfaces S2; class ancestors S6c.3): true iff `value`
                // is an instance whose class equals `type_name` OR has `type_name` among its
                // supertypes — parent classes AND interfaces, via the shared `instanceof_table`
                // oracle. A non-instance value is `false` (never a fault) — matching PHP's
                // `instanceof`. The class name is single-sourced on `Value::Instance` (P4-4), so all
                // three backends agree.
                let v = self.eval(value)?;
                let is = matches!(&v, Value::Instance(inst)
                    if inst.class == *type_name
                        || self
                            .class_implements
                            .get(&inst.class)
                            .is_some_and(|ifaces| ifaces.iter().any(|i| i == type_name)));
                Ok(Value::Bool(is))
            }
            Expr::Cast {
                value, type_name, ..
            } => {
                // M4 as-matrix: a primitive `as` that is a CONVERSION was rewritten to a native call
                // before this backend; the only primitive `Cast` reaching here is the **identity**
                // (`T as T`) — evaluate the value unchanged.
                if matches!(
                    type_name.as_str(),
                    "int" | "float" | "string" | "bool" | "decimal"
                ) {
                    return self.eval(value);
                }
                // Checked downcast (M4 casting axis 2): evaluate `value` ONCE; keep it when it really is
                // a `type_name` (same predicate as `instanceof` above — exact class or a supertype via
                // `class_implements`), else produce `null`. Result type is `type_name?` (checker). Never
                // faults — `as` is the safe alternative to `opt!`-style force-unwrap.
                let v = self.eval(value)?;
                let is = matches!(&v, Value::Instance(inst)
                    if inst.class == *type_name
                        || self
                            .class_implements
                            .get(&inst.class)
                            .is_some_and(|ifaces| ifaces.iter().any(|i| i == type_name)));
                Ok(if is { v } else { Value::Null })
            }
            Expr::Call { callee, args, .. } => self.eval_call(callee, args),
            Expr::Member {
                object, name, safe, ..
            } => {
                // Static read `ClassName.field` (M-mut.7): head is a class name, not a local.
                if !*safe {
                    if let Expr::Ident(cls, _) = &**object {
                        if self.frame.lookup(cls).is_none() && self.classes.contains_key(cls) {
                            // A `const` class constant (Feature A) is inlined before a static field;
                            // the checker has already enforced visibility + class-name-only access.
                            if let Some(v) = self.consts.get(&(cls.clone(), name.clone())) {
                                return Ok(v.clone());
                            }
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
                            match inst.get_field(name) {
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
                // S1b: a `with` clone reuses the base's shared layout (same class ⇒ same slots), so
                // copy the slot `Vec` and overwrite the named slots by name.
                let new_inst = Instance {
                    class: base.class.clone(),
                    layout: base.layout.clone(),
                    fields: RefCell::new(base.fields.borrow().clone()),
                };
                for (name, e) in fields {
                    let v = self.eval(e)?;
                    new_inst.set_field(name, v);
                }
                Ok(Value::Instance(Rc::new(new_inst)))
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
                // Capture `this` (the live `Rc` instance handle) when the body references it,
                // including through a nested lambda (Phase 1 closures slice). `None` otherwise, so a
                // non-`this` lambda is unchanged.
                let this_capture = if crate::ast::lambda_uses_this(body) {
                    self.this.clone()
                } else {
                    None
                };
                Ok(Value::Closure(Rc::new(ClosureData::Tree {
                    params: params.clone(),
                    ret: ret.clone(),
                    body: body.clone(),
                    env,
                    this_capture,
                })))
            }
            // `html"…"` literals are erased to `html.concat([…])` kernel calls by
            // `checker::resolve_html` before any backend runs; the interpreter never sees one.
            Expr::Html(..) => unreachable!("html literal not resolved before interpretation"),
            Expr::OverloadSelect { .. } => {
                unreachable!(
                    "overload selector resolved + rewritten before interpretation (Slice C1)"
                )
            }
            // `parent.m(args)` / `parent(A).m(args)` — super/parent dispatch (M-RT super/parent).
            Expr::ParentCall {
                ancestor,
                method,
                args,
                ..
            } => {
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval(a)?);
                }
                self.eval_parent_call(ancestor.as_deref(), method, argv)
            }
            Expr::New(..) => {
                unreachable!("Expr::New is unwrapped before interpretation (checker::unwrap_new)")
            }
        }
    }

    pub(super) fn eval_ident(&mut self, name: &str) -> R<Value> {
        if let Some(v) = self.frame.lookup(name) {
            return Ok(v.clone());
        }
        // bare field reference inside a method body (mirrors checker scope seeding)
        if let Some(Value::Instance(inst)) = &self.this {
            if let Some(v) = inst.get_field(name) {
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

    pub(super) fn eval_str(&mut self, parts: &[StrPart]) -> R<Value> {
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

    pub(super) fn eval_unary(&mut self, op: UnaryOp, expr: &Expr) -> R<Value> {
        let v = self.eval(expr)?;
        match (op, v) {
            (UnaryOp::Neg, Value::Int(n)) => match crate::value::int_neg(n) {
                Ok(v) => Ok(Value::Int(v)),
                Err(msg) => rt(msg),
            },
            (UnaryOp::Neg, Value::Float(x)) => Ok(Value::Float(-x)),
            (UnaryOp::Neg, Value::Decimal { unscaled, scale }) => {
                // Negate via the shared kernel (checked — `i128::MIN` faults, never `-0` in render).
                match crate::value::decimal_neg(unscaled, scale) {
                    Ok(v) => Ok(v),
                    Err(msg) => rt(msg),
                }
            }
            (UnaryOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
            (UnaryOp::BitNot, Value::Int(n)) => Ok(Value::Int(crate::value::int_bitnot(n))),
            (op, v) => rt(format!("cannot apply {op:?} to {}", v.type_name())),
        }
    }

    pub(super) fn eval_binary(&mut self, op: BinaryOp, lhs: &Expr, rhs: &Expr) -> R<Value> {
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
            Add | Sub | Mul | Pow | Div | Rem => arith(op, l, r),
            BitAnd | BitOr | BitXor | Shl | Shr => bitwise(op, l, r),
            Eq => Ok(Value::Bool(l.eq_val(&r))),
            NotEq => Ok(Value::Bool(!l.eq_val(&r))),
            Lt | Gt | Le | Ge => compare(op, l, r),
            Pipe => unreachable!("`|>` is lowered to a call in the parser"),
            And | Or | Coalesce => unreachable!("handled above"),
        }
    }

    pub(super) fn eval_match(&mut self, scrutinee: &Expr, arms: &[MatchArm]) -> R<Value> {
        let value = self.eval(scrutinee)?;
        for arm in arms {
            let mut bindings = Vec::new();
            if match_pattern(&arm.pattern, &value, &self.class_implements, &mut bindings) {
                self.frame.push_scope();
                for (n, v) in bindings {
                    self.frame.declare(&n, v);
                }
                // An arm guard runs with the pattern's bindings in scope; a false guard falls
                // through to the next arm (discarding this arm's bindings first).
                if let Some(g) = &arm.guard {
                    match self.eval(g).and_then(|v| as_bool(&v)) {
                        Ok(true) => {}
                        Ok(false) => {
                            self.frame.pop_scope();
                            continue;
                        }
                        Err(e) => {
                            self.frame.pop_scope();
                            return Err(e);
                        }
                    }
                }
                let r = self.eval(&arm.body);
                self.frame.pop_scope();
                return r;
            }
        }
        rt("non-exhaustive match at runtime")
    }
}
