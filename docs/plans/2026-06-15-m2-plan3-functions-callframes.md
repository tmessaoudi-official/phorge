# M2 Plan 3 — Functions: Call Frames + `Call`/`Return` + Recursion

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement
> this plan task-by-task (inline; subagents deadlock on the ask-human gate in this repo).
> Steps use checkbox (`- [ ]`) syntax. **Rule 10 applies: confirm before every commit.**

**Goal:** Compile and execute user-defined function calls (including recursion and mutual
recursion) on the bytecode VM, so `phg runvm <file>` produces byte-identical stdout to
`phg run` for programs that call free functions — `examples/fib.phg` runs on the VM.

**Architecture:** The compiler grows from "compile `main` to one `Chunk`" to "compile every
top-level function to its own `Chunk`, indexed in a `BytecodeProgram`". The VM grows a
clox-style **call-frame stack**: each `Frame` is `{ func, ip, slot_base }`; locals become a
window into the value stack starting at `slot_base` (so `GetLocal`/`SetLocal` are
frame-relative). `Op::Call(idx)` pushes a frame whose window opens over the already-pushed
args; `Op::Return` pops the return value, truncates the window, pops the frame, and pushes
the return value onto the caller's stack — ending execution when `main`'s frame returns.
`main` becomes just another function (frame 0, `slot_base` 0), so all P2 programs keep
running unchanged. Classes/enums/`match`/methods stay a clean P4 compile error.

**Tech Stack:** Rust (std only), `enum Op` bytecode, per-function `Chunk`, `value::Value`
reused for scalars + inline lists (the arena heap still arrives at P4). Toolchain:
`export PATH=/stack/tools/cargo/bin:$PATH` (cargo 1.96).

---

## P3 Scope (frozen)

**In:** user-defined free functions with typed params and an optional return type · calls
`f(a, b, …)` used as expressions (in interpolation/arithmetic) and as statements ·
**recursion** and **mutual recursion** (forward references resolve via a pre-pass) ·
`return expr;` and bare `return;` · functions that fall off the end (implicit `Unit`
return) · all of the P2 surface inside any function body.

**Out (clean compile error until the named step):** classes, methods, `this`, enums,
`match`, member access → **P4** (`"… (M2 P4)"`); `null`, user `a[i]` indexing, `|>` → not in
the M1 surface (the interpreter already errors on these, so differential tests never use
them). First-class functions / function values are **not** in the surface — calls resolve
to a static function index at compile time.

## Design decisions (review these)

| # | Decision | Choice | Rationale |
|---|---|---|---|
| P3-1 | Function representation | Each function owns its **own `Chunk`**; `BytecodeProgram { functions: Vec<Function>, main: usize }` | Per-function chunks keep each function's jump targets + constant pool self-contained and 0-based — no global offset arithmetic |
| P3-2 | `Op::Return` semantics | **Pop return value → truncate to `slot_base` → pop frame → push return value on caller** (end on empty frame stack) | Uniform for `main` and callees; replaces P1/P2's "Return just ends + returns `out`" |
| P3-3 | `Op::Call` operand | `Call(usize)` carries **only the function index**; arity comes from `functions[idx].arity` | Calls resolve statically (no first-class functions); a separate `argc` operand would duplicate the function's known arity (single source of truth). Diverges from the design-sketch `Call(argc)` — that form is for clox's function-on-stack model |
| P3-4 | Recursion bound | `MAX_FRAMES` cap → clean `"stack overflow"` runtime error | The explicit frame stack can't Rust-stack-overflow; an unbounded `Vec` would OOM/hang CI on a runaway recursion. The cap is a VM safety bound, not parity (the interpreter would abort on deep native recursion; neither backend is differential-tested with *infinite* recursion). Limit value is `[Speculative]` — generous vs real depth |
| P3-5 | `main` bootstrap | `main` is an ordinary function; `run()` seeds `Frame { func: main, ip: 0, slot_base: 0 }` | With `slot_base == 0`, frame-relative `GetLocal`/`SetLocal` collapse to absolute — every P2 program keeps its exact behavior |
| P3-6 | Call-result numeric typing | Extend `num_ty` to resolve a call's declared return type (`int`/`float`) via the function-metadata map | `fib(n-1) + fib(n-2)` needs `AddI` vs `AddF` selection; the post-check AST carries no inferred types, so the compiler reads the callee's declared `ret` |
| P3-7 | Implicit return | Every compiled function ends with `Const(Unit); Return` | Mirrors the interpreter ("falling off the end yields `Unit`"); guarantees `Op::Return` always has a value to pop |

