//! Tree-walking interpreter — stmt (M-Decomp W4). See mod.rs for the struct + core + entry points.

use super::*;

impl<'c> Interp<'c> {
    pub(super) fn exec_stmt(&mut self, s: &Stmt) -> R<()> {
        // Track the current source line on the active trace frame, so a fault reports the right line
        // (and a non-top frame reports its call-site line — the call statement currently executing).
        let line = stmt_line(s);
        if let Some(fr) = self.trace_stack.last_mut() {
            fr.line = line;
        }
        // M-DX S5: an attached interactive debugger pauses before a statement. The hot path is a cheap
        // `Option` check; the pause machinery is a `#[cold]` helper so it never bloats this recursive
        // frame (same discipline as S3's dump capture).
        if self
            .debug
            .as_ref()
            .is_some_and(|d| d.should_pause(line, self.depth))
        {
            self.debug_pause(line);
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
                // Value-type element set — flat `xs[i]=e` (M-mut.5) or NESTED `g[i][j]=e` (Spec
                // nested-value-index). Flatten the place into (root local, index path j0..jk), eval the
                // indices left-to-right then the value, then mutate the root **in its slot** via the
                // shared `value::set_nested` kernel (COW `Rc::make_mut` root-to-leaf, O(1) per level in
                // the common case — same single-source the VM calls, so `run ≡ runvm`). Looking the slot
                // up mutably keeps the outer refcount at 1 (M-DOGFOOD W8, generalized).
                Expr::Index { .. } => {
                    // Collect index expressions innermost-`Index`-first, then reverse to source order.
                    let mut chain: Vec<&Expr> = Vec::new();
                    let mut cur = target;
                    let root = loop {
                        match cur {
                            Expr::Index { object, index, .. } => {
                                chain.push(index);
                                cur = object;
                            }
                            Expr::Ident(n, _) => break n,
                            _ => unreachable!("checker restricts the index-assign base to a local"),
                        }
                    };
                    chain.reverse();
                    let mut idx_vals: Vec<Value> = Vec::with_capacity(chain.len());
                    for e in chain {
                        idx_vals.push(self.eval(e)?);
                    }
                    let new_val = self.eval(value)?;
                    let slot = self.frame.lookup_mut(root).ok_or_else(|| {
                        Signal::Runtime(Diagnostic::runtime(format!("undefined variable `{root}`")))
                    })?;
                    crate::value::set_nested(slot, &idx_vals, new_val)
                        .map_err(|m| Signal::Runtime(Diagnostic::runtime(m)))?;
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
                                let cls = inst.class.clone();
                                self.run_call(
                                    &setter,
                                    &[p.name],
                                    &body,
                                    vec![v],
                                    Some(Value::Instance(inst)),
                                    Some(cls.as_str()),
                                )?;
                                return Ok(());
                            }
                            inst.set_field(name, v);
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
                name,
                val,
                iter,
                body,
                ..
            } => {
                // B1 iteration protocol: normalize the source (`List`/`Set`/`string`/`Map`) to its
                // element sequence via the shared `value::iter_elements` kernel (byte-identical order
                // to the VM's `Op::IterElems`). A `Map` yields `[key, value]` 2-lists.
                let iterable = self.eval(iter)?;
                let items = match crate::value::iter_elements(&iterable) {
                    Ok(v) => v,
                    Err(e) => return rt(e),
                };
                for item in items.iter() {
                    self.frame.push_scope();
                    match val {
                        // Two-binding Map form: `item` is a `[key, value]` list — unpack both.
                        Some((_, vname)) => {
                            if let Value::List(pair) = item {
                                self.frame.declare(name, pair[0].clone());
                                self.frame.declare(vname, pair[1].clone());
                            }
                        }
                        None => self.frame.declare(name, item.clone()),
                    }
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
            Stmt::Expr(e, _) | Stmt::Discard(e, _) => {
                self.eval(e)?;
                Ok(())
            }
            // `throw e;` — evaluate the exception value and unwind as a `Throw` signal (M-faults 2b).
            Stmt::Throw { value, .. } => {
                let v = self.eval(value)?;
                Err(Signal::Throw(v))
            }
            // Let-destructuring (Phase 1 slice 5): bind each binder into the current scope. A struct
            // pattern reads the instance's fields; a list pattern length-checks and, on a mismatch,
            // runs the (diverging) `else` — propagating its signal.
            Stmt::Destructure {
                pat,
                init,
                else_block,
                ..
            } => {
                let v = self.eval(init)?;
                self.exec_destructure(pat, v, else_block.as_deref())
            }
            // `try { … } catch (T e) { … } … [finally { … }]` — native unwinding (M-faults 2b).
            // The body runs; a `Throw` it raises is matched against the catch clauses in order (a
            // catch whose type — or any union member — is the value's class or a supertype). A
            // `Runtime` fault (panic/index-OOB) is NOT a `Throw`, so it passes straight through every
            // catch. `finally` runs on *every* exit edge (normal, caught, re-thrown, or a
            // return/break/continue escaping the body) and its own signal, if any, takes precedence.
            Stmt::Try {
                body,
                catches,
                finally_block,
                ..
            } => {
                let outcome = match self.exec_scoped(body) {
                    Err(Signal::Throw(v)) => match self.match_catch(catches, &v) {
                        Some(clause) => {
                            self.frame.push_scope();
                            self.frame.declare(&clause.name, v);
                            let r = self.exec_stmts(&clause.body);
                            self.frame.pop_scope();
                            r
                        }
                        None => Err(Signal::Throw(v)), // no clause matched — re-propagate
                    },
                    // Ok, a fault, or a return/break/continue all flow to `finally` unchanged.
                    other => other,
                };
                if let Some(fb) = finally_block {
                    // A `finally` that itself diverges (return/throw/break/continue/fault) overrides
                    // the body/catch outcome; a normal `finally` lets `outcome` propagate.
                    self.exec_scoped(fb)?;
                }
                outcome
            }
        }
    }

    /// Find the first `catch` clause matching a thrown value `v` (M-faults 2b): a clause whose type
    /// — or, for a union `catch (A | B e)`, any member — is `v`'s class or a supertype of it (the
    /// shared `instanceof` oracle). Returns the clause to run, or `None` to re-propagate the throw.
    pub(super) fn match_catch<'a>(
        &self,
        catches: &'a [crate::ast::CatchClause],
        v: &Value,
    ) -> Option<&'a crate::ast::CatchClause> {
        catches.iter().find(|c| {
            catch_type_names(&c.ty)
                .iter()
                .any(|n| self.value_is_a(v, n))
        })
    }

    /// Whether `v` is an instance of (or a subtype of) the type named `name` — the same test as
    /// `instanceof`: an exact class match or `name` is an interface the class implements.
    pub(super) fn value_is_a(&self, v: &Value, name: &str) -> bool {
        matches!(v, Value::Instance(inst)
            if inst.class == *name
                || self
                    .class_implements
                    .get(&inst.class)
                    .is_some_and(|ifaces| ifaces.iter().any(|i| i == name)))
    }

    /// Bind a [`Stmt::Destructure`] (Phase 1 slice 5) into the current scope. A struct pattern reads
    /// the instance's fields by name; a list pattern length-checks against the binder count and, on a
    /// mismatch, runs the `else` block whose `Signal` (a guaranteed divergence) is propagated. Both
    /// error paths are checker-unreachable (the checker proves the value's shape) and only defensive.
    fn exec_destructure(
        &mut self,
        pat: &crate::ast::DestructurePat,
        v: Value,
        else_block: Option<&[Stmt]>,
    ) -> R<()> {
        use crate::ast::DestructurePat;
        match pat {
            DestructurePat::Struct {
                type_name, fields, ..
            } => match v {
                Value::Instance(inst) => {
                    for f in fields {
                        let fv = inst.get_field(&f.field);
                        match fv {
                            Some(val) => self.frame.declare(&f.binding, val),
                            None => {
                                return rt(format!("no field `{}` on `{}`", f.field, inst.class))
                            }
                        }
                    }
                    Ok(())
                }
                other => rt(format!(
                    "cannot destructure {} as `{type_name}`",
                    other.type_name()
                )),
            },
            DestructurePat::List { binders, .. } => match v {
                Value::List(items) => {
                    if items.len() != binders.len() {
                        // Refutable mismatch → run the (diverging) `else`; propagate its signal.
                        if let Some(eb) = else_block {
                            return self.exec_scoped(eb);
                        }
                        return rt(format!(
                            "list destructuring expected {} element(s), found {}",
                            binders.len(),
                            items.len()
                        ));
                    }
                    for ((name, _), item) in binders.iter().zip(items.iter()) {
                        self.frame.declare(name, item.clone());
                    }
                    Ok(())
                }
                other => rt(format!("cannot list-destructure {}", other.type_name())),
            },
        }
    }

    /// `while (cond) { .. }` / `do { .. } while (cond);` (M-mut.3). Each iteration runs the body in
    /// its own scope; a `Signal::Break` stops the loop, `Continue` proceeds to the next test, both
    /// consumed here. while-let is desugared by the parser, so it arrives as a plain `while (true)`.
    pub(super) fn exec_while(&mut self, cond: &Expr, body: &[Stmt], post_cond: bool) -> R<()> {
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
    pub(super) fn exec_cfor(
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

    pub(super) fn cfor_loop(
        &mut self,
        cond: Option<&Expr>,
        step: Option<&Stmt>,
        body: &[Stmt],
    ) -> R<()> {
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
}
