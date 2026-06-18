# M2 Plan 1 — VM Core (Chunk + Instruction Set + Dispatch Loop) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (or
> subagent-driven-development) to implement this plan task-by-task. Steps use checkbox
> (`- [ ]`) syntax. Inline execution on `master` (ask-human-gate deadlocks subagents).

**Goal:** Stand up the bytecode runtime skeleton — a `Chunk` (instructions + constant
pool + line table), a typed instruction `enum Op`, and a stack VM that executes a
*hand-built* chunk doing scalar arithmetic and `print`. No compiler yet (that is P2);
the runnable proof is a VM unit test executing a hand-assembled chunk.

**Architecture:** Two new flat modules — `src/chunk.rs` (data) and `src/vm.rs` (execution)
— wired into `src/lib.rs`. The VM reuses `value::Value` for scalars, which gives output
formatting parity with the interpreter for free (`as_display`); the VM-specific heap/handle
object model is introduced later (P4) when compound objects appear. The VM captures output
into a `String` (mirroring `interpreter::interpret`) so it can be differential-tested
against the tree-walker from P2 onward.

**Tech Stack:** Rust (edition 2021), std only. `export PATH=/stack/tools/cargo/bin:$PATH`.
Test/lint: `cargo test`, `cargo clippy --all-targets`.

**Design reference:** `docs/specs/2026-06-15-m2-bytecode-vm-design.md` (§4 bytecode format,
§5 instruction set, §6 VM model). Decisions M2-5 (`enum Value`), M2-7 (typed `enum Op`,
not raw bytes), M2-8 (stack VM).

> **Parity note (conscious deviation):** the spec's VM `Value` carries heap *handles* for
> compound objects (M2-4). P1 exercises only scalars, so it reuses `value::Value` directly
> — fully consistent with M2-5 (tagged-union value) and avoids duplicating float-formatting
> logic. Handles are introduced in P4, where compound objects first need heap storage.

---

### Task 1: Chunk + instruction set (`src/chunk.rs`)

**Files:**
- Create: `src/chunk.rs`
- Modify: `src/lib.rs` (add `pub mod chunk;`)

- [ ] **Step 1: Write the failing test**

Add to the bottom of the new `src/chunk.rs` (write the file with the test first, empty impl):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;

    #[test]
    fn add_const_returns_sequential_indices() {
        let mut c = Chunk::new();
        assert_eq!(c.add_const(Value::Int(1)), 0);
        assert_eq!(c.add_const(Value::Int(2)), 1);
        assert_eq!(c.consts.len(), 2);
    }

    #[test]
    fn emit_tracks_code_and_lines() {
        let mut c = Chunk::new();
        c.emit(Op::Const(0), 1);
        c.emit(Op::Return, 2);
        assert_eq!(c.code.len(), 2);
        assert_eq!(c.lines, vec![1, 2]);
    }
}
```

- [ ] **Step 2: Write the implementation** (top of `src/chunk.rs`)

```rust
//! Bytecode chunk + instruction set for the M2 VM.
//! See docs/specs/2026-06-15-m2-bytecode-vm-design.md (§4, §5).
//! P1 scope: scalar arithmetic + print. Reuses `value::Value` (scalar formatting parity
//! with the interpreter is free); the VM heap/handle object model arrives in P4.

use crate::value::Value;

/// One VM instruction. Typed operands — no raw-byte decode (decision M2-7).
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Push `consts[idx]`.
    Const(usize),
    // Type-specialized arithmetic (the checker guarantees operand types).
    AddI,
    SubI,
    MulI,
    DivI,
    AddF,
    SubF,
    MulF,
    DivF,
    /// Negate the top of stack (int or float).
    Neg,
    /// Pop, render, append a line to captured output.
    Print,
    /// End execution, returning captured output.
    Return,
}

/// A unit of compiled bytecode: instructions, a constant pool, and a per-instruction
/// source-line table (for runtime-error reporting).
#[derive(Debug, Clone, Default)]
pub struct Chunk {
    pub code: Vec<Op>,
    pub consts: Vec<Value>,
    pub lines: Vec<u32>,
}

impl Chunk {
    pub fn new() -> Self {
        Self::default()
    }

    /// Intern a constant, returning its pool index.
    pub fn add_const(&mut self, v: Value) -> usize {
        self.consts.push(v);
        self.consts.len() - 1
    }

    /// Append an instruction tagged with its source line.
    pub fn emit(&mut self, op: Op, line: u32) {
        self.code.push(op);
        self.lines.push(line);
    }
}
```

Then add to `src/lib.rs` after `pub mod value;`:

```rust
pub mod chunk;
```

- [ ] **Step 3: Run the tests**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test chunk`
Expected: the two `chunk::tests` pass.

- [ ] **Step 4: Commit**

```bash
git add src/chunk.rs src/lib.rs
git commit -m "feat(vm): Chunk + typed Op instruction set (M2 P1)"
```

---

### Task 2: Stack VM dispatch loop (`src/vm.rs`)

**Files:**
- Create: `src/vm.rs`
- Modify: `src/lib.rs` (add `pub mod vm;`)

