# M2 Plan 2 — AST→Bytecode Compiler + `runvm` + Differential Harness

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement
> this plan task-by-task (inline; subagents deadlock on the ask-human gate in this repo).
> Steps use checkbox (`- [ ]`) syntax. **Rule 10 applies: confirm before every commit.**

**Goal:** Compile the type-checked AST to bytecode and execute it on the P1 stack VM, so
`phorge runvm <file>` produces byte-identical stdout to `phorge run` (the tree-walker) across
the P2 language surface.

**Architecture:** A new `src/compiler.rs` pass walks the existing typed `Program`, emits a
`Chunk` (the P1 `enum Op`, expanded), and the P1 `Vm` runs it. The tree-walker is retained
as a differential oracle: a `tests/differential.rs` harness asserts `cmd_run == cmd_runvm`
for every P2 program. Locals are clox-style stack slots (sets up P3 call frames). Lists are
inline `Value::List` in P2 — they migrate to the arena heap at P4.

**Tech Stack:** Rust (std only), `enum Op` bytecode, `value::Value` reused for scalars +
inline lists. Toolchain: `export PATH=/stack/tools/cargo/bin:$PATH` (cargo 1.96).

---

## P2 Scope (frozen)

**In:** int/float/bool/string literals · int+float arithmetic (`+ - * / %`, type-specialized)
· comparison (`< > <= >=`) · equality (`== != is`) · logical short-circuit (`&& ||`) · unary
(`- !`) · string interpolation · `println(...)` · list literals · locals (`Type x = e;` +
identifier use) · `if`/`else` · `for (T x in list) {}` · nested blocks.

**Out (clean compile error until the named step):** user function calls → P3; classes,
methods, `this`, enums, `match`, member access → P4; `null`, user `a[i]` indexing, `|>` →
not in the M1 surface (interpreter already errors on these, so differential tests never use
them).

## Design decisions (review these)