---

## File Structure

- **Modify** `src/chunk.rs` — add `Op::Call(usize)`; add `Function` + `BytecodeProgram`; update the `Return` doc-comment.
- **Modify** `src/vm.rs` — frame stack (`Frame`, `frames`), frame-driven dispatch loop, `Op::Call`/frame-aware `Op::Return`, frame-relative locals, `MAX_FRAMES` guard, migrate the in-crate unit tests to the `BytecodeProgram` model.
- **Modify** `src/compiler.rs` — multi-function compile (`compile -> BytecodeProgram`), function-index pre-pass, params as slot locals, `compile_call` for user functions, `num_ty` call-result case, implicit `Unit` return; migrate the in-crate tests + flip `user_call_is_rejected_cleanly`.
- **Modify** `src/cli.rs` — `cmd_runvm` builds a `BytecodeProgram` (rename local `chunk` → `program`).
- **Modify** `tests/differential.rs` — add the P3 program set + run `examples/fib.phg`.
- **Modify** `docs/MILESTONES.md` — mark M2 P3 ✅, P4 next.
- **Check** `README.md` — update any claim that `runvm` rejects user function calls (Phase 7).

> `src/main.rs` is **not** modified — `runvm` is already wired into USAGE + dispatch (P2).

---

## Task 1: `chunk.rs` — `Call` op + program/function types (additive, stays green)

**Files:**
- Modify: `src/chunk.rs`

- [ ] **Step 1: Add the `Call` variant + update the `Return` doc**

In `src/chunk.rs`, inside `pub enum Op { … }`, replace the final two lines:

```rust
    /// Pop `n` values, space-join their `as_display`, append a line to output.
    Print(usize),
    /// End execution, returning captured output.
    Return,
```

with:

```rust
    /// Pop `n` values, space-join their `as_display`, append a line to output.
    Print(usize),
    /// Call `functions[idx]`: its args are already on top of the stack; the new frame's
    /// local window opens at `stack.len() - functions[idx].arity` (decision P3-1, P3-3).
    Call(usize),
    /// Pop the return value, unwind the current frame (truncate its slot window), pop the
    /// frame, push the return value onto the caller's stack. End execution when the last
    /// (`main`) frame returns (decision P3-2).
    Return,
```

- [ ] **Step 2: Add `Function` + `BytecodeProgram` below the `Chunk` impl**

After the `impl Chunk { … }` block (before `#[cfg(test)]`), add:

```rust
/// A compiled function: name, parameter count, and its own bytecode chunk. Each function
/// owns its chunk so its jump targets and constant pool are self-contained (decision P3-1).
#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub arity: usize,
    pub chunk: Chunk,
}

/// A whole compiled program: every top-level function plus the index of `main`.
#[derive(Debug, Clone)]
pub struct BytecodeProgram {
    pub functions: Vec<Function>,
    pub main: usize,
}
```

- [ ] **Step 3: Add a unit test for the new types**

In `chunk.rs`'s `mod tests`, add:

```rust
    #[test]
    fn bytecode_program_holds_functions_and_main_index() {
        let mut c = Chunk::new();
        c.emit(Op::Return, 1);
        let prog = BytecodeProgram {
            functions: vec![Function { name: "main".into(), arity: 0, chunk: c }],
            main: 0,
        };
        assert_eq!(prog.functions[prog.main].name, "main");
        assert_eq!(prog.functions[0].arity, 0);
    }
```

