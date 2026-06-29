//! Bytecode compiler — stmt (M-Decomp W4.1). See compiler/mod.rs for the struct,
//! emission/scope core, and the (kept-whole) `stack_effect`.

use super::*;

impl Compiler<'_> {
    pub(super) fn stmt(&mut self, s: &Stmt) -> Result<(), String> {
        // Every statement begins with a clean operand stack (transients == 0), so the live operand
        // height equals the live-locals count. Anchoring here keeps `match`'s scrutinee slot exact
        // regardless of any height drift in preceding dead-code-after-`return`.
        self.height = self.locals.len();
        match s {
            Stmt::VarDecl { ty, name, init, .. } => {
                self.expr(init)?; // value stays on the stack as the new local's slot
                                  // `var` carries no annotation — derive the local's `CTy` from the initializer so
                                  // later arithmetic on it still specializes (AddI/AddF). `ctype` is total over
                                  // checker-valid initializers here; fall back to `Other` defensively so a `var`
                                  // never makes a program the interpreter accepts fail to compile (parity spine).
                let local_ty = match ty {
                    Type::Infer(_) => self.ctype(init).unwrap_or(CTy::Other),
                    _ => resolve_cty(ty),
                };
                self.add_local(name, local_ty);
                Ok(())
            }
            Stmt::Assign {
                target,
                value,
                span,
            } => match target {
                // Local reassignment reuses `Op::SetLocal` — no new Op (M-mut.1). The checker
                // guarantees the target is a `mutable` in-scope local, so the slot always resolves.
                Expr::Ident(name, _) => {
                    let slot = self
                        .resolve_local(name)
                        .ok_or_else(|| format!("unresolved local in assignment: {name}"))?;
                    self.expr(value)?; // push the new value
                    self.emit(Op::SetLocal(slot), span.line); // set-and-pop into the existing slot
                    Ok(())
                }
                // Value-type element set `xs[i] = e` / `m[k] = e` (M-mut.5). The container is a
                // mutable local (checker-enforced); load it, push index + value, `SetIndex` (COW),
                // then store the resulting container back. Nested places (`a[i][j]`, `this.f[i]`)
                // are a later slice — the checker rejects a non-Ident container as `E-ASSIGN-TARGET`.
                Expr::Index { object, index, .. } => {
                    let name = match &**object {
                        Expr::Ident(n, _) => n,
                        _ => unreachable!("checker restricts index-assign to a local container"),
                    };
                    let slot = self
                        .resolve_local(name)
                        .ok_or_else(|| format!("unresolved local in index-assignment: {name}"))?;
                    self.emit(Op::GetLocal(slot), span.line); // [container]
                    self.expr(index)?; // [container, index]
                    self.expr(value)?; // [container, index, value]
                    self.emit(Op::SetIndex, span.line); // [newcontainer]
                    self.emit(Op::SetLocal(slot), span.line); // write back
                    Ok(())
                }
                // Static write `ClassName.field = e` (M-mut.7): push the value, store into the
                // program-level static slot. Checked first — the head is a class name, not a local.
                Expr::Member { object, name, .. } if self.static_slot(object, name).is_some() => {
                    let idx = self.static_slot(object, name).unwrap();
                    self.expr(value)?; // [value]
                    self.emit(Op::SetStatic(idx), span.line); // pop into the static slot
                    Ok(())
                }
                // Property hook write `o.name = e` (M-mut.7b) → call the synthetic `<name>$set`
                // 1-arg method with the receiver + value; it runs the set block and returns `Unit`,
                // which we discard. Resolved before the plain field path.
                Expr::Member { object, name, .. }
                    if self.hook_set_method(object, name).is_some() =>
                {
                    let setm = self.hook_set_method(object, name).unwrap();
                    self.expr(object)?; // [instance]
                    self.expr(value)?; // [instance, value]
                    let idx = self.field_name_index(&setm)?;
                    self.emit(Op::CallMethod(idx, 1), span.line); // [unit result]
                    self.emit(Op::Pop, span.line); // discard the set's return value
                    Ok(())
                }
                // Shared-mutable instance field set `o.f = e` / `this.f = e` (M-mut.6). Evaluate the
                // object then the value (interpreter eval order), then `SetField` mutates the shared
                // `Rc<Instance>` cell in place and pops both. The field is checker-guaranteed `mutable`.
                Expr::Member { object, name, .. } => {
                    self.expr(object)?; // [instance]
                    self.expr(value)?; // [instance, value]
                    let idx = self.field_name_index(name)?;
                    self.emit(Op::SetField(idx), span.line); // mutate in place, pop both
                    Ok(())
                }
                _ => unreachable!("checker rejects other assignment targets"),
            },
            Stmt::Expr(e, span) | Stmt::Discard(e, span) => {
                self.expr(e)?;
                self.emit(Op::Pop, span.line);
                Ok(())
            }
            Stmt::Return { value, span } => {
                // Inside a synthetic constructor body, a `return` does not yield the body's value:
                // the interpreter discards it and always returns the promoted instance
                // (`construct`). So evaluate any operand for its side effects, drop it, and jump to
                // the ctor epilogue (which loads + returns the instance). The checker pins a ctor
                // body's return type to `Unit`, so `value` is `None` or a unit-typed expression.
                if self.ctor_return_jumps.is_some() {
                    if let Some(e) = value {
                        self.expr(e)?;
                        self.emit(Op::Pop, span.line);
                    }
                    let j = self.emit_jump(Op::Jump(0), span.line);
                    self.ctor_return_jumps
                        .as_mut()
                        .expect("ctor_return_jumps is Some")
                        .push(j);
                    return Ok(());
                }
                match value {
                    Some(e) => self.expr(e)?,
                    None => self.emit_const(Value::Unit, span.line),
                }
                // A `return` exits every enclosing `try` (M-faults 2b): pop their handlers and run
                // their `finally` blocks first. Because a `finally` may evaluate (and the handler ops
                // must not perturb the value), spill the return value to a temp local so finally runs
                // on a clean operand stack, then reload it. No-op when no `try` is active.
                if self.finally_stack.is_empty() {
                    self.emit(Op::Return, span.line);
                } else {
                    let tmp = self.add_local("$ret", CTy::Other);
                    self.height = self.locals.len();
                    let n = self.finally_stack.len();
                    self.emit_finally_for_exit(n, span.line)?;
                    self.emit(Op::GetLocal(tmp), span.line);
                    self.emit(Op::Return, span.line);
                    self.locals.pop(); // unregister `$ret` (dead code follows a `return`)
                }
                Ok(())
            }
            Stmt::Block(stmts, span) => {
                self.begin_scope();
                for st in stmts {
                    self.stmt(st)?;
                }
                self.end_scope(span.line);
                Ok(())
            }
            Stmt::If {
                cond,
                bind,
                then_block,
                else_block,
                span,
            } => self.compile_if(
                cond,
                bind.as_deref(),
                then_block,
                else_block.as_deref(),
                span.line,
            ),
            Stmt::For {
                ty,
                name,
                iter,
                body,
                span,
            } => self.compile_for(name, resolve_cty(ty), iter, body, span.line),
            Stmt::While {
                cond,
                body,
                post_cond,
                span,
            } => self.compile_while(cond, body, *post_cond, span.line),
            Stmt::CFor {
                init,
                cond,
                step,
                body,
                span,
            } => self.compile_cfor(
                init.as_deref(),
                cond.as_ref(),
                step.as_deref(),
                body,
                span.line,
            ),
            Stmt::Break(span) => self.compile_break_continue(true, span.line),
            Stmt::Continue(span) => self.compile_break_continue(false, span.line),
            // `throw e;` — evaluate the exception and emit `Op::Throw` (M-faults 2b).
            Stmt::Throw { value, span } => {
                self.expr(value)?;
                self.emit(Op::Throw, span.line);
                Ok(())
            }
            Stmt::Try {
                body,
                catches,
                finally_block,
                span,
            } => self.compile_try(body, catches, finally_block.as_deref(), span.line),
            Stmt::Destructure {
                pat,
                init,
                else_block,
                span,
            } => self.compile_destructure(pat, init, else_block.as_deref(), span.line),
        }
    }

    /// Let-destructuring (Phase 1 slice 5) — NO new `Op`. Spill the init to a hidden `$destructure`
    /// local, then: a **struct** pattern reads each field (`GetField`) into a fresh binder
    /// (irrefutable, no branch); a **list** pattern reserves the binder slots, length-checks
    /// (`Len`/`Eq`/`JumpIfFalse`), assigns each element on the success path (the bounds-checked
    /// `Index`), and runs the diverging `else` on the fail path — structurally the same lowering as an
    /// `if`. Reserving the binder slots up front keeps the locals layout identical on both branches,
    /// so the continuation (where the binders are live) needs no save/restore.
    pub(super) fn compile_destructure(
        &mut self,
        pat: &crate::ast::DestructurePat,
        init: &Expr,
        else_block: Option<&[Stmt]>,
        line: u32,
    ) -> Result<(), String> {
        use crate::ast::DestructurePat;
        self.expr(init)?; // [.., initval] — spilled to the hidden temp below
        let init_cty = self.ctype(init).unwrap_or(CTy::Other);
        let d = self.add_local("$destructure", init_cty.clone());
        match pat {
            DestructurePat::Struct {
                type_name, fields, ..
            } => {
                for f in fields {
                    self.emit(Op::GetLocal(d), line); // [.., instance]
                    let idx = self.field_name_index(&f.field)?;
                    self.emit(Op::GetField(idx), line); // [.., fieldval]
                                                        // The binder's CTy is the class-field type so a destructured int is a first-class
                                                        // arithmetic operand on the VM (`Point { x } ⇒ x + 1`), the operand trap.
                    let cty = self
                        .class_field_ctys
                        .get(type_name)
                        .and_then(|m| m.get(&f.field))
                        .cloned()
                        .unwrap_or(CTy::Other);
                    self.add_local(&f.binding, cty); // fieldval becomes the binder
                }
                Ok(())
            }
            DestructurePat::List { binders, .. } => {
                let elem_cty = match &init_cty {
                    CTy::List(e) => (**e).clone(),
                    _ => CTy::Other,
                };
                let arity = binders.len();
                // Reserve every binder slot (null placeholder) BEFORE the branch so the locals layout
                // is identical on the success and `else` paths.
                for (name, _) in binders {
                    self.emit_const(Value::Null, line);
                    self.add_local(name, elem_cty.clone());
                }
                // len(init) == arity ?
                self.emit(Op::GetLocal(d), line);
                self.emit(Op::Len, line);
                self.emit_const(Value::Int(arity as i64), line);
                self.emit(Op::Eq, line);
                let else_jump = self.emit_jump(Op::JumpIfFalse(0), line); // jump to `else` if length differs
                                                                          // Success: assign each element into its reserved slot (the `Index` is in-range here).
                for (i, (name, _)) in binders.iter().enumerate() {
                    let slot = self
                        .resolve_local(name)
                        .ok_or_else(|| format!("unresolved destructure binder: {name}"))?;
                    self.emit(Op::GetLocal(d), line);
                    self.emit_const(Value::Int(i as i64), line);
                    self.emit(Op::Index, line);
                    self.emit(Op::SetLocal(slot), line);
                }
                let end_jump = self.emit_jump(Op::Jump(0), line);
                self.patch_jump(else_jump);
                // The `else` block (checker-guaranteed to diverge) runs with the binders reserved as
                // null — harmless, the checker forbids referencing them here.
                if let Some(eb) = else_block {
                    self.begin_scope();
                    for s in eb {
                        self.stmt(s)?;
                    }
                    self.end_scope(line);
                }
                self.patch_jump(end_jump);
                Ok(())
            }
        }
    }

    pub(super) fn compile_if(
        &mut self,
        cond: &Expr,
        bind: Option<&str>,
        then_block: &[Stmt],
        else_block: Option<&[Stmt]>,
        line: u32,
    ) -> Result<(), String> {
        if let Some(name) = bind {
            return self.compile_if_let(name, cond, then_block, else_block, line);
        }
        self.expr(cond)?;
        let else_jump = self.emit_jump(Op::JumpIfFalse(0), line); // pops cond
        self.begin_scope();
        for s in then_block {
            self.stmt(s)?;
        }
        self.end_scope(line);
        let end_jump = self.emit_jump(Op::Jump(0), line);
        self.patch_jump(else_jump);
        if let Some(eb) = else_block {
            self.begin_scope();
            for s in eb {
                self.stmt(s)?;
            }
            self.end_scope(line);
        }
        self.patch_jump(end_jump);
        Ok(())
    }

    /// `if (var name = cond)` (M3 S2.4). The scrutinee value lands in a scoped local that *is* the
    /// binding `name` (its `CTy` is the optional's inner type so `name + 1` still specializes); a
    /// non-consuming null-test (`GetLocal; Const null; Ne`) selects the branch. No new `Op` — the
    /// scrutinee slot persists across both arms and is popped by the enclosing `end_scope`. The
    /// checker forbids referencing `name` in the else block, so leaving it registered is harmless.
    pub(super) fn compile_if_let(
        &mut self,
        name: &str,
        cond: &Expr,
        then_block: &[Stmt],
        else_block: Option<&[Stmt]>,
        line: u32,
    ) -> Result<(), String> {
        self.begin_scope();
        self.expr(cond)?; // [opt] — this slot becomes the binding `name`
        let inner_cty = self.ctype(cond).unwrap_or(CTy::Other);
        let slot = self.add_local(name, inner_cty);
        self.emit(Op::GetLocal(slot), line); // [opt, opt]
        self.emit_const(Value::Null, line); // [opt, opt, null]
        self.emit(Op::Ne, line); // [opt, opt != null]
        let else_jump = self.emit_jump(Op::JumpIfFalse(0), line); // pops bool → [opt]; jump if null
        self.begin_scope();
        for s in then_block {
            self.stmt(s)?;
        }
        self.end_scope(line);
        let end_jump = self.emit_jump(Op::Jump(0), line);
        self.patch_jump(else_jump);
        if let Some(eb) = else_block {
            self.begin_scope();
            for s in eb {
                self.stmt(s)?;
            }
            self.end_scope(line);
        }
        self.patch_jump(end_jump);
        self.end_scope(line); // pops the scrutinee slot (`name`) — both arms converge with [opt] live
        Ok(())
    }

    /// `for (T name in iter)` desugars to a counter loop over an inline list
    /// (decision P2-7). Hidden locals `$for_list` and `$for_idx` bracket `name`.
    pub(super) fn compile_for(
        &mut self,
        name: &str,
        elem_ty: CTy,
        iter: &Expr,
        body: &[Stmt],
        line: u32,
    ) -> Result<(), String> {
        self.begin_scope();
        self.expr(iter)?; // [iterable]
                          // B1 iteration protocol: normalize the source collection (`List`/`Set`) to a plain element
                          // list once, so the loop body below indexes a list regardless of the source kind. Shared with
                          // the interpreter via `value::iter_elements` ⇒ byte-identical iteration order.
        self.emit(Op::IterElems, line); // [list]
        let s_list = self.add_local("$for_list", CTy::Other);
        self.emit_const(Value::Int(0), line); // [list, 0]
        let s_idx = self.add_local("$for_idx", CTy::Int);

        // break/continue pop down to here (just `$for_list`+`$for_idx` live): a `break` lands at the
        // exit (where `end_scope` drops those two), a `continue` at the index-increment — both after
        // the loop variable has been dropped, so the pop count covers the loop var + any body locals.
        let body_base = self.locals.len();
        let loop_start = self.here();
        self.emit(Op::GetLocal(s_idx), line);
        self.emit(Op::GetLocal(s_list), line);
        self.emit(Op::Len, line); // [idx, len]
        self.emit(Op::Lt, line); // [idx < len]
        let exit_jump = self.emit_jump(Op::JumpIfFalse(0), line);

        self.emit(Op::GetLocal(s_list), line);
        self.emit(Op::GetLocal(s_idx), line);
        self.emit(Op::Index, line); // [elem]
        self.add_local(name, elem_ty); // elem becomes the loop variable

        self.loop_frames.push(LoopFrame {
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            body_base,
        });
        self.begin_scope(); // body's own locals get cleaned each iteration
        for s in body {
            self.stmt(s)?;
        }
        self.end_scope(line);
        let frame = self.loop_frames.pop().expect("for loop frame");

        self.emit(Op::Pop, line); // drop the loop variable
        self.locals.pop(); // unregister `name`

        // idx = idx + 1 — also the `continue` target (loop var already dropped above).
        let cont_target = self.here();
        for j in &frame.continue_jumps {
            self.patch_jump_to(*j, cont_target);
        }
        self.emit(Op::GetLocal(s_idx), line);
        self.emit_const(Value::Int(1), line);
        self.emit(Op::AddI, line);
        self.emit(Op::SetLocal(s_idx), line);
        self.emit(Op::Jump(loop_start), line);

        self.patch_jump(exit_jump);
        for j in &frame.break_jumps {
            self.patch_jump(*j);
        }
        self.end_scope(line); // pops $for_idx, $for_list
        Ok(())
    }

    /// `while (cond) { .. }` / `do { .. } while (cond);` (M-mut.3). Lowers to a `JumpIfFalse`-guarded
    /// back-edge — no new loop opcode (F5). A loop frame collects `break`/`continue` jumps; `break`
    /// targets the exit, `continue` the condition re-test (the loop top for `while`, the bottom test
    /// for `do-while`). `body_base` is the locals depth both forms pop down to.
    pub(super) fn compile_while(
        &mut self,
        cond: &Expr,
        body: &[Stmt],
        post_cond: bool,
        line: u32,
    ) -> Result<(), String> {
        self.begin_scope();
        let body_base = self.locals.len();
        if post_cond {
            // do-while: body first, condition (and the continue target) at the bottom.
            let loop_start = self.here();
            self.loop_frames.push(LoopFrame {
                break_jumps: Vec::new(),
                continue_jumps: Vec::new(),
                body_base,
            });
            self.begin_scope();
            for s in body {
                self.stmt(s)?;
            }
            self.end_scope(line);
            let frame = self.loop_frames.pop().expect("do-while loop frame");

            let cont_target = self.here();
            for j in &frame.continue_jumps {
                self.patch_jump_to(*j, cont_target);
            }
            self.expr(cond)?; // [cond]
            let exit_jump = self.emit_jump(Op::JumpIfFalse(0), line); // false → exit
            self.emit(Op::Jump(loop_start), line); // true → loop again
            self.patch_jump(exit_jump);
            for j in &frame.break_jumps {
                self.patch_jump(*j);
            }
        } else {
            // while: condition (and the continue target) at the top.
            let loop_start = self.here();
            self.expr(cond)?; // [cond]
            let exit_jump = self.emit_jump(Op::JumpIfFalse(0), line); // pops cond
            self.loop_frames.push(LoopFrame {
                break_jumps: Vec::new(),
                continue_jumps: Vec::new(),
                body_base,
            });
            self.begin_scope();
            for s in body {
                self.stmt(s)?;
            }
            self.end_scope(line);
            let frame = self.loop_frames.pop().expect("while loop frame");

            self.emit(Op::Jump(loop_start), line);
            self.patch_jump(exit_jump);
            for j in &frame.break_jumps {
                self.patch_jump(*j);
            }
            for j in &frame.continue_jumps {
                self.patch_jump_to(*j, loop_start);
            }
        }
        self.end_scope(line);
        Ok(())
    }

    /// C-style `for (init; cond; step) { body }` (M-mut.3). `init`'s binding lives in the loop's own
    /// scope; `continue` jumps to `step`, `break` to the exit. Same jump back-edge as `compile_while`.
    pub(super) fn compile_cfor(
        &mut self,
        init: Option<&Stmt>,
        cond: Option<&Expr>,
        step: Option<&Stmt>,
        body: &[Stmt],
        line: u32,
    ) -> Result<(), String> {
        self.begin_scope();
        if let Some(s) = init {
            self.stmt(s)?;
        }
        // break/continue pop down to here (init's local stays live; the exit's `end_scope` drops it).
        let body_base = self.locals.len();
        let loop_start = self.here();
        let exit_jump = if let Some(c) = cond {
            self.expr(c)?;
            Some(self.emit_jump(Op::JumpIfFalse(0), line))
        } else {
            None // `for (init;;step)` — no condition, loop until `break`
        };
        self.loop_frames.push(LoopFrame {
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            body_base,
        });
        self.begin_scope();
        for s in body {
            self.stmt(s)?;
        }
        self.end_scope(line);
        let frame = self.loop_frames.pop().expect("c-for loop frame");

        // `continue` lands at the step (run the step, then re-test).
        let cont_target = self.here();
        for j in &frame.continue_jumps {
            self.patch_jump_to(*j, cont_target);
        }
        if let Some(s) = step {
            self.stmt(s)?;
        }
        self.emit(Op::Jump(loop_start), line);

        if let Some(e) = exit_jump {
            self.patch_jump(e);
        }
        for j in &frame.break_jumps {
            self.patch_jump(*j);
        }
        self.end_scope(line); // pops init's local
        Ok(())
    }

    /// Emit a `break` (`is_break`) or `continue`: pop every body-scope local back down to the
    /// innermost loop's `body_base`, then a placeholder `Jump` recorded in the loop frame for the
    /// loop to patch (exit for `break`, continue-target for `continue`). No new `Op` (F5). The
    /// checker rejects break/continue outside a loop, so the frame is always present.
    pub(super) fn compile_break_continue(
        &mut self,
        is_break: bool,
        line: u32,
    ) -> Result<(), String> {
        let body_base = self
            .loop_frames
            .last()
            .map(|f| f.body_base)
            .ok_or("`break`/`continue` outside a loop")?;
        // Run the `finally` (+ `PopHandler`) of every `try` entered *inside* this (innermost) loop,
        // before unwinding the loop-body locals (M-faults 2b). A `try` outside the loop is not exited.
        let cur_loop_depth = self.loop_frames.len();
        let n_exit = self
            .finally_stack
            .iter()
            .rev()
            .take_while(|c| c.loop_depth >= cur_loop_depth)
            .count();
        self.emit_finally_for_exit(n_exit, line)?;
        for _ in 0..(self.locals.len() - body_base) {
            self.emit(Op::Pop, line);
        }
        let j = self.emit_jump(Op::Jump(0), line);
        let frame = self.loop_frames.last_mut().expect("loop frame present");
        if is_break {
            frame.break_jumps.push(j);
        } else {
            frame.continue_jumps.push(j);
        }
        Ok(())
    }

    /// `try { body } catch (T e) { … } … [finally { … }]` — native unwinding (M-faults 2b).
    ///
    /// Shape emitted:
    /// ```text
    ///   PushHandler(catch_lp)        ; capture frame depth + stack height
    ///   <body>                       ; a Throw unwinds to catch_lp
    ///   PopHandler ; <finally> ; Jump(end)        ; normal completion
    /// catch_lp:                       ; VM landed with the thrown value at slot `v_slot`
    ///   ; ($exc local registered here so catch-body locals stack above the value)
    ///   for each clause: <type test(s)> → bind+body → <finally> → Jump(cleanup)
    ///   <finally> ; Throw             ; no clause matched → re-throw
    /// cleanup: Pop($exc)              ; caught paths discard the value, converge with the normal path
    /// end:
    /// ```
    /// A `return`/`break`/`continue` inside the body or a catch runs the same `finally` (and pops the
    /// handler) via the `finally_stack` (see `emit_finally_for_exit`).
    pub(super) fn compile_try(
        &mut self,
        body: &[Stmt],
        catches: &[crate::ast::CatchClause],
        finally: Option<&[Stmt]>,
        line: u32,
    ) -> Result<(), String> {
        // A `try` is a statement, so the operand stack is clean here: height == locals.len().
        let body_base = self.locals.len();
        let push_idx = self.emit_jump(Op::PushHandler(0), line); // target patched to catch_lp

        // --- try body ---
        self.finally_stack.push(TryCtx {
            finally: finally.map(<[Stmt]>::to_vec),
            has_handler: true,
            loop_depth: self.loop_frames.len(),
        });
        self.begin_scope();
        for s in body {
            self.stmt(s)?;
        }
        self.end_scope(line);
        self.finally_stack.pop();

        // --- normal completion: drop the handler, run finally, skip the catch block ---
        self.emit(Op::PopHandler, line);
        self.emit_finally_block(finally, line)?;
        let normal_jump = self.emit_jump(Op::Jump(0), line);

        // --- catch landing pad: the VM pushed the thrown value at slot `v_slot` ---
        let catch_lp = self.here();
        self.patch_jump_to(push_idx, catch_lp);
        let v_slot = body_base;
        self.height = v_slot + 1;
        // Register the thrown value as a local so a catch body's own locals stack above it (the
        // per-statement `height = locals.len()` reset then keeps every slot aligned).
        let exc = self.add_local("$exc", CTy::Other);
        debug_assert_eq!(exc, v_slot, "thrown value slot must be the next frame slot");

        let mut caught_jumps = Vec::new();
        for clause in catches {
            // Dispatch: a match on any of the clause's type names falls through to `bind`; a full
            // miss jumps to `no_match` (the next clause).
            let names = catch_clause_names(&clause.ty);
            let mut to_bind = Vec::new();
            for name in &names {
                self.height = v_slot + 1;
                self.emit(Op::GetLocal(v_slot), line);
                self.emit(Op::IsInstance(name.clone()), line);
                let next_name = self.emit_jump(Op::JumpIfFalse(0), line); // false → try next name
                to_bind.push(self.emit_jump(Op::Jump(0), line)); // true → bind
                self.patch_jump(next_name);
            }
            let no_match = self.emit_jump(Op::Jump(0), line);
            let bind = self.here();
            for j in to_bind {
                self.patch_jump_to(j, bind);
            }

            // Bind the value to the clause variable (via `match_bindings`, reading slot `v_slot`),
            // then compile the catch body with its own finally/transfer context (no handler — the
            // unwind already consumed it).
            self.height = v_slot + 1;
            self.begin_scope();
            let n_before = self.match_bindings.len();
            self.match_bindings.push(MatchBinding {
                name: clause.name.clone(),
                match_slot: v_slot,
                path: Vec::new(),
                ty: catch_binding_cty(&clause.ty),
            });
            self.finally_stack.push(TryCtx {
                finally: finally.map(<[Stmt]>::to_vec),
                has_handler: false,
                loop_depth: self.loop_frames.len(),
            });
            for s in &clause.body {
                self.stmt(s)?;
            }
            self.finally_stack.pop();
            self.match_bindings.truncate(n_before);
            self.end_scope(line);

            // Caught: run finally, drop the value, converge.
            self.height = v_slot + 1;
            self.emit_finally_block(finally, line)?;
            self.emit(Op::Pop, line); // discard $exc → height v_slot
            caught_jumps.push(self.emit_jump(Op::Jump(0), line)); // → end (past cleanup's Pop)
            self.patch_jump(no_match);
        }

        // --- no clause matched: run finally, re-throw the value (still on top at `v_slot`) ---
        self.height = v_slot + 1;
        self.emit_finally_block(finally, line)?;
        self.emit(Op::Throw, line);

        // --- converge ---
        self.locals.pop(); // unregister $exc
        let end = self.here();
        self.patch_jump_to(normal_jump, end);
        for j in caught_jumps {
            self.patch_jump_to(j, end);
        }
        self.height = body_base;
        Ok(())
    }

    /// Emit a `finally` block inline (a fresh scope), or nothing when there is no `finally`. Balanced:
    /// `self.height`/`self.locals` are unchanged on return (M-faults 2b).
    pub(super) fn emit_finally_block(
        &mut self,
        finally: Option<&[Stmt]>,
        line: u32,
    ) -> Result<(), String> {
        if let Some(stmts) = finally {
            self.begin_scope();
            for s in stmts {
                self.stmt(s)?;
            }
            self.end_scope(line);
        }
        Ok(())
    }

    /// Emit the `PopHandler` (when the context's handler is still installed) and `finally` block for
    /// the innermost `n_exit` `try` contexts, innermost-first — run before a `return`/`break`/
    /// `continue` transfers out of them (M-faults 2b). The contexts are temporarily removed while
    /// their finallys are emitted so a transfer *inside* a finally doesn't re-enter them, then
    /// restored (the `try`s remain lexically active for the fall-through paths).
    pub(super) fn emit_finally_for_exit(&mut self, n_exit: usize, line: u32) -> Result<(), String> {
        if n_exit == 0 {
            return Ok(());
        }
        let start = self.finally_stack.len() - n_exit;
        let removed = self.finally_stack.split_off(start); // innermost == last
        let mut result = Ok(());
        for ctx in removed.iter().rev() {
            if ctx.has_handler {
                self.emit(Op::PopHandler, line);
            }
            if let Some(stmts) = &ctx.finally {
                self.begin_scope();
                for s in stmts {
                    if let Err(e) = self.stmt(s) {
                        result = Err(e);
                        break;
                    }
                }
                if result.is_ok() {
                    self.end_scope(line);
                }
            }
            if result.is_err() {
                break;
            }
        }
        self.finally_stack.extend(removed); // the try contexts are still lexically active
        result
    }
}
