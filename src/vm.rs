//! Stack VM that executes a `Chunk`. See docs/specs/2026-06-15-m2-bytecode-vm-design.md (§6).
//! P1: scalar arithmetic, negate, print, return. Output is captured into a String (mirrors
//! `interpreter::interpret`) so the VM can be differential-tested against the tree-walker.
//!
//! `Err` carries the bare runtime-error message (no "runtime error:" prefix), matching
//! `interpreter`'s `RuntimeError.message`; the future `cmd_runvm` adds the prefix, keeping
//! error parity with `cmd_run`.

use crate::chunk::{BytecodeProgram, Op};
use crate::value::{Value, MAX_CALL_DEPTH};

// Call-frame depth is capped by the shared `value::MAX_CALL_DEPTH` (same limit the interpreter
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
    out: String,
}

impl<'a> Vm<'a> {
    pub fn new(program: &'a BytecodeProgram) -> Self {
        Self {
            program,
            stack: Vec::new(),
            frames: Vec::new(),
            out: String::new(),
        }
    }

    /// Execute the program from `main`, returning captured output (`Ok`) or a runtime
    /// error (`Err`).
    pub fn run(mut self) -> Result<String, String> {
        // Fail fast on malformed bytecode (a compiler bug) with a clean error instead of a panic
        // mid-execution — keeps the no-crash contract (EV-7). See `BytecodeProgram::validate`.
        self.program.validate()?;
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
                    self.stack.push(Value::List(items));
                }
                Op::Index => {
                    let idx = match self.pop() {
                        Value::Int(n) => n,
                        v => return Err(format!("expected int index, found {}", v.type_name())),
                    };
                    let list = match self.pop() {
                        Value::List(xs) => xs,
                        v => return Err(format!("cannot index {}", v.type_name())),
                    };
                    let i = usize::try_from(idx)
                        .ok()
                        .filter(|i| *i < list.len())
                        .ok_or_else(|| "list index out of range".to_string())?;
                    self.stack.push(list[i].clone());
                }
                Op::Len => match self.pop() {
                    Value::List(xs) => self.stack.push(Value::Int(xs.len() as i64)),
                    v => return Err(format!("cannot take length of {}", v.type_name())),
                },

                Op::Print(n) => {
                    let parts = self.split_off(n);
                    let mut line = String::new();
                    for (i, v) in parts.iter().enumerate() {
                        if i > 0 {
                            line.push(' ');
                        }
                        match v.as_display() {
                            Some(t) => line.push_str(&t),
                            None => return Err(format!("println cannot print {}", v.type_name())),
                        }
                    }
                    self.out.push_str(&line);
                    self.out.push('\n');
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
                        return Ok(self.out);
                    }
                }
            }
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
mod tests {
    use super::*;
    use crate::chunk::{BytecodeProgram, Chunk, Function, Op};
    use crate::value::Value;

    /// Emit the standard function terminator: push `Unit`, then `Return` (P3-7).
    fn term(c: &mut Chunk) {
        let u = c.add_const(Value::Unit);
        c.emit(Op::Const(u), 1);
        c.emit(Op::Return, 1);
    }

    /// Wrap a single hand-built chunk as `main` and run it.
    fn run_chunk(chunk: Chunk) -> Result<String, String> {
        let program = BytecodeProgram {
            functions: vec![Function {
                name: "main".into(),
                arity: 0,
                chunk,
            }],
            main: 0,
        };
        Vm::new(&program).run()
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
        c.emit(Op::Print(1), 1);
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
        c.emit(Op::Print(1), 1);
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
        c.emit(Op::Print(1), 1);
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
        c.emit(Op::Print(1), 1);
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
        c.emit(Op::Print(1), 1);
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
        c.emit(Op::Print(1), 1); // 3
        let jend = c.code.len();
        c.emit(Op::Jump(0), 1); // 4 (patched below)
        let else_target = c.code.len(); // 5
        c.emit(Op::Const(two), 1); // 5
        c.emit(Op::Print(1), 1); // 6
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
        c.emit(Op::Print(1), 1);
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
        c.emit(Op::Print(1), 1);
        c.emit(Op::GetLocal(0), 1);
        c.emit(Op::Const(one), 1);
        c.emit(Op::Index, 1);
        c.emit(Op::Print(1), 1);
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
        c.emit(Op::Print(2), 1);
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
        m.emit(Op::Print(1), 1);
        term(&mut m);

        let mut f = Chunk::new();
        f.emit(Op::GetLocal(0), 1); // the single arg
        f.emit(Op::Return, 1);

        let program = BytecodeProgram {
            functions: vec![
                Function {
                    name: "main".into(),
                    arity: 0,
                    chunk: m,
                },
                Function {
                    name: "f".into(),
                    arity: 1,
                    chunk: f,
                },
            ],
            main: 0,
        };
        assert_eq!(Vm::new(&program).run().unwrap(), "7\n");
    }
}