- [ ] **Step 4: Verify it compiles + the new test passes**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --lib chunk`
Expected: PASS (additive change — `Vm`/`compile` still use the old single-`Chunk` paths and are untouched, so the whole crate still builds).

- [ ] **Step 5: Commit**

```bash
git add src/chunk.rs
git commit -m "feat(vm): Op::Call + Function/BytecodeProgram types (M2 P3)"
```

---

## Task 2: the atomic core switch — VM frames + multi-function compiler + CLI

> **Why one task / one commit:** `compile()` changes from `-> Result<Chunk, String>` to
> `-> Result<BytecodeProgram, String>` and `Vm::new` changes from `&Chunk` to
> `&BytecodeProgram`. These signatures are interdependent — the crate does **not** compile
> between them. The honest red bar for an interdependent signature change is a *compile
> failure* of the new tests after Step 1; green is reached after Steps 2–4 land together.

**Files:**
- Modify: `src/vm.rs`
- Modify: `src/compiler.rs`
- Modify: `src/cli.rs`

- [ ] **Step 1: Write the failing acceptance tests (compiler-level)**

In `src/compiler.rs`'s `mod tests`, **replace** the existing test:

```rust
    #[test]
    fn user_call_is_rejected_cleanly() {
        let src = r#"function f() -> int { return 1; } function main() { println("{f()}"); }"#;
        let e = run(src).unwrap_err();
        assert!(e.contains("M2 P3"), "{e}");
    }
```

with:

```rust
    #[test]
    fn user_function_call_runs() {
        let src = r#"function inc(int n) -> int { return n + 1; } function main() { println("{inc(4)}"); }"#;
        assert_eq!(out(src), "5\n");
    }

    #[test]
    fn recursion_runs() {
        let src = r#"function fib(int n) -> int {
            if (n < 2) { return n; }
            return fib(n - 1) + fib(n - 2);
        } function main() { println("{fib(10)}"); }"#;
        assert_eq!(out(src), "55\n");
    }

    #[test]
    fn class_call_still_rejected_as_p4() {
        // a name that is neither a function nor `println` is a variant/class → P4.
        let src = r#"function main() { println("{Circle(2.0)}"); }"#;
        let e = run(src).unwrap_err();
        assert!(e.contains("M2 P4"), "{e}");
    }
```

(These will not compile until Steps 2–4 change the signatures. That compile failure *is*
the red bar.)

- [ ] **Step 2: Rewrite `src/vm.rs` to the frame model**

Replace the imports + `struct Vm` + `impl Vm` `new`/`run` region (lines ~9–189, through the
end of the `run` method — keep the `pop`/`split_off`/`pop2_*`/`push_i` helpers and the
`compare` fn **unchanged**) with:

```rust
use crate::chunk::{BytecodeProgram, Op};
use crate::value::Value;

/// Cap on call-frame depth. Exceeding it is a clean `"stack overflow"` runtime error rather
/// than an OOM/abort (decision P3-4). Generous — real recursion is far shallower.
const MAX_FRAMES: usize = 64 * 1024;

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
        Self { program, stack: Vec::new(), frames: Vec::new(), out: String::new() }
    }

    /// Execute the program from `main`, returning captured output (`Ok`) or a runtime
    /// error (`Err`).
    pub fn run(mut self) -> Result<String, String> {
        self.frames.push(Frame { func: self.program.main, ip: 0, slot_base: 0 });
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
            let op = code[ip].clone();
            self.frames[fr].ip += 1;
            match op {
                Op::Const(i) => {
                    let v = self.program.functions[func].chunk.consts[i].clone();
                    self.stack.push(v);
                }

                Op::AddI => {
                    let (a, b) = self.pop2_int()?;
                    self.push_i(a.checked_add(b))?;
                }
                Op::SubI => {
                    let (a, b) = self.pop2_int()?;
                    self.push_i(a.checked_sub(b))?;
                }
                Op::MulI => {
                    let (a, b) = self.pop2_int()?;
                    self.push_i(a.checked_mul(b))?;
                }
                Op::DivI => {
                    let (a, b) = self.pop2_int()?;
                    if b == 0 {
                        return Err("division by zero".to_string());
                    }
                    self.push_i(a.checked_div(b))?;
                }
                Op::RemI => {
                    let (a, b) = self.pop2_int()?;
                    if b == 0 {
                        return Err("modulo by zero".to_string());
                    }
                    self.push_i(a.checked_rem(b))?;
                }

                Op::AddF => {
                    let (a, b) = self.pop2_float()?;
                    self.stack.push(Value::Float(a + b));
                }
                Op::SubF => {
                    let (a, b) = self.pop2_float()?;
                    self.stack.push(Value::Float(a - b));
                }
                Op::MulF => {
                    let (a, b) = self.pop2_float()?;
                    self.stack.push(Value::Float(a * b));
                }
                Op::DivF => {
                    let (a, b) = self.pop2_float()?;
                    self.stack.push(Value::Float(a / b));
                }
                Op::RemF => {
                    let (a, b) = self.pop2_float()?;
                    self.stack.push(Value::Float(a % b));
                }

                Op::Neg => match self.pop() {
                    Value::Int(n) => self.stack.push(Value::Int(-n)),
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
                    let v = self.stack[base + slot].clone();
                    self.stack.push(v);
                }
                Op::SetLocal(slot) => {
                    let base = self.frames[fr].slot_base;
                    let v = self.pop();
                    self.stack[base + slot] = v;
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
                            None => {
                                return Err(format!("println cannot print {}", v.type_name()))
                            }
                        }
                    }
                    self.out.push_str(&line);
                    self.out.push('\n');
                }

                Op::Call(idx) => {
                    if self.frames.len() >= MAX_FRAMES {
                        return Err("stack overflow".to_string());
                    }
                    let arity = self.program.functions[idx].arity;
                    let slot_base = self.stack.len() - arity;
                    self.frames.push(Frame { func: idx, ip: 0, slot_base });
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
        self.stack.truncate(base);
        self.frames.pop();
        if !self.frames.is_empty() {
            self.stack.push(rv);
        }
    }
