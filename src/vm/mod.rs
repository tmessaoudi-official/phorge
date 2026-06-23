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

/// A live exception handler (M-faults 2b): the catch landing pad to jump to, plus the frame depth
/// and stack height to unwind to when a `Throw` reaches it. Pushed by `Op::PushHandler`, removed by
/// `Op::PopHandler` or consumed by an unwind.
struct Handler {
    catch_ip: usize,
    frame_depth: usize,
    stack_height: usize,
}

/// Display name of a thrown value for an uncaught-exception message — its class for an instance,
/// else its type name (M-faults 2b; mirrors the interpreter's `throw_what` for trace parity).
fn throw_display(v: &Value) -> String {
    match v {
        Value::Instance(inst) => inst.class.clone(),
        other => other.type_name().to_string(),
    }
}

pub struct Vm<'a> {
    program: &'a BytecodeProgram,
    stack: Vec<Value>,
    frames: Vec<Frame>,
    /// Program-lifetime `static` field storage (M-mut.7), indexed by `Op::GetStatic`/`SetStatic`.
    /// Seeded once from `program.static_inits` (the once-at-load literal values).
    statics: Vec<Value>,
    out: String,
    /// Active exception handlers, innermost last (M-faults 2b). `Op::PushHandler`/`PopHandler`
    /// maintain it; a `Throw` unwinds to the topmost handler owned by the current run loop.
    handlers: Vec<Handler>,
    /// Carries a thrown value alongside the [`THROW_SENTINEL`] fault (the analogue of the
    /// interpreter's field): set by `Op::Throw`, taken by the unwind that lands it or by the
    /// uncaught-exception path (M-faults 2b).
    pending_throw: Option<Value>,
}

// cohesion split (M-Decomp W4): exec/closure clusters.
mod closure;
mod exec;

impl<'a> Vm<'a> {
    pub fn new(program: &'a BytecodeProgram) -> Self {
        Self {
            program,
            stack: Vec::new(),
            frames: Vec::new(),
            statics: program.static_inits.clone(),
            out: String::new(),
            handlers: Vec::new(),
            pending_throw: None,
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
                    // A thrown exception: unwind to the nearest handler (any handler, floor 0) and
                    // resume at its catch landing pad. If none exists, it escapes `main` uncaught —
                    // fall through to the diagnostic path below with an uncaught-exception body
                    // (the checker's `E-UNCAUGHT-THROW` makes this unreachable for a checked program).
                    if msg == crate::chunk::THROW_SENTINEL && self.unwind_throw(0) {
                        continue;
                    }
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
                    // An uncaught throw (no handler matched above) surfaces as an uncaught-exception
                    // fault naming the thrown type; any other `msg` is a plain runtime fault.
                    let body = if msg == crate::chunk::THROW_SENTINEL {
                        let what = self
                            .pending_throw
                            .take()
                            .map_or_else(|| "?".to_string(), |v| throw_display(&v));
                        format!("uncaught exception `{what}`")
                    } else {
                        msg
                    };
                    return Err(Diagnostic::runtime_at_line(body, line).with_frames(frames));
                }
            }
        }
    }

    /// Try to unwind a pending `Throw` to the topmost handler owned by the current run loop (one
    /// whose `frame_depth` is above `floor`: `0` for the main loop, `target_depth` for a re-entrant
    /// `run_until`). On success, truncate frames + stack to the handler's marks, push the thrown
    /// value, jump the landed frame to the catch landing pad, and return `true` (ready to resume).
    /// On no handler, leave all state — including `pending_throw` — untouched and return `false`
    /// (the caller propagates the sentinel / reports an uncaught exception). M-faults 2b.
    fn unwind_throw(&mut self, floor: usize) -> bool {
        match self.handlers.last() {
            Some(h) if h.frame_depth > floor => {
                let h = self.handlers.pop().expect("checked by the guard");
                let v = self
                    .pending_throw
                    .take()
                    .expect("the throw sentinel implies a pending value");
                self.frames.truncate(h.frame_depth);
                self.stack.truncate(h.stack_height);
                self.stack.push(v);
                let top = self.frames.len() - 1;
                self.frames[top].ip = h.catch_ip;
                true
            }
            _ => false,
        }
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
        // Drop any handler that belonged to the frame just popped (defensive: the compiler emits a
        // `PopHandler` on every normal/transfer exit from a try body, so a live handler should not
        // outlive its frame — but a future codegen path must never leave a dangling handler that a
        // later `Throw` could wrongly catch). M-faults 2b.
        let depth = self.frames.len();
        self.handlers.retain(|h| h.frame_depth <= depth);
        if !self.frames.is_empty() {
            self.stack.push(rv);
        }
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
mod tests;
