//! Tree-walking interpreter — call (M-Decomp W4). See mod.rs for the struct + core + entry points.

use super::*;

impl Interp {
    pub(super) fn eval_call(&mut self, callee: &Expr, args: &[Expr]) -> R<Value> {
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
                                            match self.call_closure(rc.clone(), cargs) {
                                                Ok(v) => Ok(v),
                                                // A `throw` inside a closure passed to the native
                                                // can't cross the `Result<_, String>` boundary as a
                                                // value — stash it and signal via the sentinel, then
                                                // rebuild the `Throw` once the native returns.
                                                Err(Signal::Throw(v)) => {
                                                    self.pending_throw = Some(v);
                                                    Err(THROW_SENTINEL.to_string())
                                                }
                                                Err(other) => Err(signal_msg(other)),
                                            }
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
                                Err(msg) => {
                                    // Reconstruct a throw that unwound out of a higher-order native.
                                    if msg == THROW_SENTINEL {
                                        if let Some(v) = self.pending_throw.take() {
                                            return Err(Signal::Throw(v));
                                        }
                                    }
                                    rt(msg)
                                }
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
            if let Some(set) = self.funcs.get(name).cloned() {
                let f = self.select_free_overload(name, &set, &argv)?;
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

    /// Select the overload of free function `name` to run for `argv` (M-RT dynamic dispatch). A
    /// single-overload set is returned directly; otherwise the most-specific match by the runtime
    /// argument types wins. The same selection runs in the VM (`dispatch::select_overload` over the
    /// same `ParamKind`s), so `run`/`runvm` pick the same body. An ambiguous or unmatched call faults
    /// with a byte-identical message.
    pub(super) fn select_free_overload(
        &self,
        name: &str,
        set: &[FunctionDecl],
        argv: &[Value],
    ) -> R<FunctionDecl> {
        if set.len() == 1 {
            return Ok(set[0].clone());
        }
        let candidates: Vec<Vec<crate::dispatch::ParamKind>> = set
            .iter()
            .map(|f| {
                f.params
                    .iter()
                    .map(|p| crate::dispatch::param_kind(&p.ty))
                    .collect()
            })
            .collect();
        match crate::dispatch::select_overload(&candidates, argv, &self.class_implements) {
            Ok(i) => Ok(set[i].clone()),
            Err(crate::dispatch::SelectErr::Ambiguous) => {
                rt(format!("ambiguous overloaded call to `{name}`"))
            }
            Err(crate::dispatch::SelectErr::NoMatch) => rt(format!(
                "no overload of `{name}` matches the argument types"
            )),
        }
    }

    /// Execute a closure value with the supplied arguments.
    pub(super) fn call_closure(&mut self, closure: Rc<ClosureData>, args: Vec<Value>) -> R<Value> {
        match &*closure {
            ClosureData::Tree {
                params,
                body,
                env,
                this_capture,
                ..
            } => {
                if args.len() != params.len() {
                    return rt(format!(
                        "lambda expects {} arg(s), got {}",
                        params.len(),
                        args.len()
                    ));
                }
                self.call_tree_closure(params, body, env, this_capture.clone(), args)
            }
            ClosureData::Named(name) => {
                // A first-class named-function value never refers to an overloaded function
                // (`E-OVERLOAD-FN-VALUE`), so the set has exactly one member.
                let f = match self.funcs.get(name).and_then(|v| v.first()).cloned() {
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
    /// runs the body, then restores the frame. `this` is the closure's captured receiver
    /// (`this_capture`) — `None` unless the lambda referenced `this` (Phase 1 closures slice).
    pub(super) fn call_tree_closure(
        &mut self,
        params: &[Param],
        body: &LambdaBody,
        env: &[(String, Value)],
        this_capture: Option<Value>,
        args: Vec<Value>,
    ) -> R<Value> {
        if self.depth >= crate::limits::MAX_CALL_DEPTH {
            return rt("stack overflow");
        }
        self.depth += 1;
        let saved_frame = std::mem::replace(&mut self.frame, CallScopes::new());
        // Restore the captured receiver (if any) as `this` for the duration of the body.
        let saved_this = std::mem::replace(&mut self.this, this_capture);
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

    pub(super) fn eval_args(&mut self, args: &[Expr]) -> R<Vec<Value>> {
        let mut out = Vec::with_capacity(args.len());
        for a in args {
            out.push(self.eval(a)?);
        }
        Ok(out)
    }

    pub(super) fn call_method(&mut self, recv: Value, name: &str, args: Vec<Value>) -> R<Value> {
        let inst = match recv {
            Value::Instance(inst) => inst,
            other => return rt(format!("cannot call `.{name}()` on {}", other.type_name())),
        };
        // M-RT S6/S6b: resolve the method through the shared dispatch table, which maps `(class, name)`
        // to the `(declaring_class, method)` it runs — already accounting for override, multi-parent
        // composition, diamond auto-merge, and `use`/`rename`/`exclude` resolution clauses. The
        // compiler pre-flattens the identical table into the VM's `methods` table, so `run`/`runvm`
        // dispatch to the same body. The candidates are that origin class's overloads of the resolved
        // method name (which differs from `name` only for a renamed alias).
        let candidates: Vec<FunctionDecl> = {
            let key = (inst.class.clone(), name.to_string());
            match self.method_origins.get(&key) {
                Some((origin_class, origin_method)) => self
                    .classes
                    .get(origin_class)
                    .map(|class| {
                        class
                            .members
                            .iter()
                            .filter_map(|m| match m {
                                ClassMember::Method(f) if &f.name == origin_method => {
                                    Some(f.clone())
                                }
                                _ => None,
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                None => Vec::new(),
            }
        };
        let f = match candidates.len() {
            0 => return rt(format!("no method `{name}` on `{}`", inst.class)),
            1 => candidates[0].clone(),
            _ => {
                let kinds: Vec<Vec<crate::dispatch::ParamKind>> = candidates
                    .iter()
                    .map(|f| {
                        f.params
                            .iter()
                            .map(|p| crate::dispatch::param_kind(&p.ty))
                            .collect()
                    })
                    .collect();
                match crate::dispatch::select_overload(&kinds, &args, &self.class_implements) {
                    Ok(i) => candidates[i].clone(),
                    Err(crate::dispatch::SelectErr::Ambiguous) => {
                        return rt(format!("ambiguous overloaded call to `{name}`"))
                    }
                    Err(crate::dispatch::SelectErr::NoMatch) => {
                        return rt(format!(
                            "no overload of `{name}` matches the argument types"
                        ))
                    }
                }
            }
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
}