```

Leave the helper functions (`pop`, `split_off`, `pop2_int`, `pop2_float`, `pop_int`,
`pop_float`, `push_i`) and the free `compare` fn exactly as they are.

- [ ] **Step 3: Migrate the `vm.rs` unit tests to the program model**

The hand-built tests call `Vm::new(&chunk)` and end their chunks with a bare `Op::Return`.
Under P3, `Op::Return` pops a return value, so each chunk must leave one. Add this helper at
the top of `vm.rs`'s `mod tests` (after the `use` lines):

```rust
    /// Emit the standard function terminator: push `Unit`, then `Return` (P3-7).
    fn term(c: &mut Chunk) {
        let u = c.add_const(Value::Unit);
        c.emit(Op::Const(u), 1);
        c.emit(Op::Return, 1);
    }

    /// Wrap a single hand-built chunk as `main` and run it.
    fn run_chunk(chunk: Chunk) -> Result<String, String> {
        let program = BytecodeProgram {
            functions: vec![Function { name: "main".into(), arity: 0, chunk }],
            main: 0,
        };
        Vm::new(&program).run()
    }
```

Update the test-module imports to:

```rust
    use super::*;
    use crate::chunk::{BytecodeProgram, Chunk, Function, Op};
    use crate::value::Value;
```

Then, in **each** test builder, replace the terminal `c.emit(Op::Return, 1);` with
`term(&mut c);`, and replace each `Vm::new(&<chunk>).run()` / `Vm::new(&<chunk>).run().unwrap()`
call with `run_chunk(<chunk>)` / `run_chunk(<chunk>).unwrap()`. The affected tests are:
`runs_integer_arithmetic_and_prints`, `float_print_matches_interpreter_formatting`,
`division_by_zero_is_runtime_error`, `negate_works_for_int_and_float`,
`comparison_and_equality`, `locals_get_and_set`, `jump_if_false_skips_branch`,
`concat_renders_mixed_scalars`, `list_make_index_len`, `print_joins_multiple_args_with_space`.

Worked example — `arith_chunk` + its test become:

```rust
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
    fn runs_integer_arithmetic_and_prints() {
        let out = run_chunk(arith_chunk()).unwrap();
        assert_eq!(out, "10\n");
    }
```

Worked example — the backpatched jump test (the only one that captures the end index): the
`let end = c.code.len();` line is followed by `term(&mut c);` instead of `c.emit(Op::Return, 1);`.
`Op::Jump(end)` then lands on the terminator's `Const(Unit)`, which returns — output unchanged:

```rust
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
```

Add one new VM-level test that exercises a frame directly (two functions, a call):

```rust
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
                Function { name: "main".into(), arity: 0, chunk: m },
                Function { name: "f".into(), arity: 1, chunk: f },
            ],
            main: 0,
        };
        assert_eq!(Vm::new(&program).run().unwrap(), "7\n");
    }
