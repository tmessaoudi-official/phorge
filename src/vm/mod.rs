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

/// The process exit code carried by `main`'s return value (Batch-1 B; mirrors the interpreter's
/// `exit_code_of`): the `int` it returned, or `0` for `Value::Unit` (a `void` `main`).
fn exit_code_of(v: &Value) -> i64 {
    match v {
        Value::Int(n) => *n,
        _ => 0,
    }
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
    /// `main`'s return value, captured by `Op::Return` when the entry frame is about to pop (Batch-1
    /// B). `do_return` discards a return value once the stack is empty, so it is stashed here first;
    /// [`run_main`](Vm::run_main) reads its `int` (or `0` for `Value::Unit`) as the process exit code.
    exit_value: Value,
    /// Per-site monomorphic **inline caches** for `Op::GetField`/`SetField` (M-perf S2), indexed
    /// `[func][ip]`. A site that repeatedly reads one class hits `(layout_ptr, slot)` and skips the
    /// `name → slot` hash entirely (and the name clone); a class change at the site refills it. The
    /// key is the receiver's `ClassLayout` pointer — every instance of one class shares that `Rc`, so
    /// a raw-pointer compare is an exact, allocation-free monomorphism test. Classes are immutable, so
    /// a filled entry never goes stale. Built empty per run (a fresh `Vm`), so no cross-run leakage.
    field_caches: Vec<Vec<FieldCache>>,
    /// Green-thread coordination (M6 W4): the scheduler/id-allocator + finished-task results. Owned
    /// per-`Vm` in the synchronous-degenerate path (`spawn` runs eagerly, storing its result here;
    /// `join` reads it); the cooperative driver shares one `Coop` across every task-`Vm` instead.
    /// `Value::Channel`/`Task` carry the `ChanId`/`TaskId` allocated from this scheduler.
    coop: std::rc::Rc<std::cell::RefCell<crate::green::exec::Coop>>,
    /// Green-thread cooperative cutover (S4.3): the suspension handle of the coroutine *this* task-VM
    /// runs on. `Some` only inside the cooperative driver ([`coop::run_cooperative_vm`]); `None` on the
    /// ordinary synchronous run path, where `spawn` is eager and `recv`/`join` fault. The borrow is a
    /// closure-local of the driver's coroutine body (it shares the program-borrow lifetime `'a`).
    coop_suspend: Option<&'a dyn crate::green::exec::Suspend>,
    /// The owning bytecode program, held only by a **cooperative** task-VM so a `SpawnCall` can build a
    /// fresh child task-VM coroutine (which captures a `'static` `Rc`). `None` on the synchronous path.
    /// The `program` field above borrows from this same program; this `Rc` keeps it alive for the
    /// coroutine and clones cheaply into each spawned child.
    #[cfg_attr(
        not(all(feature = "green", not(target_arch = "wasm32"))),
        allow(dead_code)
    )]
    program_rc: Option<std::rc::Rc<BytecodeProgram>>,
}

/// One inline-cache slot (M-perf S2): the `ClassLayout` pointer last seen at a field site and the
/// field's resolved slot. The `(null, _)` value is the never-filled sentinel (no real `Rc` is null).
/// Only ever *compared* by pointer — never dereferenced — so it carries no aliasing/lifetime hazard.
type FieldCache = std::cell::Cell<(*const crate::value::ClassLayout, u32)>;

// cohesion split (M-Decomp W4): exec/closure clusters.
mod closure;
mod exec;