| # | Decision | Choice | Rationale |
|---|---|---|---|
| P2-1 | Locals | clox-style stack slots (`GetLocal`/`SetLocal(slot)`), compile-time slot resolution | Spec §6 mandates "locals are a window into the value stack"; sets up P3 frames |
| P2-2 | Jump addressing | **absolute** instruction indices (typed `enum Op`), backpatched | With `enum Op` (not raw bytes) absolute is simplest; makes the spec's separate `Loop` op redundant — dropped |
| P2-3 | `println` | generalize `Op::Print` → `Op::Print(n)` (pop n, space-join, render, newline) | Matches interpreter's multi-arg join; subsumes P1's single-value print as `Print(1)` |
| P2-4 | `SetLocal` | set-and-**pop** (`stack[slot] = pop()`) | Only used by the compiler-internal for-loop counter; Phorge has no user assignment |
| P2-5 | `JumpIfFalse` | always **pops** the condition | Uniform for `if`, `for` guard, and `&&`/`||` desugar |
| P2-6 | int vs float arithmetic | minimal `num_ty` inference in the compiler (literals + local declared types + arithmetic recursion) | Post-check AST carries no inferred types; arithmetic is the only op needing the int/float distinction; comparison/equality stay runtime-generic |
| P2-7 | Lists | inline `Value::List` (reuse the interpreter's variant) | Lets `for`/list literals run in P2; **migrates to the arena heap at P4** (documented churn) |
| P2-8 | `Eq`/`Ne`/`Lt`…`Ge` | runtime-generic (dispatch on `Value`), reuse `Value::eq_val` + a `compare` helper | Avoids 4× type-specialized equality ops; parity with interpreter `eval_binary` |

---

## File Structure

- **Modify** `src/chunk.rs` — expand `enum Op`; change `Print` → `Print(usize)`.
- **Modify** `src/vm.rs` — execute the new ops; add `compare`/`pop_bool` helpers; update P1 tests.
- **Create** `src/compiler.rs` — `pub fn compile(&Program) -> Result<Chunk, String>` + the pass.
- **Modify** `src/lib.rs` — add `pub mod compiler;`.
- **Modify** `src/cli.rs` — add `pub fn cmd_runvm`.
- **Modify** `src/main.rs` — wire `runvm` into USAGE + dispatch.
- **Modify** `README.md` — document the `runvm` subcommand (Phase 7).
- **Create** `tests/differential.rs` — `cmd_run == cmd_runvm` across the P2 surface.
- **Modify** `tests/cli.rs` — `runvm` subprocess exit-code test.

---

## Task 1: Expand the instruction set + VM execution

**Files:**
- Modify: `src/chunk.rs` (the `enum Op` definition)
- Modify: `src/vm.rs` (dispatch arms + helpers + P1 test fixups)

- [ ] **Step 1: Replace the `enum Op` definition in `src/chunk.rs`**

Replace the current `pub enum Op { … }` (lines ~9-28) with:

```rust
/// One VM instruction. Typed operands — no raw-byte decode (decision M2-7).
/// Jump targets are absolute instruction indices (decision P2-2).
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Push `consts[idx]`.
    Const(usize),
    // Type-specialized arithmetic (the checker guarantees operand types).
    AddI,
    SubI,
    MulI,
    DivI,
    RemI,
    AddF,
    SubF,
    MulF,
    DivF,
    RemF,
    /// Negate the top of stack (int or float).
    Neg,
    /// Logical not (bool).
    Not,
    // Comparison / equality — runtime-generic (decision P2-8).
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    /// Discard the top of stack.
    Pop,
    /// Push a copy of the local at stack slot `n`.
    GetLocal(usize),
    /// Pop and store into the local at stack slot `n` (set-and-pop, decision P2-4).
    SetLocal(usize),
    /// Unconditional jump to absolute instruction index.
    Jump(usize),
    /// Pop a bool; if false, jump to absolute instruction index (decision P2-5).
    JumpIfFalse(usize),
    /// Pop `n` values, concatenate their `as_display` (interpolation), push the `Str`.
    Concat(usize),
    /// Pop `n` values into a `List` (top-of-stack is the last element).
    MakeList(usize),
    /// Pop an int index and a list; push the element clone (bounds-checked).
    Index,
    /// Pop a list; push its length as an `Int`.
    Len,
    /// Pop `n` values, space-join their `as_display`, append a line to output.
    Print(usize),
    /// End execution, returning captured output.
    Return,
}
```

- [ ] **Step 2: Update the `enum Op` doc-comment in `src/chunk.rs` header**

In the module doc-comment (lines 1-4) change the `P1 scope` line to:
`//! P2 scope: full M1 expression/statement surface for `main` (see docs/plans/…m2-plan2…).`

- [ ] **Step 3: Run chunk tests — verify they still compile/pass**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --lib chunk:: 2>&1 | tail -5`
Expected: chunk tests pass (they use `Const`/`Return`, unaffected).

- [ ] **Step 4: Update P1 VM tests for `Print(usize)` in `src/vm.rs`**

In `mod tests`, change every `Op::Print` to `Op::Print(1)`:
- `arith_chunk()`: `c.emit(Op::Print(1), 1);`
- `float_print_matches_interpreter_formatting`: `c.emit(Op::Print(1), 1);`
- `negate_works_for_int_and_float`: `c.emit(Op::Print(1), 1);`

- [ ] **Step 5: Run VM tests to verify they now FAIL to compile (RED)**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --lib vm:: 2>&1 | tail -20`
Expected: compile error — `Op::Print` arm in `run()` is now `Print(usize)` but the match
still has the old no-arg arm. This drives Step 6.

- [ ] **Step 6: Replace the dispatch loop body in `src/vm.rs` `run()`**

Replace the `match op { … }` block (lines ~31-87) with the full set of arms. Keep the
existing `Const`, `AddI/SubI/MulI/DivI`, `AddF/SubF/MulF/DivF`, `Neg` arms; add the rest:

```rust
            match op {
                Op::Const(i) => self.stack.push(self.chunk.consts[i].clone()),

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
                    let v = self.stack[slot].clone();
                    self.stack.push(v);
                }
                Op::SetLocal(slot) => {
                    let v = self.pop();
                    self.stack[slot] = v;
                }

                Op::Jump(target) => ip = target,
                Op::JumpIfFalse(target) => match self.pop() {
                    Value::Bool(false) => ip = target,
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

                Op::Return => return Ok(self.out),
            }
```

- [ ] **Step 7: Add the `split_off` helper + free `compare` fn in `src/vm.rs`**

Add a method to `impl<'a> Vm<'a>` (next to `pop`):

```rust
    /// Pop the top `n` values, returning them in stack order (bottom-most first).
    /// The compiler guarantees `n <= stack.len()`.
    fn split_off(&mut self, n: usize) -> Vec<Value> {
        let start = self.stack.len() - n;
        self.stack.split_off(start)
    }
```

Add a free function below the `impl` block (before `#[cfg(test)]`):

```rust
/// Ordering comparison for `Lt`/`Gt`/`Le`/`Ge` on int or float operands. Mirrors
/// `interpreter::compare`: NaN and mixed/non-numeric operands behave identically.
fn compare(op: &Op, a: &Value, b: &Value) -> Result<bool, String> {
    use std::cmp::Ordering;
    let ord = match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.partial_cmp(y),
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y),
        _ => {
            return Err(format!(
                "cannot compare {} and {}",
                a.type_name(),
                b.type_name()
            ))
        }
    };
    Ok(match ord {
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
```

- [ ] **Step 8: Add hand-built-chunk tests for the new ops in `src/vm.rs` `mod tests`**

Append these tests (each builds a chunk by hand, like P1):

```rust
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
        c.emit(Op::Return, 1);
        assert_eq!(Vm::new(&c).run().unwrap(), "true\n");
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
        c.emit(Op::Return, 1);
        assert_eq!(Vm::new(&c).run().unwrap(), "15\n");
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
        let end = c.code.len(); // 7
        c.emit(Op::Return, 1); // 7
        c.code[jif] = Op::JumpIfFalse(else_target);
        c.code[jend] = Op::Jump(end);
        assert_eq!(Vm::new(&c).run().unwrap(), "2\n");
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
        c.emit(Op::Return, 1);
        assert_eq!(Vm::new(&c).run().unwrap(), "x=7\n");
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
        c.emit(Op::Return, 1);
        assert_eq!(Vm::new(&c).run().unwrap(), "3\n20\n");
    }

    #[test]
    fn print_joins_multiple_args_with_space() {
        let mut c = Chunk::new();
        let a = c.add_const(Value::Str("a".into()));
        let b = c.add_const(Value::Int(1));
        c.emit(Op::Const(a), 1);
        c.emit(Op::Const(b), 1);
        c.emit(Op::Print(2), 1);
        c.emit(Op::Return, 1);
        assert_eq!(Vm::new(&c).run().unwrap(), "a 1\n");
    }
```

- [ ] **Step 9: Run VM tests (GREEN) + clippy**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --lib vm:: 2>&1 | grep -E 'test result|error' || echo done`
Expected: all vm tests pass.
Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo clippy --all-targets 2>&1 | grep -E 'warning|error' || echo clean`
Expected: clean.

- [ ] **Step 10: Commit (confirm first — Rule 10)**

```bash
git add src/chunk.rs src/vm.rs
git commit -m "feat(vm): full P2 instruction set + execution (M2 P2)"
```

---

## Task 2: Compiler core — expressions + `println` + expr/return statements

**Files:**
- Create: `src/compiler.rs`
- Modify: `src/lib.rs` (add `pub mod compiler;`)

- [ ] **Step 1: Add the module to `src/lib.rs`**

After `pub mod vm;` add: `pub mod compiler;`

- [ ] **Step 2: Create `src/compiler.rs` with the pass skeleton + expressions + `println`**

This step covers literals, unary, binary (arithmetic via `num_ty`, comparison, equality,
short-circuit), interpolation, list literals, `println`, and `Expr`/`Return` statements.
Locals (Task 3) and control flow (Task 4) extend this file.

```rust
//! AST → bytecode compiler (M2 P2). A dedicated pass over the type-checked AST,
//! emitting a `Chunk` the VM executes. Mirrors the tree-walker's semantics so
//! `runvm` output is byte-identical to `run` (the differential oracle).
//!
//! P2 scope: `main`-only programs — literals, arithmetic, comparison, logical
//! short-circuit, unary, interpolation, `println`, list literals, locals, `if`/`else`,
//! `for…in`, blocks. User calls (P3), classes/enums/`match`/`this`/member (P4) raise a
//! clean compile error until implemented. Lists are inline `Value::List` in P2; they
//! migrate to the arena heap at P4.

use crate::ast::{BinaryOp, Expr, Item, Program, Stmt, StrPart, UnaryOp};
use crate::chunk::{Chunk, Op};
use crate::value::Value;

/// Numeric operand kind, inferred just enough to pick int- vs float-specialized
/// arithmetic ops (decision P2-6).
#[derive(Clone, Copy, PartialEq)]
enum NumTy {
    Int,
    Float,
}

/// A declared local: its name, the declared type name (for `num_ty`), and the lexical
/// depth it lives at (for scope cleanup). Its stack slot is its index in `locals`.
struct Local {
    name: String,
    ty: String,
    depth: u32,
}

struct Compiler {
    chunk: Chunk,
    locals: Vec<Local>,
    scope_depth: u32,
}

/// Compile a whole program to a single `Chunk`. P2 compiles the body of `main`; other
/// declarations are ignored (no user calls yet). The chunk ends in `Return`.
pub fn compile(program: &Program) -> Result<Chunk, String> {
    let main = program
        .items
        .iter()
        .find_map(|it| match it {
            Item::Function(f) if f.name == "main" => Some(f),
            _ => None,
        })
        .ok_or_else(|| "no `main` function".to_string())?;

    let mut c = Compiler { chunk: Chunk::new(), locals: Vec::new(), scope_depth: 0 };
    let last_line = main.span.line;
    for s in &main.body {
        c.stmt(s)?;
    }
    c.emit(Op::Return, last_line);
    Ok(c.chunk)
}

impl Compiler {
    fn emit(&mut self, op: Op, line: u32) {
        self.chunk.emit(op, line);
    }

    fn emit_const(&mut self, v: Value, line: u32) {
        let k = self.chunk.add_const(v);
        self.emit(Op::Const(k), line);
    }

    fn here(&self) -> usize {
        self.chunk.code.len()
    }

    /// Emit a jump placeholder (target 0); returns its code index for `patch_jump`.
    fn emit_jump(&mut self, op: Op, line: u32) -> usize {
        let idx = self.here();
        self.emit(op, line);
        idx
    }

    /// Patch a previously-emitted forward jump to point at the current code position.
    fn patch_jump(&mut self, idx: usize) {
        let target = self.here();
        self.chunk.code[idx] = match self.chunk.code[idx] {
            Op::Jump(_) => Op::Jump(target),
            Op::JumpIfFalse(_) => Op::JumpIfFalse(target),
            ref other => unreachable!("patch_jump on {other:?}"),
        };
    }

    fn begin_scope(&mut self) {
        self.scope_depth += 1;
    }

    fn end_scope(&mut self, line: u32) {
        self.scope_depth -= 1;
        while matches!(self.locals.last(), Some(l) if l.depth > self.scope_depth) {
            self.emit(Op::Pop, line);
            self.locals.pop();
        }
    }

    fn add_local(&mut self, name: &str, ty: &str) -> usize {
        self.locals.push(Local {
            name: name.to_string(),
            ty: ty.to_string(),
            depth: self.scope_depth,
        });
        self.locals.len() - 1
    }

    fn resolve_local(&self, name: &str) -> Option<usize> {
        self.locals.iter().rposition(|l| l.name == name)
    }

    /// Infer whether an arithmetic expression is int- or float-typed (decision P2-6).
    /// Only reached for operands of `+ - * / %`, which the checker guarantees are numeric.
    fn num_ty(&self, e: &Expr) -> Result<NumTy, String> {
        match e {
            Expr::Int(..) => Ok(NumTy::Int),
            Expr::Float(..) => Ok(NumTy::Float),
            Expr::Ident(name, _) => {
                let l = self
                    .resolve_local(name)
                    .map(|s| self.locals[s].ty.as_str())
                    .ok_or_else(|| format!("undefined variable `{name}`"))?;
                match l {
                    "int" => Ok(NumTy::Int),
                    "float" => Ok(NumTy::Float),
                    other => Err(format!("`{name}` is `{other}`, not numeric")),
                }
            }
            Expr::Unary { expr, .. } => self.num_ty(expr),
            Expr::Binary { lhs, .. } => self.num_ty(lhs),
            other => Err(format!("cannot infer numeric type of {other:?}")),
        }
    }

    fn stmt(&mut self, s: &Stmt) -> Result<(), String> {
        match s {
            Stmt::Expr(e, span) => {
                self.expr(e)?;
                self.emit(Op::Pop, span.line);
                Ok(())
            }
            Stmt::Return { value, span } => {
                // P2 only `main`; `Return` ends the program (the VM discards the stack).
                if let Some(e) = value {
                    self.expr(e)?;
                    self.emit(Op::Pop, span.line);
                }
                self.emit(Op::Return, span.line);
                Ok(())
            }
            // VarDecl + Block — Task 3; If + For — Task 4.
            other => Err(format!("statement not yet supported by the VM compiler: {other:?}")),
        }
    }

    fn expr(&mut self, e: &Expr) -> Result<(), String> {
        match e {
            Expr::Int(n, sp) => self.emit_const(Value::Int(*n), sp.line),
            Expr::Float(x, sp) => self.emit_const(Value::Float(*x), sp.line),
            Expr::Bool(b, sp) => self.emit_const(Value::Bool(*b), sp.line),
            Expr::Str(parts, sp) => self.compile_str(parts, sp.line)?,
            Expr::List(items, sp) => {
                for it in items {
                    self.expr(it)?;
                }
                self.emit(Op::MakeList(items.len()), sp.line);
            }
            Expr::Unary { op, expr, span } => {
                self.expr(expr)?;
                match op {
                    UnaryOp::Neg => self.emit(Op::Neg, span.line),
                    UnaryOp::Not => self.emit(Op::Not, span.line),
                }
            }
            Expr::Binary { op, lhs, rhs, span } => self.compile_binary(*op, lhs, rhs, span.line)?,
            Expr::Call { callee, args, span } => self.compile_call(callee, args, span.line)?,
            // Ident — Task 3.
            Expr::Ident(name, _) => {
                return Err(format!("identifier `{name}` needs locals (M2 P2 Task 3)"))
            }
            Expr::Null(_) => return Err("null is not supported (M1 surface)".into()),
            Expr::This(_) => return Err("`this` is not supported by the VM compiler yet (M2 P4)".into()),
            Expr::Member { .. } => {
                return Err("member access is not supported by the VM compiler yet (M2 P4)".into())
            }
            Expr::Index { .. } => return Err("indexing is not supported (M1 surface)".into()),
            Expr::Match { .. } => {
                return Err("`match` is not supported by the VM compiler yet (M2 P4)".into())
            }
        }
        Ok(())
    }

    fn compile_str(&mut self, parts: &[StrPart], line: u32) -> Result<(), String> {
        // A single literal segment (or empty) is just a string constant.
        if let [StrPart::Literal(s)] = parts {
            self.emit_const(Value::Str(s.clone()), line);
            return Ok(());
        }
        if parts.is_empty() {
            self.emit_const(Value::Str(String::new()), line);
            return Ok(());
        }
        for part in parts {
            match part {
                StrPart::Literal(s) => self.emit_const(Value::Str(s.clone()), line),
                StrPart::Expr(e) => self.expr(e)?,
            }
        }
        self.emit(Op::Concat(parts.len()), line);
        Ok(())
    }

    fn compile_binary(
        &mut self,
        op: BinaryOp,
        lhs: &Expr,
        rhs: &Expr,
        line: u32,
    ) -> Result<(), String> {
        use BinaryOp::*;
        // Short-circuit logical ops desugar to jumps (decision P2-5).
        match op {
            And => {
                self.expr(lhs)?;
                let l_false = self.emit_jump(Op::JumpIfFalse(0), line);
                self.expr(rhs)?;
                let l_end = self.emit_jump(Op::Jump(0), line);
                self.patch_jump(l_false);
                self.emit_const(Value::Bool(false), line);
                self.patch_jump(l_end);
                return Ok(());
            }
            Or => {
                self.expr(lhs)?;
                let l_rhs = self.emit_jump(Op::JumpIfFalse(0), line);
                self.emit_const(Value::Bool(true), line);
                let l_end = self.emit_jump(Op::Jump(0), line);
                self.patch_jump(l_rhs);
                self.expr(rhs)?;
                self.patch_jump(l_end);
                return Ok(());
            }
            _ => {}
        }
        // Strict ops: evaluate both, then emit.
        match op {
            Add | Sub | Mul | Div | Rem => {
                let nt = self.num_ty(lhs)?;
                self.expr(lhs)?;
                self.expr(rhs)?;
                let emit = match (op, nt) {
                    (Add, NumTy::Int) => Op::AddI,
                    (Add, NumTy::Float) => Op::AddF,
                    (Sub, NumTy::Int) => Op::SubI,
                    (Sub, NumTy::Float) => Op::SubF,
                    (Mul, NumTy::Int) => Op::MulI,
                    (Mul, NumTy::Float) => Op::MulF,
                    (Div, NumTy::Int) => Op::DivI,
                    (Div, NumTy::Float) => Op::DivF,
                    (Rem, NumTy::Int) => Op::RemI,
                    (Rem, NumTy::Float) => Op::RemF,
                    _ => unreachable!("arithmetic op set"),
                };
                self.emit(emit, line);
            }
            Eq | Is => {
                self.expr(lhs)?;
                self.expr(rhs)?;
                self.emit(Op::Eq, line);
            }
            NotEq => {
                self.expr(lhs)?;
                self.expr(rhs)?;
                self.emit(Op::Ne, line);
            }
            Lt | Gt | Le | Ge => {
                self.expr(lhs)?;
                self.expr(rhs)?;
                self.emit(
                    match op {
                        Lt => Op::Lt,
                        Gt => Op::Gt,
                        Le => Op::Le,
                        Ge => Op::Ge,
                        _ => unreachable!(),
                    },
                    line,
                );
            }
            Pipe => return Err("the `|>` pipe operator is not supported (M1 surface)".into()),
            And | Or => unreachable!("handled above"),
        }
        Ok(())
    }

    fn compile_call(&mut self, callee: &Expr, args: &[Expr], line: u32) -> Result<(), String> {
        if let Expr::Ident(name, _) = callee {
            if name == "println" {
                for a in args {
                    self.expr(a)?;
                }
                self.emit(Op::Print(args.len()), line);
                return Ok(());
            }
            return Err(format!(
                "calling `{name}` is not supported by the VM compiler yet (M2 P3)"
            ));
        }
        Err("method calls are not supported by the VM compiler yet (M2 P4)".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    use crate::parser::Parser;
    use crate::vm::Vm;

    /// Compile + run a program on the VM, returning captured output.
    fn run(src: &str) -> Result<String, String> {
        let tokens = lex(src).expect("lex ok");
        let prog = Parser::new(tokens).parse_program().expect("parse ok");
        let chunk = compile(&prog)?;
        Vm::new(&chunk).run()
    }

    fn out(src: &str) -> String {
        run(src).expect("run ok")
    }

    #[test]
    fn prints_a_literal_string() {
        assert_eq!(out(r#"function main() { println("hi"); }"#), "hi\n");
    }

    #[test]
    fn integer_arithmetic_in_interpolation() {
        assert_eq!(out(r#"function main() { println("{1 + 2 * 3}"); }"#), "7\n");
    }

    #[test]
    fn float_arithmetic_formats_like_interpreter() {
        assert_eq!(out(r#"function main() { println("{3.0 * 4.0}"); }"#), "12\n");
    }

    #[test]
    fn comparison_and_short_circuit() {
        assert_eq!(out(r#"function main() { println("{1 < 2 && 3 >= 3}"); }"#), "true\n");
        assert_eq!(out(r#"function main() { println("{1 > 2 || false}"); }"#), "false\n");
    }

    #[test]
    fn unary_negation_and_not() {
        assert_eq!(out(r#"function main() { println("{-5}"); println("{!true}"); }"#), "-5\nfalse\n");
    }

    #[test]
    fn division_by_zero_is_runtime_error() {
        let e = run(r#"function main() { println("{1 / 0}"); }"#).unwrap_err();
        assert!(e.contains("division by zero"), "{e}");
    }

    #[test]
    fn missing_main_is_compile_error() {
        let e = run(r#"function other() {}"#).unwrap_err();
        assert!(e.contains("main"), "{e}");
    }

    #[test]
    fn user_call_is_rejected_cleanly() {
        let src = r#"function f() -> int { return 1; } function main() { println("{f()}"); }"#;
        let e = run(src).unwrap_err();
        assert!(e.contains("M2 P3"), "{e}");
    }
}
```

- [ ] **Step 3: Run compiler tests (GREEN) + clippy**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --lib compiler:: 2>&1 | grep -E 'test result|error\[' || echo done`
Expected: all compiler tests pass.
Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo clippy --all-targets 2>&1 | grep -E 'warning|error' || echo clean`

- [ ] **Step 4: Commit (confirm first — Rule 10)**

```bash
git add src/lib.rs src/compiler.rs
git commit -m "feat(compiler): AST→bytecode for expressions + println (M2 P2)"
```

---

## Task 3: Compiler — locals (`VarDecl` / identifier) + blocks

**Files:**
- Modify: `src/compiler.rs`

- [ ] **Step 1: Add a failing test for locals in `src/compiler.rs` `mod tests`**

```rust
    #[test]
    fn var_decl_and_use() {
        assert_eq!(out(r#"function main() { int x = 10; println("{x + 5}"); }"#), "15\n");
    }

    #[test]
    fn multiple_locals_resolve_to_distinct_slots() {
        let src = r#"function main() { int a = 1; int b = 2; println("{a + b}"); }"#;
        assert_eq!(out(src), "3\n");
    }

    #[test]
    fn float_local_uses_float_arithmetic() {
        let src = r#"function main() { float r = 2.0; println("{r * r}"); }"#;
        assert_eq!(out(src), "4\n");
    }
```

- [ ] **Step 2: Run to verify FAIL (RED)**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --lib compiler::tests::var_decl 2>&1 | tail -5`
Expected: FAIL — `Stmt::VarDecl` hits the catch-all error; `Ident` returns the Task-3 error.

- [ ] **Step 3: Implement `VarDecl` + `Block` in `stmt()`**

Replace the catch-all arm in `stmt()` with:

```rust
            Stmt::VarDecl { ty, name, init, .. } => {
                self.expr(init)?; // value stays on the stack as the new local's slot
                let tyname = match ty {
                    crate::ast::Type::Named { name, .. } => name.as_str(),
                    crate::ast::Type::Optional { .. } => "optional",
                };
                self.add_local(name, tyname);
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
            // If + For — Task 4.
            other => Err(format!("statement not yet supported by the VM compiler: {other:?}")),
```

Add the import at the top if needed: extend the `use crate::ast::{…}` line to include `Type`
(or reference it fully-qualified as shown above — fully-qualified avoids touching the import).

- [ ] **Step 4: Implement identifier resolution in `expr()`**

Replace the `Expr::Ident` arm with:

```rust
            Expr::Ident(name, sp) => {
                let slot = self
                    .resolve_local(name)
                    .ok_or_else(|| format!("undefined variable `{name}`"))?;
                self.emit(Op::GetLocal(slot), sp.line);
            }
```

- [ ] **Step 5: Run (GREEN) + clippy**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --lib compiler:: 2>&1 | grep -E 'test result|error\[' || echo done`
Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo clippy --all-targets 2>&1 | grep -E 'warning|error' || echo clean`

- [ ] **Step 6: Commit (confirm first — Rule 10)**

```bash
git add src/compiler.rs
git commit -m "feat(compiler): locals (slot-based) + blocks (M2 P2)"
```

---

## Task 4: Compiler — `if`/`else` + `for…in`

**Files:**
- Modify: `src/compiler.rs`

- [ ] **Step 1: Add failing tests in `src/compiler.rs` `mod tests`**

```rust
    #[test]
    fn if_else_picks_branch() {
        let src = r#"function main() { if (1 < 2) { println("yes"); } else { println("no"); } }"#;
        assert_eq!(out(src), "yes\n");
    }

    #[test]
    fn if_without_else() {
        let src = r#"function main() { if (1 > 2) { println("never"); } println("after"); }"#;
        assert_eq!(out(src), "after\n");
    }

    #[test]
    fn for_loop_over_list() {
        let src = r#"function main() { List<int> xs = [1, 2, 3]; for (int x in xs) { println("{x}"); } }"#;
        assert_eq!(out(src), "1\n2\n3\n");
    }

    #[test]
    fn for_loop_body_locals_do_not_leak() {
        // A body-local must be cleaned each iteration (stack stays balanced).
        let src = r#"function main() {
            List<int> xs = [1, 2];
            for (int x in xs) { int y = x + 10; println("{y}"); }
            println("done");
        }"#;
        assert_eq!(out(src), "11\n12\ndone\n");
    }
```

- [ ] **Step 2: Run to verify FAIL (RED)**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --lib compiler::tests::if_else 2>&1 | tail -5`
Expected: FAIL — `Stmt::If`/`Stmt::For` hit the catch-all.

- [ ] **Step 3: Implement `If` + `For` in `stmt()`**

Replace the catch-all arm with calls to two new methods:

```rust
            Stmt::If { cond, then_block, else_block, span } => {
                self.compile_if(cond, then_block, else_block.as_deref(), span.line)
            }
            Stmt::For { ty, name, iter, body, span } => {
                let elem_ty = match ty {
                    crate::ast::Type::Named { name, .. } => name.clone(),
                    crate::ast::Type::Optional { .. } => "optional".to_string(),
                };
                self.compile_for(name, &elem_ty, iter, body, span.line)
            }
```

(There is now no catch-all — every `Stmt` variant is handled. Remove the `other =>` arm.)

- [ ] **Step 4: Add `compile_if` + `compile_for` methods to `impl Compiler`**

```rust
    fn compile_if(
        &mut self,
        cond: &Expr,
        then_block: &[Stmt],
        else_block: Option<&[Stmt]>,
        line: u32,
    ) -> Result<(), String> {
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

    /// `for (T name in iter)` desugars to a counter loop over an inline list
    /// (decision P2-7). Hidden locals `$for_list` and `$for_idx` bracket `name`.
    fn compile_for(
        &mut self,
        name: &str,
        elem_ty: &str,
        iter: &Expr,
        body: &[Stmt],
        line: u32,
    ) -> Result<(), String> {
        self.begin_scope();
        self.expr(iter)?; // [list]
        let s_list = self.add_local("$for_list", "list");
        self.emit_const(Value::Int(0), line); // [list, 0]
        let s_idx = self.add_local("$for_idx", "int");

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

        self.begin_scope(); // body's own locals get cleaned each iteration
        for s in body {
            self.stmt(s)?;
        }
        self.end_scope(line);

        self.emit(Op::Pop, line); // drop the loop variable
        self.locals.pop(); // unregister `name`

        // idx = idx + 1
        self.emit(Op::GetLocal(s_idx), line);
        self.emit_const(Value::Int(1), line);
        self.emit(Op::AddI, line);
        self.emit(Op::SetLocal(s_idx), line);
        self.emit(Op::Jump(loop_start), line);

        self.patch_jump(exit_jump);
        self.end_scope(line); // pops $for_idx, $for_list
        Ok(())
    }
```

- [ ] **Step 5: Run (GREEN) + clippy**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --lib compiler:: 2>&1 | grep -E 'test result|error\[' || echo done`
Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo clippy --all-targets 2>&1 | grep -E 'warning|error' || echo clean`

- [ ] **Step 6: Commit (confirm first — Rule 10)**

```bash
git add src/compiler.rs
git commit -m "feat(compiler): if/else + for-in over lists (M2 P2)"
```

---

## Task 5: `cmd_runvm` + CLI wiring + README

**Files:**
- Modify: `src/cli.rs`
- Modify: `src/main.rs`
- Modify: `README.md`

- [ ] **Step 1: Add a failing `cmd_runvm` test in `src/cli.rs` `mod tests`**

```rust
    #[test]
    fn runvm_matches_run_on_simple_program() {
        let src = r#"function main() { int x = 21; println("{x + x}"); }"#;
        assert_eq!(cmd_runvm(src).unwrap(), cmd_run(src).unwrap());
        assert_eq!(cmd_runvm(src).unwrap(), "42\n");
    }

    #[test]
    fn runvm_reports_type_error_via_the_gate() {
        let err = cmd_runvm(r#"function main() { int x = "no"; }"#).unwrap_err();
        assert!(err.contains("type error"), "{err}");
    }

    #[test]
    fn runvm_reports_runtime_error_with_prefix() {
        let err = cmd_runvm(r#"function main() { println("{1 / 0}"); }"#).unwrap_err();
        assert!(err.contains("runtime error"), "{err}");
    }
```

- [ ] **Step 2: Run to verify FAIL (RED)**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --lib cli::tests::runvm 2>&1 | tail -5`
Expected: FAIL — `cmd_runvm` does not exist.

- [ ] **Step 3: Add `cmd_runvm` to `src/cli.rs`**

Add the import near the top: `use crate::compiler::compile;` and `use crate::vm::Vm;`
Add after `cmd_run`:

```rust
/// `runvm`: lex -> parse -> check (gate) -> compile to bytecode -> VM -> captured stdout.
/// The bytecode backend; must produce byte-identical output to `cmd_run` (differential).
pub fn cmd_runvm(src: &str) -> Result<String, String> {
    let prog = parse_checked(src)?;
    let chunk = compile(&prog).map_err(|e| format!("compile error: {e}"))?;
    Vm::new(&chunk).run().map_err(|e| format!("runtime error: {e}"))
}
```

- [ ] **Step 4: Wire `runvm` into `src/main.rs`**

- Change `USAGE` to: `"usage: phorge <run|runvm|check|parse|lex|transpile> <file>"`
- Add `runvm` to the command match: `Some(c @ ("run" | "runvm" | "check" | "parse" | "lex" | "transpile")) => c,`
- Add the dispatch arm: `"runvm" => cli::cmd_runvm(&src),`
- Update the module doc-comment (line 1) command list to include `runvm`.

- [ ] **Step 5: Document `runvm` in `README.md`**

Find the subcommand documentation section and add a `runvm` entry mirroring `run`, noting:
"`runvm <file>` — same as `run`, but executes via the bytecode VM (M2). Output is identical
to `run`; the VM is verified against the tree-walker by the differential test harness."

- [ ] **Step 6: Run (GREEN) + clippy + full lib suite**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --lib 2>&1 | grep -E 'test result|error\[' || echo done`
Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo clippy --all-targets 2>&1 | grep -E 'warning|error' || echo clean`

- [ ] **Step 7: Commit (confirm first — Rule 10)**

```bash
git add src/cli.rs src/main.rs README.md
git commit -m "feat(cli): phorge runvm — bytecode backend command (M2 P2)"
```

---

## Task 6: Differential test harness (the correctness spine)

**Files:**
- Create: `tests/differential.rs`
- Modify: `tests/cli.rs` (add a `runvm` subprocess test)

- [ ] **Step 1: Create `tests/differential.rs`**

```rust
//! Differential harness (M2 P2): the bytecode VM (`cmd_runvm`) must produce byte-identical
//! stdout to the tree-walking interpreter (`cmd_run`) for every P2-surface program. This is
//! the M2 correctness spine (mirrors the transpiler round-trip-against-real-PHP technique).

use phorge::cli::{cmd_run, cmd_runvm};

/// Assert the two backends agree, and (when given) that the output is exactly `expected`.
fn agree(src: &str) {
    let tree = cmd_run(src).expect("interpreter ok");
    let vm = cmd_runvm(src).expect("vm ok");
    assert_eq!(tree, vm, "backend mismatch for:\n{src}\n  run={tree:?}\n  runvm={vm:?}");
}

/// Programs spanning the whole P2 surface. Each must run identically on both backends.
const P2_PROGRAMS: &[&str] = &[
    // literals + interpolation
    r#"function main() { println("hello"); }"#,
    r#"function main() { println("{42}"); println("{3.14}"); println("{true}"); }"#,
    // int + float arithmetic (formatting parity: 12.0 -> "12")
    r#"function main() { println("{1 + 2 * 3 - 4}"); }"#,
    r#"function main() { println("{2.0 * 3.0}"); println("{7.5 / 2.5}"); }"#,
    r#"function main() { println("{7 % 3}"); println("{7.5 % 2.0}"); }"#,
    // comparison + equality + logical short-circuit
    r#"function main() { println("{1 < 2}"); println("{2 <= 2}"); println("{3 > 4}"); }"#,
    r#"function main() { println("{1 == 1}"); println("{1 != 2}"); }"#,
    r#"function main() { println("{1 < 2 && 2 < 3}"); println("{1 > 2 || 3 > 2}"); }"#,
    // unary
    r#"function main() { println("{-5}"); println("{!false}"); }"#,
    // locals (int + float + string + bool)
    r#"function main() { int x = 10; float y = 2.5; println("{x}"); println("{y}"); }"#,
    r#"function main() { string s = "hi"; bool b = true; println("{s}"); println("{b}"); }"#,
    r#"function main() { int a = 3; int b = 4; println("{a * a + b * b}"); }"#,
    // if / else
    r#"function main() { if (1 < 2) { println("a"); } else { println("b"); } }"#,
    r#"function main() { int n = 5; if (n > 3) { println("big"); } println("end"); }"#,
    // for-in over list literals
    r#"function main() { List<int> xs = [1, 2, 3]; for (int x in xs) { println("{x}"); } }"#,
    r#"function main() { for (float f in [1.5, 2.5]) { println("{f * 2.0}"); } }"#,
    // nested blocks + for body locals
    r#"function main() { for (int x in [10, 20]) { int y = x + 1; println("{y}"); } }"#,
    // multi-arg println (space-join parity)
    r#"function main() { println("a", "b", "c"); }"#,
];

#[test]
fn p2_programs_match_between_backends() {
    for src in P2_PROGRAMS {
        agree(src);
    }
}

#[test]
fn examples_that_are_p2_compatible_match() {
    // `examples/hello.phg` and `examples/fib.phg` are P2-incompatible (fib uses user
    // function calls → P3). Drive only the P2 list above here; the full examples sweep
    // arrives in P6. This test documents the boundary explicitly.
    agree(r#"function main() { println("Hello, Phorge!"); }"#);
}
```

- [ ] **Step 2: Run the differential harness (GREEN)**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --test differential 2>&1 | grep -E 'test result|panicked|mismatch' || echo done`
Expected: pass. **If any program mismatches, that is a real compiler bug** — fix the
compiler/VM (return to the relevant task), not the expected value.

- [ ] **Step 3: Add a `runvm` subprocess test in `tests/cli.rs`**

```rust
#[test]
fn runvm_sample_simple_program_exits_0() {
    let path = write_temp("runvm_ok", r#"function main() { println("{1 + 1}"); }"#);
    let out = Command::new(BIN)
        .args(["runvm", path.to_str().unwrap()])
        .output()
        .expect("spawn phorge");
    let _ = std::fs::remove_file(&path);
    assert!(out.status.success(), "exit {:?}", out.status.code());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n");
}

#[test]
fn runvm_runtime_error_exits_1() {
    let path = write_temp("runvm_rt", r#"function main() { println("{1 / 0}"); }"#);
    let out = Command::new(BIN)
        .args(["runvm", path.to_str().unwrap()])
        .output()
        .expect("spawn phorge");
    let _ = std::fs::remove_file(&path);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("runtime error"));
}
```

- [ ] **Step 4: Full suite + clippy**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test 2>&1 | grep -E 'test result|error\[|FAILED' ; echo "exit: $?"`
Expected: every suite `ok`, exit 0 (note: rtk tee may swallow the summary — trust per-suite
`test result: ok` + exit code).
Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo clippy --all-targets 2>&1 | grep -E 'warning|error' || echo clean`

- [ ] **Step 5: Commit (confirm first — Rule 10)**

```bash
git add tests/differential.rs tests/cli.rs
git commit -m "test: VM-vs-interpreter differential harness for P2 surface (M2 P2)"
```

---

## Done criteria (P2 complete)

1. `phorge runvm <file>` runs every P2-surface program with **byte-identical** stdout to
   `phorge run` (asserted by `tests/differential.rs`).
2. `cargo test` green (lib + all integration suites, incl. the differential harness).
3. `cargo clippy --all-targets` clean.
4. Completion Gate (Coverage/Docs/Config/Blast-radius) evidenced in the closing report.

**Next (P3):** function calls + call frames (`Call`/`Return` with `slot_base`) + recursion;
extend the differential harness to user functions and `examples/fib.phg`.

## Notes / known churn

- **Inline lists** (`Value::List` on the value stack) are a P2 stepping stone; P4 introduces
  the arena heap and migrates lists (+ instances, enums, strings) to handles. `MakeList`,
  `Index`, `Len` will be rewritten then.
- **`num_ty` inference** is intentionally minimal (arithmetic operands only). P3+ may need a
  richer type-of-expression pass for function return types; revisit then.
- **`Return` in `main`** ends the program (the VM's `Return` discards the stack). Real
  function-return semantics (pop frame, leave value on caller stack) arrive in P3.