```

- [ ] **Step 4: Rewrite `src/compiler.rs` for multi-function compilation**

(a) Update the top-of-file imports:

```rust
use crate::ast::{BinaryOp, Expr, FunctionDecl, Item, Program, Stmt, StrPart, Type, UnaryOp};
use crate::chunk::{BytecodeProgram, Chunk, Function, Op};
use crate::value::Value;
use std::collections::HashMap;
```

(b) Add the function-metadata struct next to `Local` (after the `Local` struct):

```rust
/// Per-function metadata gathered in the pre-pass: its index in `BytecodeProgram.functions`
/// and its declared return-type name (for `num_ty` of a call result — decision P3-6).
struct FnMeta {
    index: usize,
    ret: String,
}
```

(c) Give `Compiler` a borrow of the metadata map (change the struct + the `impl` line):

```rust
struct Compiler<'a> {
    chunk: Chunk,
    locals: Vec<Local>,
    scope_depth: u32,
    fns: &'a HashMap<String, FnMeta>,
}
```

```rust
impl<'a> Compiler<'a> {
```

(d) Replace the whole `pub fn compile(...)` function with the multi-function version:

```rust
/// Compile a whole program: a pre-pass indexes every top-level function (so calls — including
/// forward references and recursion — resolve to a static index), then each function body is
/// compiled into its own `Chunk`. Parameters occupy slots `0..arity` at the base of the frame
/// window; every function ends with an implicit `Unit` return (P3-7).
pub fn compile(program: &Program) -> Result<BytecodeProgram, String> {
    let mut order: Vec<&FunctionDecl> = Vec::new();
    let mut fns: HashMap<String, FnMeta> = HashMap::new();
    for it in &program.items {
        if let Item::Function(f) = it {
            fns.insert(
                f.name.clone(),
                FnMeta {
                    index: order.len(),
                    ret: f.ret.as_ref().map_or_else(|| "unit".to_string(), type_name),
                },
            );
            order.push(f);
        }
    }
    let main = fns
        .get("main")
        .map(|m| m.index)
        .ok_or_else(|| "no `main` function".to_string())?;

    let mut functions = Vec::with_capacity(order.len());
    for f in &order {
        let mut c = Compiler { chunk: Chunk::new(), locals: Vec::new(), scope_depth: 0, fns: &fns };
        for p in &f.params {
            c.add_local(&p.name, &type_name(&p.ty));
        }
        let last_line = f.span.line;
        for s in &f.body {
            c.stmt(s)?;
        }
        c.emit_const(Value::Unit, last_line);
        c.emit(Op::Return, last_line);
        functions.push(Function { name: f.name.clone(), arity: f.params.len(), chunk: c.chunk });
    }
    Ok(BytecodeProgram { functions, main })
}
```

(e) Replace the `Stmt::Return` arm in `fn stmt` (drop the P2 `main`-only special-case):

```rust
            Stmt::Return { value, span } => {
                match value {
                    Some(e) => self.expr(e)?,
                    None => self.emit_const(Value::Unit, span.line),
                }
                self.emit(Op::Return, span.line);
                Ok(())
            }
```

(f) Add a `Call` case to `fn num_ty` (decision P3-6) — insert before the final `other =>` arm:

```rust
            Expr::Call { callee, .. } => {
                if let Expr::Ident(name, _) = &**callee {
                    if let Some(meta) = self.fns.get(name) {
                        return match meta.ret.as_str() {
                            "int" => Ok(NumTy::Int),
                            "float" => Ok(NumTy::Float),
                            other => Err(format!("`{name}` returns `{other}`, not numeric")),
                        };
                    }
                }
                Err(format!("cannot infer numeric type of {e:?}"))
            }
```

(g) Replace the user-call branch of `fn compile_call` (the `return Err(... M2 P3 ...)` line):

```rust
    fn compile_call(&mut self, callee: &Expr, args: &[Expr], line: u32) -> Result<(), String> {
        if let Expr::Ident(name, _) = callee {
            if name == "println" {
                for a in args {
                    self.expr(a)?;
                }
                self.emit(Op::Print(args.len()), line);
                // `println` evaluates to `Unit` (interpreter parity): leave a value so the
                // enclosing expression-statement's `Pop` is balanced.
                self.emit_const(Value::Unit, line);
                return Ok(());
            }
            if let Some(meta) = self.fns.get(name) {
                for a in args {
                    self.expr(a)?;
                }
                self.emit(Op::Call(meta.index), line);
                return Ok(());
            }
            // A non-function, non-`println` identifier call is an enum variant or class
            // constructor — those land at P4.
            return Err(format!(
                "calling `{name}` is not supported by the VM compiler yet (M2 P4)"
            ));
        }
        Err("method calls are not supported by the VM compiler yet (M2 P4)".into())
    }
```

(h) Update the compiler test harness `run()` helper (in `mod tests`) to the program model:

```rust
    /// Compile + run a program on the VM, returning captured output.
    fn run(src: &str) -> Result<String, String> {
        let tokens = lex(src).expect("lex ok");
        let prog = Parser::new(tokens).parse_program().expect("parse ok");
        let program = compile(&prog)?;
        Vm::new(&program).run()
    }
```

(The `user_function_call_runs` / `recursion_runs` / `class_call_still_rejected_as_p4` tests
from Step 1 now have working signatures.)

- [ ] **Step 5: Rewire `cmd_runvm` in `src/cli.rs`**

Replace the body of `cmd_runvm`:

```rust
pub fn cmd_runvm(src: &str) -> Result<String, String> {
    let prog = parse_checked(src)?;
    let program = compile(&prog).map_err(|e| format!("compile error: {e}"))?;
    Vm::new(&program).run().map_err(|e| format!("runtime error: {e}"))
}
```

- [ ] **Step 6: Build, then run the full suite**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test`
Expected: PASS — including `recursion_runs`, `user_function_call_runs`,
`class_call_still_rejected_as_p4`, `call_runs_a_second_function_and_returns`, and every
migrated `vm.rs`/`compiler.rs` test.

- [ ] **Step 7: Clippy**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo clippy --all-targets`
Expected: clean (no warnings).

- [ ] **Step 8: Commit**

```bash
git add src/vm.rs src/compiler.rs src/cli.rs
git commit -m "feat(vm): call frames + Call/Return + recursion (M2 P3)"
```

---

## Task 3: Differential coverage — P3 programs + the `fib` example

**Files:**
- Modify: `tests/differential.rs`

- [ ] **Step 1: Add the P3 program set + test**

After the `p2_programs_match_between_backends` test, add:

```rust
/// P3 surface: user function calls, recursion, mutual recursion, void functions, returns in
/// branches, nested calls, float-returning functions, and calls as statements. Each must run
/// identically on both backends.
const P3_PROGRAMS: &[&str] = &[
    // single call used in interpolation
    r#"function inc(int n) -> int { return n + 1; } function main() { println("{inc(41)}"); }"#,
    // multiple params + call inside arithmetic
    r#"function add(int a, int b) -> int { return a + b; }
       function main() { println("{add(2, 3) * 10}"); }"#,
    // recursion (classic fib)
    r#"function fib(int n) -> int {
           if (n < 2) { return n; }
           return fib(n - 1) + fib(n - 2);
       }
       function main() { println("{fib(12)}"); }"#,
    // return in a branch vs fall-through
    r#"function sign(int n) -> int { if (n < 0) { return -1; } return 1; }
       function main() { println("{sign(-9)}"); println("{sign(4)}"); }"#,
    // mutual recursion (forward reference: isEven calls isOdd declared later)
    r#"function isEven(int n) -> bool { if (n == 0) { return true; } return isOdd(n - 1); }
       function isOdd(int n) -> bool { if (n == 0) { return false; } return isEven(n - 1); }
       function main() { println("{isEven(10)}"); println("{isOdd(7)}"); }"#,
    // nested calls
    r#"function sq(int n) -> int { return n * n; }
       function main() { println("{sq(sq(2))}"); }"#,
    // float-returning function in float arithmetic
    r#"function half(float x) -> float { return x / 2.0; }
       function main() { println("{half(5.0) + 1.0}"); }"#,
    // void function (no return type) called for its side effect
    r#"function greet(string who) { println("hi, {who}"); }
       function main() { greet("Phorj"); greet("world"); }"#,
    // call used as a statement (return value discarded)
    r#"function noisy(int n) -> int { println("got {n}"); return n; }
       function main() { noisy(42); println("done"); }"#,
];