// Cooperative green-thread driver (M6 W4 / S4.3) — native + `green` only (stackful coroutines, which
// corosensei cannot provide on wasm32; the wasm VM keeps the eager model).
#[cfg(all(feature = "green", not(target_arch = "wasm32")))]
mod coop;
#[cfg(all(feature = "green", not(target_arch = "wasm32")))]
pub use coop::run_cooperative_vm;

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
            exit_value: Value::Unit,
            // One cache cell per op in every function (sparse use — only field sites read it — but a
            // one-time, per-run allocation keyed directly by `ip`, so the hot path is a flat index).
            field_caches: program
                .functions
                .iter()
                .map(|f| {
                    f.chunk
                        .code
                        .iter()
                        .map(|_| std::cell::Cell::new((std::ptr::null(), 0u32)))
                        .collect()
                })
                .collect(),
            coop: std::rc::Rc::new(std::cell::RefCell::new(crate::green::exec::Coop::new())),
            coop_suspend: None,
            program_rc: None,
        }
    }

    /// Build a VM for a cooperative green task (S4.3): same as [`new`](Vm::new) but with a suspension
    /// handle, sharing `coop` (the scheduler + task results + spawn queue) with every sibling task so
    /// the shared `green::sched` kernel drives identical interleaving on both backends. The caller sets
    /// [`program_rc`](Vm::program_rc) so this task can itself `spawn`. Native + `green` only.
    #[cfg(all(feature = "green", not(target_arch = "wasm32")))]
    pub(super) fn new_cooperative(
        program: &'a BytecodeProgram,
        coop: std::rc::Rc<std::cell::RefCell<crate::green::exec::Coop>>,
        suspend: &'a dyn crate::green::exec::Suspend,
    ) -> Self {
        let mut vm = Vm::new(program);
        vm.coop = coop;
        vm.coop_suspend = Some(suspend);
        vm
    }

    /// Execute the program from `main`, returning captured output (`Ok`) or a runtime
    /// error (`Err`). Stdout-only wrapper over [`run_main`](Vm::run_main) — preserves every existing
    /// caller and the differential harness (which gates stdout identity).
    pub fn run(self) -> Result<String, Diagnostic> {
        self.run_main().map(|(out, _exit)| out)
    }

    /// Like [`run`](Vm::run), but also returns `main`'s exit code (Batch-1 B): the `int` it returned,
    /// or `0` for a `void` `main`. The CLI maps it to the process exit status.
    pub fn run_main(mut self) -> Result<(String, i64), Diagnostic> {
        // Fail fast on malformed bytecode (a compiler bug) with a clean error instead of a panic
        // mid-execution — keeps the no-crash contract (EV-7). See `BytecodeProgram::validate`.
        // Bytecode-validation faults have no source line, so they surface position-less.
        self.program.validate().map_err(Diagnostic::runtime)?;
        // Batch-1 B/D: lay out the entry frame's slots. A class-static entry's compiled `Function`
        // reserves slot 0 for `$this` (a dummy receiver — a static method never reads it), so push a
        // placeholder first. Then a one-parameter `main` receives the program argv as the next slot;
        // a zero-parameter `main` gets nothing. `E-MAIN-SIGNATURE` bounds the params to 0 or 1.
        if self.program.main_is_static {
            self.stack.push(Value::Unit);
        }
        if self.program.main_params == 1 {
            self.stack.push(crate::native::process_args_value());
        }
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
                    return Ok((self.out, exit_code_of(&self.exit_value)));
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
                Ok(Flow::Done) => return Ok((self.out, exit_code_of(&self.exit_value))),
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

    /// Pop two raw values in operand order: returns `(lhs, rhs)` for `lhs OP rhs`. Used by the decimal
    /// ops (M-NUM S1), whose kernel coerces a mixed `Decimal`/`Int` pair itself (no per-type pop).
    fn pop2(&mut self) -> (Value, Value) {
        let b = self.pop();
        let a = self.pop();
        (a, b)
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
    /// Push a fallible `f64` result, propagating a zero-divisor fault (`float_div`/`float_rem`). The
    /// `?` turns the kernel's fault body into the VM fault, byte-identical to the interpreter.
    fn push_f(&mut self, r: Result<f64, String>) -> Result<(), String> {
        self.stack.push(Value::Float(r?));
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
