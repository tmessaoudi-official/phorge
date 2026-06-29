//! Bytecode VM — closure (M-Decomp W4). See mod.rs for the struct + core + entry points.

use super::*;

impl<'a> Vm<'a> {
    /// Invoke a first-class closure VALUE re-entrantly and return its result. Unlike [`Op::CallValue`]
    /// (which pushes a frame and lets the main `run` loop drive it), this is called from *inside* a
    /// higher-order native (`Core.List.map`/`filter`/`reduce`) that needs the closure's result
    /// synchronously: it pushes the closure's frame, runs a nested loop until exactly that frame (and
    /// any frames it spawns) returns, then pops and returns the value left on the stack. The slot math
    /// mirrors `Op::CallValue`; execution shares `exec_op` with the main loop — one execution core, no
    /// second interpreter (the parity analogue of the tree-walker's `call_closure`). M-RT S7b-3.
    pub(super) fn call_closure_value(
        &mut self,
        callee: &Value,
        args: Vec<Value>,
    ) -> Result<Value, String> {
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

    /// Invoke a plain (capture-less) function by its table index re-entrantly and return its result —
    /// the `Op::SpawnCall` analogue of [`call_closure_value`] (S4.3). Used by the eager `spawn` path to
    /// run a free function inline, and by the cooperative driver to run a task's root call. Pushes the
    /// function's frame `[args..]` and drives a nested `run_until` to exactly that frame's return.
    pub(super) fn call_function_value(
        &mut self,
        func_idx: usize,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        let func_arity = self.program.functions[func_idx].arity;
        if args.len() != func_arity {
            return Err(format!(
                "wrong number of arguments: expected {func_arity}, got {}",
                args.len()
            ));
        }
        if self.frames.len() >= MAX_CALL_DEPTH {
            return Err("stack overflow".to_string());
        }
        let slot_base = self.stack.len();
        self.stack.extend(args);
        let target_depth = self.frames.len();
        self.frames.push(Frame {
            func: func_idx,
            ip: 0,
            slot_base,
        });
        self.run_until(target_depth)?;
        Ok(self.pop())
    }

    /// Drive `exec_op` until the frame stack shrinks back to `target_depth` (the depth *before* the
    /// frame to run was pushed). Used only by [`Vm::call_closure_value`] for re-entrant native
    /// callbacks; the top-level `run` loop is the `target_depth == 0` analogue that additionally
    /// captures output and returns it. A fault propagates as a raw `String` — the outer `run` loop
    /// (still executing the `CallNative` op) attaches the source line, exactly as for any native fault.
    pub(super) fn run_until(&mut self, target_depth: usize) -> Result<(), String> {
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
            match self.exec_op(op, fr, func) {
                Ok(Flow::Next) => {}
                // `Flow::Done` is only ever returned by `main`'s `Return`; at `target_depth >= 1`
                // (always, since a native runs inside at least `main`) it is unreachable, but exit
                // cleanly rather than spin if a future caller passes `target_depth == 0`.
                Ok(Flow::Done) => return Ok(()),
                Err(msg) => {
                    // A throw raised inside this re-entrant call: unwind to a handler owned by *this*
                    // call (frame_depth above `target_depth`, i.e. a `try` inside the closure). If
                    // none exists, the throw escapes the closure — propagate the sentinel (with
                    // `pending_throw` intact) so the native returns it and the outer `run` loop
                    // unwinds to the `try` surrounding the higher-order call (M-faults 2b).
                    if msg == crate::chunk::THROW_SENTINEL && self.unwind_throw(target_depth) {
                        continue;
                    }
                    return Err(msg);
                }
            }
        }
        Ok(())
    }
}