#[test]
fn p3_programs_match_between_backends() {
    for src in P3_PROGRAMS {
        agree(src);
    }
}
```

- [ ] **Step 2: Run `examples/fib.phg` through both backends**

Replace the `examples_that_are_p2_compatible_match` test with a version that also covers fib
(now P3-runnable), and fix its boundary comment:

```rust
#[test]
fn examples_match_between_backends() {
    // `examples/hello.phg` (P2) and `examples/fib.phg` (P3 recursion) both run on the VM.
    // `examples/grades.phg` and the Shape/area sample use enums/classes/`match` (P4), so the
    // full examples sweep arrives in P6. This test documents the boundary explicitly.
    agree(r#"import std.io;

function main() {
    println("Hello, Phorj!");
}"#);
    let fib = std::fs::read_to_string("examples/fib.phg").expect("read examples/fib.phg");
    agree(&fib);
}
```

- [ ] **Step 3: Run the differential suite**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --test differential`
Expected: PASS (`p2_programs_match_between_backends`, `p3_programs_match_between_backends`,
`examples_match_between_backends`).

- [ ] **Step 4: Commit**

```bash
git add tests/differential.rs
git commit -m "test: P3 differential programs + fib example (M2 P3)"
```

---

## Task 4: Docs — mark M2 P3 complete

**Files:**
- Modify: `docs/MILESTONES.md`
- Check: `README.md`

- [ ] **Step 1: Update the M2 status block in `docs/MILESTONES.md`**

Change the M2 heading line:

```markdown
## M2 — Bytecode + VM — 🔄 IN PROGRESS (P1–P3 done)
```

Replace the `P3 🔲 next` / P4 bullet lines with:

```markdown
- **P3 ✅** — user function calls + clox-style call frames (`Frame { func, ip, slot_base }`)
  + `Op::Call`/`Op::Return` + recursion and mutual recursion (`src/compiler.rs` multi-function
  compile → `BytecodeProgram`; `src/vm.rs` frame stack). `examples/fib.phg` runs on the VM,
  byte-identical to the tree-walker. Plan: `docs/plans/2026-06-15-m2-plan3-functions-callframes.md`.
- **P4 🔲 next** — classes/enums/`match` + arena allocation · P5 mark-sweep collector · P6 strings + full sweep.
```

- [ ] **Step 2: Check `README.md` for stale `runvm` limitations**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && grep -n -i "runvm\|function\|P3\|not.*support" README.md`
If any line claims `runvm` does not support user function calls, update it to reflect that
function calls + recursion now run on the VM (P2 surface + P3 calls; classes/enums/`match`
remain P4). If no such claim exists, state "no README change needed" and skip the edit.

- [ ] **Step 3: Commit**

```bash
git add docs/MILESTONES.md README.md
git commit -m "docs: mark M2 P3 complete (functions + call frames + recursion)"
```

---

## Self-Review

**Spec coverage** (design spec §9 P3 row = "Functions: call frames, `Call`/`Return`,
recursion; `fib` runs on the VM; differential"):
- Call frames → Task 2 `Frame`/`frames`/`slot_base`. ✅
- `Call`/`Return` → Task 1 (`Op::Call`) + Task 2 (frame-aware `Return`). ✅
- Recursion → Task 2 `recursion_runs` + Task 3 fib/mutual-recursion programs. ✅
- `fib` runs on the VM + differential → Task 3 Step 2. ✅

**Placeholder scan:** every code step shows complete code; no "TBD"/"handle edge cases"/
"similar to Task N". The one set-wide edit (Task 2 Step 3 vm-test migration) names every
affected test and shows two worked examples (the simple case + the index-capturing jump
case). ✅

**Type consistency:** `compile() -> BytecodeProgram` (chunk.rs type) consumed by
`Vm::new(&BytecodeProgram)` (Task 2) and `cmd_runvm` (Task 2 Step 5); `Op::Call(usize)`
(func index) emitted by `compile_call` and read by the VM using `functions[idx].arity`;
`FnMeta { index, ret }` produced in `compile()` and read by `compile_call` + `num_ty`;
`Function { name, arity, chunk }` built in `compile()` and in the migrated `vm.rs` tests
(`run_chunk`/`call_runs_a_second_function_and_returns`). All names align. ✅

## Completion criteria

- `cargo test` green (lib + `differential` + `cli` + `examples` + integration).
- `cargo clippy --all-targets` clean.
- `phg runvm examples/fib.phg` output byte-identical to `phg run examples/fib.phg`.
- `docs/MILESTONES.md` shows M2 P3 ✅, P4 next.
