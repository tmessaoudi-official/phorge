//! Bytecode VM — exec (M-Decomp W4). See mod.rs for the struct + core + entry points.

use super::*;

impl<'a> Vm<'a> {
    /// Execute one instruction in the current frame (`fr` = top of `frames`, in function `func`).
    /// Returns `Flow::Done` once `main` returns (program complete), `Flow::Next` otherwise. A
    /// fault carries only its body string; `run` attaches the source position from `Chunk.lines`.
    pub(super) fn exec_op(&mut self, op: Op, fr: usize, func: usize) -> Result<Flow, String> {
        match op {
            Op::Const(i) => {
                let v = self.program.functions[func].chunk.consts[i].clone();
                self.stack.push(v);
            }

            // Arithmetic dispatches into the single-sourced `value` kernels — the interpreter
            // calls the *same* functions, so the checked-op / div-zero / overflow fault path
            // is structurally identical across both backends (the Wave 0 `Op::Neg` divergence
            // class can no longer reopen).
            Op::AddI => {
                let (a, b) = self.pop2_int()?;
                self.push_i(crate::value::int_add(a, b))?;
            }
            Op::SubI => {
                let (a, b) = self.pop2_int()?;
                self.push_i(crate::value::int_sub(a, b))?;
            }
            Op::MulI => {
                let (a, b) = self.pop2_int()?;
                self.push_i(crate::value::int_mul(a, b))?;
            }
            Op::DivI => {
                let (a, b) = self.pop2_int()?;
                self.push_i(crate::value::int_div(a, b))?;
            }
            Op::RemI => {
                let (a, b) = self.pop2_int()?;
                self.push_i(crate::value::int_rem(a, b))?;
            }

            Op::AddF => {
                let (a, b) = self.pop2_float()?;
                self.stack.push(Value::Float(crate::value::float_add(a, b)));
            }
            Op::SubF => {
                let (a, b) = self.pop2_float()?;
                self.stack.push(Value::Float(crate::value::float_sub(a, b)));
            }
            Op::MulF => {
                let (a, b) = self.pop2_float()?;
                self.stack.push(Value::Float(crate::value::float_mul(a, b)));
            }
            Op::DivF => {
                let (a, b) = self.pop2_float()?;
                self.push_f(crate::value::float_div(a, b))?;
            }
            Op::RemF => {
                let (a, b) = self.pop2_float()?;
                self.push_f(crate::value::float_rem(a, b))?;
            }

            // Decimal `+ - *` (M-NUM S1): pop two raw values (a `Decimal`, or a mixed `Decimal`/`Int`)
            // and dispatch into the single-sourced kernels — the interpreter's `arith` calls the SAME
            // functions, so the exact-result + i128-overflow-fault path is byte-identical (the same
            // discipline as the int/float ops). The kernel widens an `Int` operand to scale 0.
            Op::AddD => {
                let (a, b) = self.pop2();
                self.stack.push(crate::value::decimal_add(&a, &b)?);
            }
            Op::SubD => {
                let (a, b) = self.pop2();
                self.stack.push(crate::value::decimal_sub(&a, &b)?);
            }
            Op::MulD => {
                let (a, b) = self.pop2();
                self.stack.push(crate::value::decimal_mul(&a, &b)?);
            }
            Op::RemD => {
                let (a, b) = self.pop2();
                self.stack.push(crate::value::decimal_rem(&a, &b)?);
            }
            Op::DivD => {
                let (a, b) = self.pop2();
                self.stack.push(crate::value::decimal_div_exact(&a, &b)?);
            }

            // Bitwise ops on ints (primitives P2) — shared `value::*` kernels (interpreter parity).
            Op::BitAnd => {
                let (a, b) = self.pop2_int()?;
                self.stack.push(Value::Int(crate::value::int_bitand(a, b)));
            }
            Op::BitOr => {
                let (a, b) = self.pop2_int()?;
                self.stack.push(Value::Int(crate::value::int_bitor(a, b)));
            }
            Op::BitXor => {
                let (a, b) = self.pop2_int()?;
                self.stack.push(Value::Int(crate::value::int_bitxor(a, b)));
            }
            Op::Shl => {
                let (a, b) = self.pop2_int()?;
                self.push_i(crate::value::int_shl(a, b))?;
            }
            Op::Shr => {
                let (a, b) = self.pop2_int()?;
                self.push_i(crate::value::int_shr(a, b))?;
            }

            Op::Neg => match self.pop() {
                // `value::int_neg` is shared with the interpreter (`eval_unary`): negating
                // `i64::MIN` is a clean `"integer overflow"` runtime error, never a panic.
                Value::Int(n) => self.push_i(crate::value::int_neg(n))?,
                Value::Float(x) => self.stack.push(Value::Float(-x)),
                // `decimal` negation via the shared kernel (M-NUM S1): checked, never `-0`.
                Value::Decimal { unscaled, scale } => {
                    self.stack.push(crate::value::decimal_neg(unscaled, scale)?);
                }
                v => return Err(format!("cannot negate {}", v.type_name())),
            },
            Op::Not => match self.pop() {
                Value::Bool(b) => self.stack.push(Value::Bool(!b)),
                v => return Err(format!("cannot apply ! to {}", v.type_name())),
            },
            Op::BitNot => match self.pop() {
                Value::Int(n) => self.stack.push(Value::Int(crate::value::int_bitnot(n))),
                v => return Err(format!("cannot apply ~ to {}", v.type_name())),
            },

            Op::Eq => {
                let b = self.pop();
                let a = self.pop();
                self.stack.push(Value::Bool(a.eq_val(&b)));
            }
            Op::Ne => {
                let b = self.pop();
                let a = self.pop();
                self.stack.push(Value::Bool(!a.eq_val(&b)));
            }
            Op::Lt | Op::Gt | Op::Le | Op::Ge => {
                let b = self.pop();
                let a = self.pop();
                self.stack.push(Value::Bool(compare(&op, &a, &b)?));
            }

            Op::Pop => {
                self.pop();
            }
            Op::GetLocal(slot) => {
                let base = self.frames[fr].slot_base;
                let idx = self.frame_slot(base, slot);
                let v = self.stack[idx].clone();
                self.stack.push(v);
            }
            Op::SetLocal(slot) => {
                let base = self.frames[fr].slot_base;
                let v = self.pop();
                let idx = self.frame_slot(base, slot);
                self.stack[idx] = v;
            }

            Op::Jump(target) => self.frames[fr].ip = target,
            Op::JumpIfFalse(target) => match self.pop() {
                Value::Bool(false) => self.frames[fr].ip = target,
                Value::Bool(true) => {}
                v => return Err(format!("expected bool, found {}", v.type_name())),
            },

            Op::Concat(n) => {
                let parts = self.split_off(n);
                let mut s = String::new();
                for v in &parts {
                    match v.as_display() {
                        Some(t) => s.push_str(&t),
                        None => {
                            return Err(format!(
                                "cannot interpolate {} into a string",
                                v.type_name()
                            ))
                        }
                    }
                }
                self.stack.push(Value::Str(s));
            }
            Op::MakeList(n) => {
                let items = self.split_off(n);
                self.stack.push(Value::List(Rc::new(items)));
            }
            Op::MakeMap(n) => {
                // The 2n operands are k1,v1,…,kn,vn (vn on top). Pair them up and build the
                // insertion-ordered map via the shared kernel (same dedup as the interpreter).
                let flat = self.split_off(2 * n);
                let mut pairs = Vec::with_capacity(n);
                let mut it = flat.into_iter();
                while let (Some(k), Some(v)) = (it.next(), it.next()) {
                    pairs.push((k, v));
                }
                let map = crate::value::build_map(pairs)?;
                self.stack.push(Value::Map(Rc::new(map)));
            }
            Op::Index => {
                // Polymorphic (M-RT S3): a list uses an int index with bounds; a map looks the key up.
                let index = self.pop();
                match self.pop() {
                    Value::List(xs) => {
                        let idx = match index {
                            Value::Int(n) => n,
                            v => {
                                return Err(format!("expected int index, found {}", v.type_name()))
                            }
                        };
                        let i = usize::try_from(idx)
                            .ok()
                            .filter(|i| *i < xs.len())
                            .ok_or_else(|| "list index out of range".to_string())?;
                        self.stack.push(xs[i].clone());
                    }
                    Value::Map(m) => self.stack.push(crate::value::map_index(&m, &index)?),
                    v => return Err(format!("cannot index {}", v.type_name())),
                }
            }
            Op::SetIndex => {
                // COW element set (M-mut.5): pop value, index, container; mutate a uniquely-owned
                // copy via `Rc::make_mut` (clones only if another binding shares it — value
                // semantics), push the resulting container for the caller to store back.
                let v = self.pop();
                let index = self.pop();
                match self.pop() {
                    Value::List(mut xs) => {
                        let idx = match index {
                            Value::Int(n) => n,
                            x => {
                                return Err(format!("expected int index, found {}", x.type_name()))
                            }
                        };
                        crate::value::list_set(Rc::make_mut(&mut xs).as_mut_slice(), idx, v)?;
                        self.stack.push(Value::List(xs));
                    }
                    Value::Map(mut m) => {
                        crate::value::map_set(Rc::make_mut(&mut m), &index, v)?;
                        self.stack.push(Value::Map(m));
                    }
                    v => return Err(format!("cannot index-assign {}", v.type_name())),
                }
            }
            Op::Len => match self.pop() {
                Value::List(xs) => self.stack.push(Value::Int(xs.len() as i64)),
                v => return Err(format!("cannot take length of {}", v.type_name())),
            },
            Op::MakeRange(inclusive) => {
                // `end` was pushed last, so it's on top; `start` is below (compiler emit order).
                let end = self.pop_int()?;
                let start = self.pop_int()?;
                // Shared size-guarded materialization (P1-#9): identical list to the interpreter, and
                // a range too wide to fit faults `"range too large"` rather than OOM-aborting (EV-7).
                let list = crate::value::build_range(start, end, inclusive)?;
                self.stack.push(Value::List(Rc::new(list)));
            }

            // Green-thread concurrency (M6 W4 / S4.3) — synchronous-degenerate bodies (the cooperative
            // driver, 3b-2b/c, reuses the same ops but defers/suspends). Channels/tasks carry their
            // scheduler ids (allocated from `self.coop`); a task's result lives in `coop.results`. The
            // fault strings MUST match the interpreter's exactly (run≡runvm + `agree_err` parity).
            Op::Spawn => {
                // Eager: the spawned call already ran inline (its result is on top); register a
                // finished task and store the result by id. (The cooperative cutover will instead
                // defer the call as a scheduler task rather than running it here.)
                let result = self.pop();
                let id = self.coop.borrow_mut().sched.spawn();
                self.coop.borrow_mut().results.insert(id, result);
                self.stack.push(Value::Task(id));
            }
            Op::SpawnCall(func_idx, argc) => {
                // Eager (synchronous path): the call has NOT run yet — run the free function inline now
                // and register the finished task, byte-identical to the former `<call>; Op::Spawn`. The
                // cooperative driver (S4.3) overrides this to defer the function as a scheduler task.
                let args = self.split_off(argc);
                let result = self.call_function_value(func_idx, args)?;
                let id = self.coop.borrow_mut().sched.spawn();
                self.coop.borrow_mut().results.insert(id, result);
                self.stack.push(Value::Task(id));
            }
            Op::ChannelNew => {
                let id = self.coop.borrow_mut().sched.new_channel();
                self.stack.push(Value::Channel(
                    id,
                    Rc::new(std::cell::RefCell::new(std::collections::VecDeque::new())),
                ));
            }
            Op::ChannelSend => {
                let value = self.pop();
                match self.pop() {
                    Value::Channel(_, buf) => {
                        buf.borrow_mut().push_back(value);
                        self.stack.push(Value::Unit);
                    }
                    v => return Err(format!("cannot send on a {}", v.type_name())),
                }
            }
            Op::ChannelRecv => match self.pop() {
                Value::Channel(_, buf) => {
                    let front = buf.borrow_mut().pop_front();
                    match front {
                        Some(v) => self.stack.push(v),
                        // No scheduler to yield to in the eager path — cooperative suspends instead.
                        None => return Err("recv from empty channel".to_string()),
                    }
                }
                v => return Err(format!("cannot recv from a {}", v.type_name())),
            },
            Op::Join => match self.pop() {
                Value::Task(id) => {
                    let result = self.coop.borrow().results.get(&id).cloned();
                    match result {
                        Some(v) => self.stack.push(v),
                        None => return Err("join on an incomplete task".to_string()),
                    }
                }
                v => return Err(format!("cannot join a {}", v.type_name())),
            },

            Op::CallNative(idx, argc) => {
                // The native's `eval` is shared verbatim with the interpreter (structural parity).
                // `validate` has already bounded `idx`; the args sit on top in source order. The
                // enum is `Copy`, so reading it ends the `'static` registry borrow before the
                // higher-order invoker captures `&mut self`.
                let args = self.split_off(argc);
                let eval = crate::native::registry()[idx].eval;
                let result = match eval {
                    crate::native::NativeEval::Pure(f) => f(&args, &mut self.out)?,
                    // Reflection natives read the precomputed class hierarchy (same `ClassTables` the
                    // interpreter holds + the transpiler emits, so the result is byte-identical).
                    crate::native::NativeEval::Reflective(f) => {
                        f(&args, &self.program.class_tables)?
                    }
                    crate::native::NativeEval::HigherOrder(f) => {
                        // A closure argument is run re-entrantly on *this* VM via
                        // `call_closure_value` — the same `exec_op` core the main loop drives, so a
                        // closure fault and its result are byte-identical to the interpreter's
                        // `call_closure` path (M-RT S7b-3).
                        let mut invoke =
                            |fv: &Value, cargs: Vec<Value>| self.call_closure_value(fv, cargs);
                        f(&args, &mut invoke)?
                    }
                };
                self.stack.push(result);
            }

            Op::Call(idx) => {
                if self.frames.len() >= MAX_CALL_DEPTH {
                    return Err("stack overflow".to_string());
                }
                let arity = self.program.functions[idx].arity;
                let slot_base = self.pop_n_start(arity);
                self.frames.push(Frame {
                    func: idx,
                    ip: 0,
                    slot_base,
                });
            }
            // `CallStaticOverload` (Statics-B) is byte-identical at runtime to `CallOverload`: the
            // compiler pushed a dummy receiver below the `argc` args, so selection (which peeks only
            // the top `argc`) is unaffected, and the selected static body's `arity` (= 1 + nparams)
            // pops the dummy together with the args. The two ops differ only in compile-time
            // `stack_effect`, so they share this arm.
            Op::CallOverload(set_id, argc) | Op::CallStaticOverload(set_id, argc) => {
                if self.frames.len() >= MAX_CALL_DEPTH {
                    return Err("stack overflow".to_string());
                }
                // M-RT dynamic dispatch: peek the `argc` argument values already on the stack and
                // pick the most-specific matching overload — the SAME selector + `ParamKind`s the
                // interpreter uses, so `run`/`runvm` resolve to the same function.
                let n = self.stack.len();
                let set = &self.program.overloads[set_id];
                let cands: Vec<Vec<crate::dispatch::ParamKind>> =
                    set.iter().map(|(k, _)| k.clone()).collect();
                let target = match crate::dispatch::select_overload(
                    &cands,
                    &self.stack[n - argc..],
                    &self.program.class_implements,
                ) {
                    Ok(pos) => set[pos].1,
                    Err(e) => {
                        let name = &self.program.functions[set[0].1].name;
                        return Err(match e {
                            crate::dispatch::SelectErr::Ambiguous => {
                                format!("ambiguous overloaded call to `{name}`")
                            }
                            crate::dispatch::SelectErr::NoMatch => {
                                format!("no overload of `{name}` matches the argument types")
                            }
                        });
                    }
                };
                let arity = self.program.functions[target].arity;
                let slot_base = self.pop_n_start(arity);
                self.frames.push(Frame {
                    func: target,
                    ip: 0,
                    slot_base,
                });
            }

            Op::Return => {
                let rv = self.pop();
                // Batch-1 B: when the entry (`main`) frame is about to pop, stash its return value
                // as the exit code before `do_return` discards it (it only re-pushes a return value
                // when a caller frame remains).
                if self.frames.len() == 1 {
                    self.exit_value = rv.clone();
                }
                self.do_return(rv);
                if self.frames.is_empty() {
                    return Ok(Flow::Done);
                }
            }

            // --- P4a: enums + match ---
            Op::MakeEnum(idx) => {
                // Clone the small descriptor (two `String`s) so the `&self.program` borrow ends
                // before `split_off` takes `&mut self`.
                let desc = self.program.enum_descs[idx].clone();
                let payload = self.split_off(desc.arity);
                self.stack.push(Value::Enum(Rc::new(EnumVal {
                    ty: desc.ty,
                    variant: desc.variant,
                    payload,
                })));
            }
            Op::MatchTag(idx) => {
                let want = self.program.enum_descs[idx].variant.clone();
                // Pop the scrutinee copy the compiler pushed for this test (it reloads `$match`
                // per arm), leaving just the bool for the following `JumpIfFalse`.
                let matched = matches!(self.pop(), Value::Enum(ev) if ev.variant == want);
                self.stack.push(Value::Bool(matched));
            }
            Op::GetEnumField(i) => match self.pop() {
                Value::Enum(ev) => {
                    // Clone the element out of the shared payload (can't move out of an `Rc`); the
                    // element is itself `Rc`-shared if compound, so this stays an O(1) bump (P5a).
                    let v = ev
                        .payload
                        .get(i)
                        .cloned()
                        .ok_or_else(|| format!("enum payload index {i} out of range"))?;
                    self.stack.push(v);
                }
                v => return Err(format!("cannot extract enum field from {}", v.type_name())),
            },
            // A fixed runtime fault (match-exhaustiveness backstop or `opt!`-on-null), byte-identical
            // to the interpreter's fault for the same cause (the `agree_err` oracle classifies by
            // body). The message is single-sourced on `FaultMsg` (M3 S2.5).
            Op::Fault(m) => return Err(m.message()),

            // --- P4b: classes ---
            Op::MakeInstance(idx) => {
                // Clone the descriptor's class + field names so the `&self.program` borrow ends
                // before `split_off` takes `&mut self` (mirrors `MakeEnum`).
                let desc = self.program.class_descs[idx].clone();
                let values = self.split_off(desc.fields.len());
                // M-perf S1b: place each promoted-field value at its slot in the shared layout. The
                // field push order (`desc.fields`) need not match slot order — `slot(name)` maps it —
                // so construction and access agree regardless of order (the MI-offset hazard is moot).
                let layout = desc.layout.clone();
                let mut slots: Vec<Option<Value>> = vec![None; layout.len()];
                for (name, val) in desc.fields.iter().zip(values) {
                    if let Some(i) = layout.slot(name) {
                        slots[i] = Some(val);
                    }
                }
                self.stack.push(Value::Instance(Rc::new(Instance {
                    class: desc.class,
                    layout,
                    fields: RefCell::new(slots),
                })));
            }
            Op::GetField(idx) => {
                // S2 inline cache: this op's site is `(func, ip - 1)` (`ip` was pre-incremented).
                let site = self.frames[fr].ip - 1;
                match self.pop() {
                    Value::Instance(inst) => {
                        let lp: *const crate::value::ClassLayout = Rc::as_ptr(&inst.layout);
                        // Resolve the slot — a monomorphic site hits the cache (no name clone, no
                        // hash); a miss falls back to the layout hash and refills. The block scopes the
                        // immutable `field_caches` borrow so the `self.stack.push` below can take `&mut`.
                        let slot = {
                            let cell = &self.field_caches[func][site];
                            let (cached, cslot) = cell.get();
                            if std::ptr::eq(cached, lp) {
                                cslot as usize
                            } else {
                                match inst.layout.slot(&self.program.names[idx]) {
                                    Some(s) => {
                                        cell.set((lp, s as u32));
                                        s
                                    }
                                    // Checker-unreachable for a typed read; fault, never panic (EV-7).
                                    None => {
                                        return Err(format!(
                                            "no field `{}` on `{}`",
                                            self.program.names[idx], inst.class
                                        ))
                                    }
                                }
                            }
                        };
                        // `slot < layout.len() == fields.len()` (slot came from this instance's layout),
                        // so the index is in-bounds. An in-layout-but-unset slot (`None`) faults exactly
                        // like a pre-S1b absent key (byte-identical to the interpreter's `Expr::Member`).
                        match inst.fields.borrow()[slot].clone() {
                            Some(v) => self.stack.push(v),
                            None => {
                                return Err(format!(
                                    "no field `{}` on `{}`",
                                    self.program.names[idx], inst.class
                                ))
                            }
                        }
                    }
                    v => {
                        return Err(format!(
                            "cannot read `.{}` on {}",
                            self.program.names[idx],
                            v.type_name()
                        ))
                    }
                }
            }
            Op::SetField(idx) => {
                // `o.f = e` (M-mut.6): pop the value (top), then the instance, and write the field
                // into the shared `Rc<Instance>` cell in place — visible through every binding. The
                // value is fully evaluated before `borrow_mut`, so no borrow is held across `eval`.
                // S2 inline cache, keyed identically to `GetField`.
                let site = self.frames[fr].ip - 1;
                let value = self.pop();
                match self.pop() {
                    Value::Instance(inst) => {
                        let lp: *const crate::value::ClassLayout = Rc::as_ptr(&inst.layout);
                        let cell = &self.field_caches[func][site];
                        let (cached, cslot) = cell.get();
                        if std::ptr::eq(cached, lp) {
                            inst.fields.borrow_mut()[cslot as usize] = Some(value);
                        } else if let Some(s) = inst.layout.slot(&self.program.names[idx]) {
                            cell.set((lp, s as u32));
                            inst.fields.borrow_mut()[s] = Some(value);
                        }
                        // A name not in the layout is checker-unreachable for a typed write — drop it
                        // silently (matches the S1b `set_field` no-op), never panic.
                    }
                    v => {
                        return Err(format!(
                            "cannot set `.{}` on {}",
                            self.program.names[idx],
                            v.type_name()
                        ))
                    }
                }
            }
            Op::GetStatic(idx) => {
                // Program-lifetime static read `ClassName.field` (M-mut.7).
                self.stack.push(self.statics[idx].clone());
            }
            Op::SetStatic(idx) => {
                // `ClassName.field = e` (M-mut.7): pop the value into the program-level static slot.
                self.statics[idx] = self.pop();
            }
            Op::IsInstance(name) => {
                // `value instanceof name` (M-RT S1; interfaces S2; class ancestors S6c.3): true iff the
                // popped value is an instance whose class equals `name` OR has `name` among its
                // supertypes — parent classes AND interfaces, via the shared `instanceof_table` oracle
                // (stored in `class_implements`). A non-instance is `false`, never a fault —
                // byte-identical to the interpreter (`Expr::InstanceOf`) and PHP's `instanceof`.
                let v = self.pop();
                let is = matches!(&v, Value::Instance(inst)
                    if inst.class == name
                        || self
                            .program
                            .class_implements
                            .get(&inst.class)
                            .is_some_and(|ifaces| ifaces.contains(&name)));
                self.stack.push(Value::Bool(is));
            }

            // --- P4c: methods + `this` ---
            Op::CallMethod(name_idx, argc) => {
                if self.frames.len() >= MAX_CALL_DEPTH {
                    return Err("stack overflow".to_string());
                }
                let mname = self.program.names[name_idx].clone();
                // The receiver sits just below the `argc` args; its slot becomes the new frame's
                // slot 0 (`this`), with the args at slots 1..=argc (decision P4-6).
                let slot_base = self.pop_n_start(argc + 1);
                let class = match &self.stack[slot_base] {
                    Value::Instance(inst) => inst.class.clone(),
                    v => return Err(format!("cannot call `.{mname}()` on {}", v.type_name())),
                };
                // Dynamic dispatch off the receiver's runtime class. Method existence is
                // checker-enforced, so the miss is a defensive backstop (interpreter parity).
                let key = (class, mname);
                // M-RT overloading: an overloaded method selects among its set by the argument values
                // (the receiver is at `slot_base`, the `argc` args at `slot_base + 1 ..`) — the same
                // selector the interpreter's `call_method` runs. A single method stays on the direct path.
                let func = if let Some(&set_id) = self.program.method_overloads.get(&key) {
                    let set = &self.program.overloads[set_id];
                    let cands: Vec<Vec<crate::dispatch::ParamKind>> =
                        set.iter().map(|(k, _)| k.clone()).collect();
                    let args = &self.stack[slot_base + 1..slot_base + 1 + argc];
                    match crate::dispatch::select_overload(
                        &cands,
                        args,
                        &self.program.class_implements,
                    ) {
                        Ok(pos) => set[pos].1,
                        Err(crate::dispatch::SelectErr::Ambiguous) => {
                            return Err(format!("ambiguous overloaded call to `{}`", key.1))
                        }
                        Err(crate::dispatch::SelectErr::NoMatch) => {
                            return Err(format!(
                                "no overload of `{}` matches the argument types",
                                key.1
                            ))
                        }
                    }
                } else {
                    *self
                        .program
                        .methods
                        .get(&key)
                        .ok_or_else(|| format!("no method `{}` on `{}`", key.1, key.0))?
                };
                self.frames.push(Frame {
                    func,
                    ip: 0,
                    slot_base,
                });
            }

            // M-RT super/parent: non-virtual dispatch to a *baked* function index — same frame layout
            // as `CallMethod` (slot 0 = the receiver/`this`, slots `1..=argc` the args) but the target
            // is resolved at compile time (the version an override shadows), not from the receiver.
            Op::CallParent(func, argc) => {
                if self.frames.len() >= MAX_CALL_DEPTH {
                    return Err("stack overflow".to_string());
                }
                let slot_base = self.pop_n_start(argc + 1);
                self.frames.push(Frame {
                    func,
                    ip: 0,
                    slot_base,
                });
            }

            // --- M3 S3: lambda closures ---

            // Pop `functions[idx].n_captures` capture values from the stack and build a
            // `Value::Closure(ClosureData::Byte { func: idx, captures })`.
            Op::MakeClosure(idx) => {
                let n_captures = self.program.functions[idx].n_captures;
                let captures = self.split_off(n_captures);
                self.stack
                    .push(Value::Closure(Rc::new(crate::value::ClosureData::Byte {
                        func: idx,
                        captures,
                    })));
            }

            // Call a first-class closure value. Stack before: `[.. closure arg0 arg1 ..]`.
            // The new frame layout is `[caps.., arg0, arg1, ..]` — captures are prepended
            // so `GetLocal(0)` is the first capture and `GetLocal(n_captures)` is the first arg,
            // matching the frame layout the compiler emits inside the lambda body.
            Op::CallValue(argc) => {
                if self.frames.len() >= MAX_CALL_DEPTH {
                    return Err("stack overflow".to_string());
                }
                // Pop the `argc` args (in source order: last pushed = last arg).
                let args = self.split_off(argc);
                // Pop the closure itself (below the args on the stack before split_off).
                let closure = self.pop();
                let (func_idx, captures) = match closure {
                    Value::Closure(cd) => match cd.as_ref().clone() {
                        crate::value::ClosureData::Byte { func, captures } => (func, captures),
                        _ => return Err("expected a bytecode closure".to_string()),
                    },
                    v => return Err(format!("cannot call {} as a function", v.type_name())),
                };
                // Verify arity: the function expects `n_captures + n_params` args in total;
                // the caller supplies `argc` args (the captures are prepended by CallValue).
                let func_arity = self.program.functions[func_idx].arity;
                let n_captures = self.program.functions[func_idx].n_captures;
                let n_params = func_arity - n_captures;
                if argc != n_params {
                    return Err(format!(
                        "wrong number of arguments: expected {n_params}, got {argc}"
                    ));
                }
                // Push captures then args as the new frame's locals window.
                let slot_base = self.stack.len();
                self.stack.extend(captures);
                self.stack.extend(args);
                self.frames.push(Frame {
                    func: func_idx,
                    ip: 0,
                    slot_base,
                });
            }

            // --- M-faults 2b: native unwinding (try/catch/finally) ---

            // Install a handler at the current frame depth + stack height; on a `Throw` the run loop
            // unwinds to its landing pad. Pure bookkeeping — no stack effect.
            Op::PushHandler(catch_ip) => {
                self.handlers.push(Handler {
                    catch_ip,
                    frame_depth: self.frames.len(),
                    stack_height: self.stack.len(),
                });
            }
            // The try body completed (or control is leaving it) — drop the most recent handler.
            Op::PopHandler => {
                self.handlers.pop();
            }
            // `throw e` — stash the value and raise the sentinel fault; the run loop searches the
            // handler stack (`unwind_throw`). Crossing a higher-order-native boundary falls out for
            // free: `run_until` can't find a handler inside the closure's frames, so the sentinel
            // propagates up to the outer loop, which unwinds to the `try` around the native call.
            Op::Throw => {
                let v = self.pop();
                self.pending_throw = Some(v);
                return Err(crate::chunk::THROW_SENTINEL.to_string());
            }
        }
        Ok(Flow::Next)
    }
}
