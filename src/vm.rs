//! Stack VM that executes a `Chunk`. See docs/specs/2026-06-15-m2-bytecode-vm-design.md (§6).
//! P1–P3: scalar arithmetic/negate/print/return (P1); the full `main`-only expression and
//! statement surface (P2); user calls, clox-style call frames, and recursion (P3). Output is
//! captured into a String (mirrors `interpreter::interpret`) so the VM can be differential-tested
//! against the tree-walker.
//!
//! `run` returns a unified runtime `Diagnostic` (M2 P3.5 Wave 2 Task 2.1): the per-op `exec_op`
//! yields a bare fault body, and `run` attaches the source line from `Chunk.lines[ip]` so the
//! fault renders `runtime error at <line>: <body>`. The canonical body (`"division by zero"`,
//! `"integer overflow"`, …) is preserved verbatim, keeping error parity with the interpreter
//! (the `agree_err` oracle classifies by body, tolerating the VM-only line prefix).

use crate::chunk::{BytecodeProgram, Op};
use crate::diagnostic::Diagnostic;
use crate::limits::MAX_CALL_DEPTH;
use crate::value::{EnumVal, Instance, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Whether the dispatch loop should fetch the next instruction or stop (the `main` frame
/// returned). Lets the per-op `exec_op` signal completion without owning the run loop.
enum Flow {
    Next,
    Done,
}

// Call-frame depth is capped by the shared `limits::MAX_CALL_DEPTH` (same limit the interpreter
// enforces, keeping the backends parity-identical). Exceeding it is a clean `"stack overflow"`
// runtime error rather than an OOM/abort (decision P3-4).

/// A live call frame: which function, the instruction pointer into its chunk, and the index
/// in the value stack where this frame's locals window begins (decision P3-1).
struct Frame {
    func: usize,
    ip: usize,
    slot_base: usize,
}

pub struct Vm<'a> {
    program: &'a BytecodeProgram,
    stack: Vec<Value>,
    frames: Vec<Frame>,
    /// Program-lifetime `static` field storage (M-mut.7), indexed by `Op::GetStatic`/`SetStatic`.
    /// Seeded once from `program.static_inits` (the once-at-load literal values).
    statics: Vec<Value>,
    out: String,
}

impl<'a> Vm<'a> {
    pub fn new(program: &'a BytecodeProgram) -> Self {
        Self {
            program,
            stack: Vec::new(),
            frames: Vec::new(),
            statics: program.static_inits.clone(),
            out: String::new(),
        }
    }

    /// Execute the program from `main`, returning captured output (`Ok`) or a runtime
    /// error (`Err`).
    pub fn run(mut self) -> Result<String, Diagnostic> {
        // Fail fast on malformed bytecode (a compiler bug) with a clean error instead of a panic
        // mid-execution — keeps the no-crash contract (EV-7). See `BytecodeProgram::validate`.
        // Bytecode-validation faults have no source line, so they surface position-less.
        self.program.validate().map_err(Diagnostic::runtime)?;
        self.frames.push(Frame {
            func: self.program.main,
            ip: 0,
            slot_base: 0,
        });
        loop {
            let fr = self.frames.len() - 1;
            let func = self.frames[fr].func;
            let ip = self.frames[fr].ip;
            let code = &self.program.functions[func].chunk.code;
            if ip >= code.len() {
                // The compiler emits a trailing `Return` for every function (P3-7); reaching
                // the end without one is a compiler bug — treat as an implicit `Unit` return.
                self.do_return(Value::Unit);
                if self.frames.is_empty() {
                    return Ok(self.out);
                }
                continue;
            }
            // Clone the op (cheap) so we don't hold a borrow of `self.program` across the
            // mutable stack operations below.
            let op = code[ip].clone();
            self.frames[fr].ip += 1;
            // `ip` is the *pre-increment* index — the op that actually executes. On a fault,
            // locate it via this function's `Chunk.lines[ip]` and surface a positioned runtime
            // `Diagnostic`. (The tree-walker can't supply a line — a deliberate backend
            // asymmetry the `agree_err` oracle tolerates: it classifies by fault body, not text.)
            match self.exec_op(op, fr, func) {
                Ok(Flow::Next) => {}
                Ok(Flow::Done) => return Ok(self.out),
                Err(msg) => {
                    // Walk the live call stack innermost → outermost. Each frame's line is the source
                    // line of the op it is paused on: the faulting op for the top frame, the pending
                    // `Call` for the rest. `Frame.ip` was pre-incremented before `exec_op`, so `ip - 1`
                    // is that op. `file` is filled later by the loader source map (Task 4).
                    let frames: Vec<crate::diagnostic::Frame> = self
                        .frames
                        .iter()
                        .rev()
                        .map(|fr| {
                            let fline = self.program.functions[fr.func]
                                .chunk
                                .lines
                                .get(fr.ip.saturating_sub(1))
                                .copied()
                                .unwrap_or(0);
                            crate::diagnostic::Frame {
                                function: self.program.functions[fr.func].name.clone(),
                                file: None,
                                line: fline,
                                col: 0,
                            }
                        })
                        .collect();
                    let line = frames.first().map_or(0, |f| f.line);
                    return Err(Diagnostic::runtime_at_line(msg, line).with_frames(frames));
                }
            }
        }
    }

    /// Execute one instruction in the current frame (`fr` = top of `frames`, in function `func`).
    /// Returns `Flow::Done` once `main` returns (program complete), `Flow::Next` otherwise. A
    /// fault carries only its body string; `run` attaches the source position from `Chunk.lines`.
    fn exec_op(&mut self, op: Op, fr: usize, func: usize) -> Result<Flow, String> {
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
                self.stack.push(Value::Float(crate::value::float_div(a, b)));
            }
            Op::RemF => {
                let (a, b) = self.pop2_float()?;
                self.stack.push(Value::Float(crate::value::float_rem(a, b)));
            }

            Op::Neg => match self.pop() {
                // `value::int_neg` is shared with the interpreter (`eval_unary`): negating
                // `i64::MIN` is a clean `"integer overflow"` runtime error, never a panic.
                Value::Int(n) => self.push_i(crate::value::int_neg(n))?,
                Value::Float(x) => self.stack.push(Value::Float(-x)),
                v => return Err(format!("cannot negate {}", v.type_name())),
            },
            Op::Not => match self.pop() {
                Value::Bool(b) => self.stack.push(Value::Bool(!b)),
                v => return Err(format!("cannot apply ! to {}", v.type_name())),
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

            Op::CallNative(idx, argc) => {
                // The native's `eval` is shared verbatim with the interpreter (structural parity).
                // `validate` has already bounded `idx`; the args sit on top in source order. The
                // enum is `Copy`, so reading it ends the `'static` registry borrow before the
                // higher-order invoker captures `&mut self`.
                let args = self.split_off(argc);
                let eval = crate::native::registry()[idx].eval;
                let result = match eval {
                    crate::native::NativeEval::Pure(f) => f(&args, &mut self.out)?,
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

            Op::Return => {
                let rv = self.pop();
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
                let fields: HashMap<String, Value> = desc.fields.into_iter().zip(values).collect();
                self.stack.push(Value::Instance(Rc::new(Instance {
                    class: desc.class,
                    fields: RefCell::new(fields),
                })));
            }
            Op::GetField(idx) => {
                let name = self.program.names[idx].clone();
                match self.pop() {
                    Value::Instance(inst) => {
                        // Clone the field value out and drop the borrow before pushing (handle
                        // semantics: the shared cell stays available for a later mutation).
                        let v = inst.fields.borrow().get(&name).cloned();
                        match v {
                            Some(v) => self.stack.push(v),
                            // Byte-identical to the interpreter (`Expr::Member`): a checker-valid but
                            // unpopulated explicit `Field` read faults here at runtime.
                            None => return Err(format!("no field `{name}` on `{}`", inst.class)),
                        }
                    }
                    v => return Err(format!("cannot read `.{name}` on {}", v.type_name())),
                }
            }
            Op::SetField(idx) => {
                // `o.f = e` (M-mut.6): pop the value (top), then the instance, and write the field
                // into the shared `Rc<Instance>` cell in place — visible through every binding. The
                // value is fully evaluated before `borrow_mut`, so no borrow is held across `eval`.
                let name = self.program.names[idx].clone();
                let value = self.pop();
                match self.pop() {
                    Value::Instance(inst) => {
                        inst.fields.borrow_mut().insert(name, value);
                    }
                    v => return Err(format!("cannot set `.{name}` on {}", v.type_name())),
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
                // `value instanceof name` (M-RT S1; interfaces S2): true iff the popped value is an
                // instance whose class equals `name` OR implements interface `name` (via the shared
                // `class_implements` table). A non-instance is `false`, never a fault — byte-identical
                // to the interpreter (`Expr::InstanceOf`) and PHP's `instanceof`.
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
                let func = *self
                    .program
                    .methods
                    .get(&key)
                    .ok_or_else(|| format!("no method `{}` on `{}`", key.1, key.0))?;
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
        }
        Ok(Flow::Next)
    }

    /// Unwind the current frame: truncate its locals window and pop it; if a caller remains,
    /// push the return value onto the caller's stack (decision P3-2).
    fn do_return(&mut self, rv: Value) {
        let base = self.frames[self.frames.len() - 1].slot_base;
        debug_assert!(
            base <= self.stack.len(),
            "vm return base {base} > stack len {} (func {})",
            self.stack.len(),
            self.frames.last().map_or(usize::MAX, |f| f.func)
        );
        self.stack.truncate(base);
        self.frames.pop();
        if !self.frames.is_empty() {
            self.stack.push(rv);
        }
    }

    /// Invoke a first-class closure VALUE re-entrantly and return its result. Unlike [`Op::CallValue`]
    /// (which pushes a frame and lets the main `run` loop drive it), this is called from *inside* a
    /// higher-order native (`Core.List.map`/`filter`/`reduce`) that needs the closure's result
    /// synchronously: it pushes the closure's frame, runs a nested loop until exactly that frame (and
    /// any frames it spawns) returns, then pops and returns the value left on the stack. The slot math
    /// mirrors `Op::CallValue`; execution shares `exec_op` with the main loop — one execution core, no
    /// second interpreter (the parity analogue of the tree-walker's `call_closure`). M-RT S7b-3.
    fn call_closure_value(&mut self, callee: &Value, args: Vec<Value>) -> Result<Value, String> {
        let (func_idx, captures) = match callee {
            Value::Closure(cd) => match cd.as_ref() {
                crate::value::ClosureData::Byte { func, captures } => (*func, captures.clone()),
                _ => return Err("expected a bytecode closure".to_string()),
            },
            v => return Err(format!("cannot call {} as a function", v.type_name())),
        };
        let func_arity = self.program.functions[func_idx].arity;
        let n_captures = self.program.functions[func_idx].n_captures;
        let n_params = func_arity - n_captures;
        if args.len() != n_params {
            return Err(format!(
                "wrong number of arguments: expected {n_params}, got {}",
                args.len()
            ));
        }
        if self.frames.len() >= MAX_CALL_DEPTH {
            return Err("stack overflow".to_string());
        }
        // Frame layout `[captures.., args..]` — identical to `Op::CallValue`.
        let slot_base = self.stack.len();
        self.stack.extend(captures);
        self.stack.extend(args);
        let target_depth = self.frames.len();
        self.frames.push(Frame {
            func: func_idx,
            ip: 0,
            slot_base,
        });
        self.run_until(target_depth)?;
        // The closure's `Return` (frames shrank back to `target_depth`, which is >= 1 so the caller
        // is non-empty) left its value on top of the stack via `do_return`.
        Ok(self.pop())
    }

    /// Drive `exec_op` until the frame stack shrinks back to `target_depth` (the depth *before* the
    /// frame to run was pushed). Used only by [`Vm::call_closure_value`] for re-entrant native
    /// callbacks; the top-level `run` loop is the `target_depth == 0` analogue that additionally
    /// captures output and returns it. A fault propagates as a raw `String` — the outer `run` loop
    /// (still executing the `CallNative` op) attaches the source line, exactly as for any native fault.
    fn run_until(&mut self, target_depth: usize) -> Result<(), String> {
        while self.frames.len() > target_depth {
            let fr = self.frames.len() - 1;
            let func = self.frames[fr].func;
            let ip = self.frames[fr].ip;
            let code = &self.program.functions[func].chunk.code;
            if ip >= code.len() {
                self.do_return(Value::Unit);
                continue;
            }
            let op = code[ip].clone();
            self.frames[fr].ip += 1;
            match self.exec_op(op, fr, func)? {
                Flow::Next => {}
                // `Flow::Done` is only ever returned by `main`'s `Return`; at `target_depth >= 1`
                // (always, since a native runs inside at least `main`) it is unreachable, but exit
                // cleanly rather than spin if a future caller passes `target_depth == 0`.
                Flow::Done => return Ok(()),
            }
        }
        Ok(())
    }

    fn pop(&mut self) -> Value {
        self.stack.pop().expect("vm stack underflow (compiler bug)")
    }

    /// Start index for popping the top `n` values. Real work in every build (`len - n`); the
    /// debug-only guard turns a compiler-bug underflow (which would wrap and then panic with a
    /// bare `index out of bounds`) into a labelled stack-desync assert. The compiler guarantees
    /// `n <= stack.len()`.
    fn pop_n_start(&self, n: usize) -> usize {
        debug_assert!(
            n <= self.stack.len(),
            "vm stack underflow: need {n} values, stack has {} (func {})",
            self.stack.len(),
            self.frames.last().map_or(usize::MAX, |f| f.func)
        );
        self.stack.len() - n
    }

    /// Absolute stack index of local `slot` within the frame whose window opens at `base`. The
    /// debug-only guard catches a slot outside the live locals window — the desync most likely to
    /// be introduced once P4/P5 mutate the stack as a GC root set — before the raw index panics.
    fn frame_slot(&self, base: usize, slot: usize) -> usize {
        let idx = base + slot;
        debug_assert!(
            idx < self.stack.len(),
            "vm local out of range: base {base} + slot {slot} = {idx} >= stack len {} (func {})",
            self.stack.len(),
            self.frames.last().map_or(usize::MAX, |f| f.func)
        );
        idx
    }

    /// Pop the top `n` values, returning them in stack order (bottom-most first).
    /// The compiler guarantees `n <= stack.len()`.
    fn split_off(&mut self, n: usize) -> Vec<Value> {
        let start = self.pop_n_start(n);
        self.stack.split_off(start)
    }

    /// Pop two ints in operand order: returns `(lhs, rhs)` for `lhs OP rhs`.
    fn pop2_int(&mut self) -> Result<(i64, i64), String> {
        let b = self.pop_int()?;
        let a = self.pop_int()?;
        Ok((a, b))
    }

    fn pop2_float(&mut self) -> Result<(f64, f64), String> {
        let b = self.pop_float()?;
        let a = self.pop_float()?;
        Ok((a, b))
    }

    fn pop_int(&mut self) -> Result<i64, String> {
        match self.pop() {
            Value::Int(n) => Ok(n),
            v => Err(format!("expected int, found {}", v.type_name())),
        }
    }

    fn pop_float(&mut self) -> Result<f64, String> {
        match self.pop() {
            Value::Float(x) => Ok(x),
            v => Err(format!("expected float, found {}", v.type_name())),
        }
    }

    /// Push the result of a checked integer kernel, propagating its fault body (e.g.
    /// `"integer overflow"`) verbatim — the fault string is single-sourced in `value`.
    fn push_i(&mut self, r: Result<i64, String>) -> Result<(), String> {
        self.stack.push(Value::Int(r?));
        Ok(())
    }
}

/// Ordering comparison for `Lt`/`Gt`/`Le`/`Ge` on int or float operands. The ordering and the
/// comparability fault are single-sourced in `value::compare_ord` (the interpreter calls the same
/// fn); only the `Op`→bool projection below is VM-local. NaN compares `false`.
fn compare(op: &Op, a: &Value, b: &Value) -> Result<bool, String> {
    use std::cmp::Ordering;
    Ok(match crate::value::compare_ord(a, b)? {
        Some(o) => match op {
            Op::Lt => o == Ordering::Less,
            Op::Gt => o == Ordering::Greater,
            Op::Le => o != Ordering::Greater,
            Op::Ge => o != Ordering::Less,
            _ => unreachable!("compare only called with Lt/Gt/Le/Ge"),
        },
        None => false, // NaN compares false
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::{BytecodeProgram, Chunk, Function, Op};
    use crate::value::Value;

    /// Compile a loose program through the real front-end (loader → check → compile) for tests that
    /// need a faithful `BytecodeProgram` rather than a hand-built one.
    fn compile_source(src: &str) -> BytecodeProgram {
        let unit = crate::loader::load_loose_src(src).unwrap();
        let checked = crate::cli::check_and_expand(&unit.program, &unit.diag_src).unwrap();
        crate::compiler::compile(&checked).unwrap()
    }

    #[test]
    fn vm_fault_carries_call_stack() {
        let program = compile_source(
            "package main;\n\
             function f() -> int { var xs = [1]; return xs[5]; }\n\
             function main() { var r = f(); }",
        );
        let err = Vm::new(&program).run().unwrap_err();
        assert_eq!(err.frames.len(), 2, "callee + main: {:?}", err.frames);
        assert_eq!(err.frames[0].function, "f");
        assert_eq!(err.frames[1].function, "main");
    }

    #[test]
    fn run_and_runvm_traces_match() {
        // The slice-1 invariant: a fault yields byte-identical trace text on both backends.
        for src in [
            "package main;\n\
             function g() -> int { var xs = [1]; return xs[9]; }\n\
             function main() { var r = g(); }",
            "package main;\nfunction main() { var x = 1 / 0; }",
        ] {
            let unit = crate::loader::load_loose_src(src).unwrap();
            let checked = crate::cli::check_and_expand(&unit.program, &unit.diag_src).unwrap();
            let interp_err = crate::interpreter::interpret(&checked).unwrap_err();
            let program = crate::compiler::compile(&checked).unwrap();
            let vm_err = Vm::new(&program).run().unwrap_err();
            assert_eq!(
                interp_err.render(""),
                vm_err.render(""),
                "run vs runvm trace text diverged for:\n{src}"
            );
        }
    }

    /// Emit the standard function terminator: push `Unit`, then `Return` (P3-7).
    fn term(c: &mut Chunk) {
        let u = c.add_const(Value::Unit);
        c.emit(Op::Const(u), 1);
        c.emit(Op::Return, 1);
    }

    /// Wrap a single hand-built chunk as `main` and run it. Renders the runtime `Diagnostic` to a
    /// string so the existing `.contains(...)` assertions on fault bodies keep working.
    fn run_chunk(chunk: Chunk) -> Result<String, String> {
        let program = BytecodeProgram {
            functions: vec![Function {
                name: "main".into(),
                arity: 0,
                n_captures: 0,
                chunk,
            }],
            main: 0,
            enum_descs: Vec::new(),
            class_descs: Vec::new(),
            names: Vec::new(),
            methods: HashMap::new(),
            class_implements: std::collections::BTreeMap::new(),
            static_inits: Vec::new(),
        };
        Vm::new(&program).run().map_err(|d| d.to_string())
    }

    /// Build a chunk for `2 * 3 + 4` then print it.
    fn arith_chunk() -> Chunk {
        let mut c = Chunk::new();
        let two = c.add_const(Value::Int(2));
        let three = c.add_const(Value::Int(3));
        let four = c.add_const(Value::Int(4));
        c.emit(Op::Const(two), 1);
        c.emit(Op::Const(three), 1);
        c.emit(Op::MulI, 1);
        c.emit(Op::Const(four), 1);
        c.emit(Op::AddI, 1);
        c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
        term(&mut c);
        c
    }

    #[test]
    fn run_rejects_invalid_bytecode_before_executing() {
        // Out-of-range const: `validate()` (run's first action) must fault cleanly, not panic.
        let mut c = Chunk::new();
        c.emit(Op::Const(42), 1); // empty const pool
        c.emit(Op::Return, 1);
        let err = run_chunk(c).unwrap_err();
        assert!(err.contains("invalid bytecode"), "{err}");
        assert!(err.contains("const index 42"), "{err}");
    }

    // Debug-only: `debug_assert!` is a no-op in release, so this `should_panic` test only holds
    // under `cfg(debug_assertions)`. A `GetLocal` past the (empty) main locals window passes
    // `validate()` — slots aren't statically checkable — and trips `frame_slot`'s guard.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "vm local out of range")]
    fn getlocal_past_window_trips_debug_assert() {
        let mut c = Chunk::new();
        c.emit(Op::GetLocal(5), 1);
        c.emit(Op::Return, 1);
        let _ = run_chunk(c);
    }

    #[test]
    fn runs_integer_arithmetic_and_prints() {
        let out = run_chunk(arith_chunk()).unwrap();
        assert_eq!(out, "10\n");
    }

    #[test]
    fn float_print_matches_interpreter_formatting() {
        // 1.5 + 2.5 = 4.0 -> rendered "4" via Rust `{}` (parity with value::as_display).
        let mut c = Chunk::new();
        let a = c.add_const(Value::Float(1.5));
        let b = c.add_const(Value::Float(2.5));
        c.emit(Op::Const(a), 1);
        c.emit(Op::Const(b), 1);
        c.emit(Op::AddF, 1);
        c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
        term(&mut c);
        assert_eq!(run_chunk(c).unwrap(), "4\n");
    }

    #[test]
    fn division_by_zero_is_runtime_error() {
        let mut c = Chunk::new();
        let a = c.add_const(Value::Int(1));
        let z = c.add_const(Value::Int(0));
        c.emit(Op::Const(a), 1);
        c.emit(Op::Const(z), 1);
        c.emit(Op::DivI, 1);
        term(&mut c);
        let err = run_chunk(c).unwrap_err();
        assert!(err.contains("division by zero"), "{err}");
    }

    #[test]
    fn negate_works_for_int_and_float() {
        let mut c = Chunk::new();
        let a = c.add_const(Value::Int(5));
        c.emit(Op::Const(a), 1);
        c.emit(Op::Neg, 1);
        c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
        term(&mut c);
        assert_eq!(run_chunk(c).unwrap(), "-5\n");
    }

    #[test]
    fn comparison_and_equality() {
        // 3 < 5  -> true
        let mut c = Chunk::new();
        let a = c.add_const(Value::Int(3));
        let b = c.add_const(Value::Int(5));
        c.emit(Op::Const(a), 1);
        c.emit(Op::Const(b), 1);
        c.emit(Op::Lt, 1);
        c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
        term(&mut c);
        assert_eq!(run_chunk(c).unwrap(), "true\n");
    }

    #[test]
    fn locals_get_and_set() {
        // local0 = 10; local0 = local0 + 5; print local0  -> 15
        let mut c = Chunk::new();
        let ten = c.add_const(Value::Int(10));
        let five = c.add_const(Value::Int(5));
        c.emit(Op::Const(ten), 1); // slot 0 (stays on stack)
        c.emit(Op::GetLocal(0), 1);
        c.emit(Op::Const(five), 1);
        c.emit(Op::AddI, 1);
        c.emit(Op::SetLocal(0), 1);
        c.emit(Op::GetLocal(0), 1);
        c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
        term(&mut c);
        assert_eq!(run_chunk(c).unwrap(), "15\n");
    }

    #[test]
    fn jump_if_false_skips_branch() {
        // if (false) print 1 else print 2  -> "2"
        let mut c = Chunk::new();
        let f = c.add_const(Value::Bool(false));
        let one = c.add_const(Value::Int(1));
        let two = c.add_const(Value::Int(2));
        c.emit(Op::Const(f), 1); // 0
        let jif = c.code.len();
        c.emit(Op::JumpIfFalse(0), 1); // 1 (patched below)
        c.emit(Op::Const(one), 1); // 2
        c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1); // 3
        let jend = c.code.len();
        c.emit(Op::Jump(0), 1); // 4 (patched below)
        let else_target = c.code.len(); // 5
        c.emit(Op::Const(two), 1); // 5
        c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1); // 6
        let end = c.code.len(); // 7 (start of the terminator)
        term(&mut c); // 7..9
        c.code[jif] = Op::JumpIfFalse(else_target);
        c.code[jend] = Op::Jump(end);
        assert_eq!(run_chunk(c).unwrap(), "2\n");
    }

    #[test]
    fn concat_renders_mixed_scalars() {
        // "x=" + 7  -> "x=7"
        let mut c = Chunk::new();
        let pre = c.add_const(Value::Str("x=".into()));
        let seven = c.add_const(Value::Int(7));
        c.emit(Op::Const(pre), 1);
        c.emit(Op::Const(seven), 1);
        c.emit(Op::Concat(2), 1);
        c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
        term(&mut c);
        assert_eq!(run_chunk(c).unwrap(), "x=7\n");
    }

    #[test]
    fn list_make_index_len() {
        // xs = [10, 20, 30]; print len(xs); print xs[1]  -> "3" then "20"
        let mut c = Chunk::new();
        let a = c.add_const(Value::Int(10));
        let b = c.add_const(Value::Int(20));
        let d = c.add_const(Value::Int(30));
        let one = c.add_const(Value::Int(1));
        c.emit(Op::Const(a), 1);
        c.emit(Op::Const(b), 1);
        c.emit(Op::Const(d), 1);
        c.emit(Op::MakeList(3), 1); // slot 0 = list
        c.emit(Op::GetLocal(0), 1);
        c.emit(Op::Len, 1);
        c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
        c.emit(Op::GetLocal(0), 1);
        c.emit(Op::Const(one), 1);
        c.emit(Op::Index, 1);
        c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
        term(&mut c);
        assert_eq!(run_chunk(c).unwrap(), "3\n20\n");
    }

    #[test]
    fn print_joins_multiple_args_with_space() {
        let mut c = Chunk::new();
        let a = c.add_const(Value::Str("a".into()));
        let b = c.add_const(Value::Int(1));
        c.emit(Op::Const(a), 1);
        c.emit(Op::Const(b), 1);
        c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 2), 1);
        term(&mut c);
        assert_eq!(run_chunk(c).unwrap(), "a 1\n");
    }

    #[test]
    fn call_runs_a_second_function_and_returns() {
        // main: push 7, Call(1), Print(1), term.   f(x): GetLocal(0), Return.
        let mut m = Chunk::new();
        let seven = m.add_const(Value::Int(7));
        m.emit(Op::Const(seven), 1);
        m.emit(Op::Call(1), 1);
        m.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
        term(&mut m);

        let mut f = Chunk::new();
        f.emit(Op::GetLocal(0), 1); // the single arg
        f.emit(Op::Return, 1);

        let program = BytecodeProgram {
            functions: vec![
                Function {
                    name: "main".into(),
                    arity: 0,
                    n_captures: 0,
                    chunk: m,
                },
                Function {
                    name: "f".into(),
                    arity: 1,
                    n_captures: 0,
                    chunk: f,
                },
            ],
            main: 0,
            enum_descs: Vec::new(),
            class_descs: Vec::new(),
            names: Vec::new(),
            methods: HashMap::new(),
            class_implements: std::collections::BTreeMap::new(),
            static_inits: Vec::new(),
        };
        assert_eq!(Vm::new(&program).run().unwrap(), "7\n");
    }

    #[test]
    fn make_enum_then_match_tag_and_get_field() {
        use crate::chunk::EnumDesc;
        // descs[0] = Opt::Some(int) (arity 1). Build:
        //   const 7; MakeEnum(0)          -> Some(7) becomes slot 0 (stays)
        //   GetLocal(0); MatchTag(0)      -> true        ; print
        //   GetLocal(0); GetEnumField(0)  -> 7           ; print
        let mut c = Chunk::new();
        let seven = c.add_const(Value::Int(7));
        c.emit(Op::Const(seven), 1);
        c.emit(Op::MakeEnum(0), 1);
        c.emit(Op::GetLocal(0), 1);
        c.emit(Op::MatchTag(0), 1);
        c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
        c.emit(Op::GetLocal(0), 1);
        c.emit(Op::GetEnumField(0), 1);
        c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
        term(&mut c);
        let program = BytecodeProgram {
            functions: vec![Function {
                name: "main".into(),
                arity: 0,
                n_captures: 0,
                chunk: c,
            }],
            main: 0,
            enum_descs: vec![EnumDesc {
                ty: "Opt".into(),
                variant: "Some".into(),
                arity: 1,
            }],
            class_descs: Vec::new(),
            names: Vec::new(),
            methods: HashMap::new(),
            class_implements: std::collections::BTreeMap::new(),
            static_inits: Vec::new(),
        };
        assert_eq!(Vm::new(&program).run().unwrap(), "true\n7\n");
    }

    #[test]
    fn match_tag_is_false_for_a_different_variant() {
        use crate::chunk::EnumDesc;
        // Build a `None` (desc 0), then test it against the `Some` tag (desc 1) -> false.
        let mut c = Chunk::new();
        c.emit(Op::MakeEnum(0), 1); // None (arity 0) -> slot 0
        c.emit(Op::GetLocal(0), 1);
        c.emit(Op::MatchTag(1), 1); // is it `Some`? -> false
        c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
        term(&mut c);
        let program = BytecodeProgram {
            functions: vec![Function {
                name: "main".into(),
                arity: 0,
                n_captures: 0,
                chunk: c,
            }],
            main: 0,
            enum_descs: vec![
                EnumDesc {
                    ty: "Opt".into(),
                    variant: "None".into(),
                    arity: 0,
                },
                EnumDesc {
                    ty: "Opt".into(),
                    variant: "Some".into(),
                    arity: 1,
                },
            ],
            class_descs: Vec::new(),
            names: Vec::new(),
            methods: HashMap::new(),
            class_implements: std::collections::BTreeMap::new(),
            static_inits: Vec::new(),
        };
        assert_eq!(Vm::new(&program).run().unwrap(), "false\n");
    }

    #[test]
    fn make_instance_then_get_field() {
        use crate::chunk::ClassDesc;
        // class Point { x, y }: build Point(3, 4) into slot 0, then read `.x` (names[0]) -> 3.
        let mut c = Chunk::new();
        let three = c.add_const(Value::Int(3));
        let four = c.add_const(Value::Int(4));
        c.emit(Op::Const(three), 1);
        c.emit(Op::Const(four), 1);
        c.emit(Op::MakeInstance(0), 1); // [Point{x:3,y:4}] becomes slot 0
        c.emit(Op::GetLocal(0), 1);
        c.emit(Op::GetField(0), 1); // names[0] == "x"
        c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
        term(&mut c);
        let program = BytecodeProgram {
            functions: vec![Function {
                name: "main".into(),
                arity: 0,
                n_captures: 0,
                chunk: c,
            }],
            main: 0,
            enum_descs: Vec::new(),
            class_descs: vec![ClassDesc {
                class: "Point".into(),
                fields: vec!["x".into(), "y".into()],
            }],
            names: vec!["x".into()],
            methods: HashMap::new(),
            class_implements: std::collections::BTreeMap::new(),
            static_inits: Vec::new(),
        };
        assert_eq!(Vm::new(&program).run().unwrap(), "3\n");
    }

    #[test]
    fn get_field_absent_faults_like_interpreter() {
        use crate::chunk::ClassDesc;
        // Empty instance, read missing field `tag` (names[0]) -> `no field` fault (parity).
        let mut c = Chunk::new();
        c.emit(Op::MakeInstance(0), 1); // [Empty{}] slot 0
        c.emit(Op::GetLocal(0), 1);
        c.emit(Op::GetField(0), 1);
        c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
        term(&mut c);
        let program = BytecodeProgram {
            functions: vec![Function {
                name: "main".into(),
                arity: 0,
                n_captures: 0,
                chunk: c,
            }],
            main: 0,
            enum_descs: Vec::new(),
            class_descs: vec![ClassDesc {
                class: "Empty".into(),
                fields: Vec::new(),
            }],
            names: vec!["tag".into()],
            methods: HashMap::new(),
            class_implements: std::collections::BTreeMap::new(),
            static_inits: Vec::new(),
        };
        let err = Vm::new(&program).run().unwrap_err().to_string();
        assert!(err.contains("no field `tag` on `Empty`"), "{err}");
    }
}