- [ ] **Step 1: Write the failing tests** (bottom of new `src/vm.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::{Chunk, Op};
    use crate::value::Value;

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
        c.emit(Op::Print, 1);
        c.emit(Op::Return, 1);
        c
    }

    #[test]
    fn runs_integer_arithmetic_and_prints() {
        let out = Vm::new(&arith_chunk()).run().unwrap();
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
        c.emit(Op::Print, 1);
        c.emit(Op::Return, 1);
        assert_eq!(Vm::new(&c).run().unwrap(), "4\n");
    }

    #[test]
    fn division_by_zero_is_runtime_error() {
        let mut c = Chunk::new();
        let a = c.add_const(Value::Int(1));
        let z = c.add_const(Value::Int(0));
        c.emit(Op::Const(a), 1);
        c.emit(Op::Const(z), 1);
        c.emit(Op::DivI, 1);
        c.emit(Op::Return, 1);
        let err = Vm::new(&c).run().unwrap_err();
        assert!(err.contains("division by zero"), "{err}");
    }

    #[test]
    fn negate_works_for_int_and_float() {
        let mut c = Chunk::new();
        let a = c.add_const(Value::Int(5));
        c.emit(Op::Const(a), 1);
        c.emit(Op::Neg, 1);
        c.emit(Op::Print, 1);
        c.emit(Op::Return, 1);
        assert_eq!(Vm::new(&c).run().unwrap(), "-5\n");
    }
}
```

- [ ] **Step 2: Write the implementation** (top of `src/vm.rs`)

```rust
//! Stack VM that executes a `Chunk`. See docs/specs/2026-06-15-m2-bytecode-vm-design.md (§6).
//! P1: scalar arithmetic, negate, print, return. Output is captured into a String (mirrors
//! `interpreter::interpret`) so the VM can be differential-tested against the tree-walker.
//!
//! `Err` carries the bare runtime-error message (no "runtime error:" prefix), matching
//! `interpreter`'s `RuntimeError.message`; the future `cmd_runvm` adds the prefix, keeping
//! error parity with `cmd_run`.

use crate::chunk::{Chunk, Op};
use crate::value::Value;

pub struct Vm<'a> {
    chunk: &'a Chunk,
    stack: Vec<Value>,
    out: String,
}

impl<'a> Vm<'a> {
    pub fn new(chunk: &'a Chunk) -> Self {
        Self { chunk, stack: Vec::new(), out: String::new() }
    }

    /// Execute the chunk, returning captured output (`Ok`) or a runtime error (`Err`).
    pub fn run(mut self) -> Result<String, String> {
        let mut ip = 0;
        while ip < self.chunk.code.len() {
            // Clone the op (cheap) so we don't hold a borrow of `self.chunk` across the
            // mutable stack operations below.
            let op = self.chunk.code[ip].clone();
            ip += 1;
            match op {
                Op::Const(i) => self.stack.push(self.chunk.consts[i].clone()),

                Op::AddI => { let (a, b) = self.pop2_int()?; self.push_i(a.checked_add(b))?; }
                Op::SubI => { let (a, b) = self.pop2_int()?; self.push_i(a.checked_sub(b))?; }
                Op::MulI => { let (a, b) = self.pop2_int()?; self.push_i(a.checked_mul(b))?; }
                Op::DivI => {
                    let (a, b) = self.pop2_int()?;
                    if b == 0 {
                        return Err("division by zero".to_string());
                    }
                    self.push_i(a.checked_div(b))?;
                }

                Op::AddF => { let (a, b) = self.pop2_float()?; self.stack.push(Value::Float(a + b)); }
                Op::SubF => { let (a, b) = self.pop2_float()?; self.stack.push(Value::Float(a - b)); }
                Op::MulF => { let (a, b) = self.pop2_float()?; self.stack.push(Value::Float(a * b)); }
                Op::DivF => { let (a, b) = self.pop2_float()?; self.stack.push(Value::Float(a / b)); }

                Op::Neg => match self.pop() {
                    Value::Int(n) => self.stack.push(Value::Int(-n)),
                    Value::Float(x) => self.stack.push(Value::Float(-x)),
                    v => return Err(format!("cannot negate {}", v.type_name())),
                },

                Op::Print => {
                    let v = self.pop();
                    let s = v
                        .as_display()
                        .ok_or_else(|| format!("cannot print {}", v.type_name()))?;
                    self.out.push_str(&s);
                    self.out.push('\n');
                }

                Op::Return => return Ok(self.out),
            }
        }
        Ok(self.out)
    }

    fn pop(&mut self) -> Value {
        self.stack.pop().expect("vm stack underflow (compiler bug)")
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

    fn push_i(&mut self, r: Option<i64>) -> Result<(), String> {
        let n = r.ok_or_else(|| "integer overflow".to_string())?;
        self.stack.push(Value::Int(n));
        Ok(())
    }
}
```

Then add to `src/lib.rs` after `pub mod chunk;`:

```rust
pub mod vm;
```

- [ ] **Step 3: Run the tests**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test vm`
Expected: the four `vm::tests` pass.

- [ ] **Step 4: Full suite + clippy**

Run: `cargo test && cargo clippy --all-targets`
Expected: all green (≈ 168 tests), clippy exit 0, no warnings.
(Note: rtk tee swallows the cargo summary on success — grep `running N tests` / trust exit code.)

- [ ] **Step 5: Commit**

```bash
git add src/vm.rs src/lib.rs
git commit -m "feat(vm): stack VM dispatch loop — scalar arithmetic + print (M2 P1)"
```

---

## Acceptance criteria (P1 done)

- `Chunk`/`Op` defined; VM executes a hand-built chunk.
- `2 * 3 + 4` → `"10\n"`; `1.5 + 2.5` → `"4\n"` (float formatting parity with the interpreter).
- Division by zero and integer overflow are runtime errors (bare message, no prefix).
- `cargo test` green, `cargo clippy --all-targets` clean.

## Next (P2, separate plan)
AST → bytecode compiler for expressions/statements (locals, `if`, `for`, blocks) + the
`phg runvm <file>` CLI command + the first **differential test** asserting
`runvm` stdout == `run` stdout.
