# Phorge Invariants

The load-bearing rules that keep Phorge correct. Each is non-obvious, easy to break with a
plausible-looking change, and enforced somewhere concrete. Read this before touching the backends,
the value kernels, or the `Op` set. (Companion to `docs/ARCHITECTURE.md` for the layout, and the
frozen design records in `docs/specs/`.)

## 1. Backend parity is the spine — `run` ≡ `runvm`, byte-identical
The tree-walking interpreter (`phorge run`) and the bytecode VM (`phorge runvm`) must produce
**identical stdout *and* identical failure behaviour** for every program. This is the project's
central correctness contract.
- **Enforced by** `tests/differential.rs`: `agree(src)` compares the `Ok` output; `agree_err(src)`
  compares *failures* by semantic `FaultKind` (matched on the fault **body** substring —
  `"division by zero"`, `"integer overflow"`, … — not the raw rendered string, because the CLI
  adds per-stage prefixes and the VM adds a source-line prefix the interpreter lacks).
- **Why it bites:** the original `Op::Neg` P0 (negating `i64::MIN`) hid in the gap that existed
  before `agree_err` — the Ok-only oracle never saw divergent *crashes*.

## 2. The interpreter is the reference oracle
When the VM and the interpreter disagree, the **interpreter is right by definition** — it is the
older, simpler implementation and the semantics of record. New VM behaviour is validated *against*
the interpreter, never the reverse.

## 3. Arithmetic & comparison are single-sourced in `value.rs`
The checked integer kernels (`int_add/sub/mul/div/rem/neg → Result<i64, String>`), the float
kernels (`float_*`), and `compare_ord` live **once**, in `src/value.rs`. Both backends call them.
- **Never** re-inline `checked_*` / `partial_cmp` / a fault string in `interpreter.rs` or `vm.rs` —
  that re-opens the dual-implementation drift that caused the `Op::Neg` P0.
- The three canonical fault bodies (`FAULT_DIV_ZERO`, `FAULT_MOD_ZERO`, `FAULT_INT_OVERFLOW`) are
  `pub const` in `value.rs`; the `agree_err` oracle classifies on these exact bodies, so changing a
  body string is a parity-affecting change.

## 4. Float display parity — `12.0` renders as `"12"`
`println`/interpolation render via `Value::as_display`, which formats floats with Rust `{}`
(`12.0 → "12"`, design rule EV-6). Both backends use the same method, so the transpiled PHP and
both runtimes agree. Don't introduce a second formatting path.

## 5. Adding an `Op` variant requires its match arm in the same commit
The per-op dispatch (`vm::Vm::exec_op`) is an **exhaustive** match; `BytecodeProgram::validate`
(`src/chunk.rs`) is a second match surface (wildcard `_ => None`). A new `Op` that carries an index
(`Const`, `Call`, jumps, and the coming P4 `MakeInstance`/`GetField`/`MatchTag`) must extend **both**
in lockstep, or the build breaks (exec) / a validation hole opens (validate).

## 6. No crash on input (EV-7)
Malformed or adversarial `.phg` must exit 1 with a clean `Diagnostic`, **never** SIGABRT/panic.
- The whole pipeline runs on a 256 MB worker thread (`cli::on_deep_stack`) so the *explicit* depth
  limits — not Rust's ambient stack — bound recursion.
- Limits are centralised in `src/limits.rs`: `MAX_CALL_DEPTH`, `MAX_NEST_DEPTH` (parser),
  `MAX_EXPR_DEPTH` (checker). Both backends share `MAX_CALL_DEPTH` — a single limit, no
  divergence band.
- `BytecodeProgram::validate` turns would-be out-of-range panics into clean errors before the VM
  runs a single op.

## 7. Errors are one `Diagnostic`; backend position-asymmetry is deliberate
All stages produce `diagnostic::Diagnostic { stage, message, line, col }`. `Display` has three
forms (`line==0` → no position; `col==0` → `at <line>` (VM runtime); else `at <line>:<col>`
(front-end)). **Known asymmetry:** the VM attaches a source line to runtime faults
(`Chunk.lines[ip]`); the tree-walker tracks no position (`runtime error: …`). The body-substring
oracle (#1) tolerates this. **Known limitation:** a fault *inside* string interpolation reports
line 1 — `parser::split_interpolation` re-lexes the inner expression with a fresh lexer that resets
to line 1. Deferred to the LSP/diagnostics layer.

## 8. Determinism — sort `HashMap`-derived lists before rendering
Any user-facing list built from `HashMap`/`HashSet` iteration must be sorted before `join`, or the
output varies with the hash seed. Live example: the non-exhaustive-`match` error in `checker.rs`
sorts the missing-variant list. New diagnostics that enumerate map keys must do the same.

## 9. The AST is untyped; backends re-derive types
The checker validates **without annotating** the AST. Backends that need a type re-derive it (the
compiler via `compiler::TyTag` + `num_ty`). Don't assume a node carries a resolved type — full
typed-AST threading is deferred to P4.

## 10. The quality gate is a compile-time + pre-commit invariant
`#![forbid(unsafe_code)]` on both crate roots; `[lints] warnings = "deny"` + `clippy.all = "deny"`
in `Cargo.toml` (so a warning *fails the build*); the toolchain is pinned (`rust-toolchain.toml`)
to keep that gate reproducible. The tracked `scripts/git-hooks/pre-commit` runs
`fmt --check` + `clippy -Dwarnings` + `test`. Green means: `cargo test` + `cargo clippy
--all-targets` + `cargo fmt --check` + `cargo build --release`, all clean.

## 11. No perf change without a measured before/after
`phorge bench <file>` (median-of-N, output-identity gated) is the baseline tool. Any
perf-motivated change (Copy-on-`Op`, deep-copy elimination, dispatch tweaks) must ship with a
before/after number from it — perf claims are **Verified**, not asserted.
