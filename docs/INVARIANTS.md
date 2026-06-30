# Phorj Invariants

The load-bearing rules that keep Phorj correct. Each is non-obvious, easy to break with a
plausible-looking change, and enforced somewhere concrete. Read this before touching the backends,
the value kernels, or the `Op` set. (Companion to `docs/ARCHITECTURE.md` for the layout, and the
frozen design records in `docs/specs/`.)

## 1. Backend parity is the spine — `run` ≡ `runvm`, byte-identical
The tree-walking interpreter (`phg run`) and the bytecode VM (`phg runvm`) must produce
**identical stdout *and* identical failure behaviour** for every program. This is the project's
central correctness contract.
- **Enforced by** `tests/differential.rs`: `agree(src)` compares the `Ok` output; `agree_err(src)`
  compares *failures* by semantic `FaultKind` (matched on the fault **body** substring —
  `"division by zero"`, `"integer overflow"`, … — not the raw rendered string, because the CLI
  adds per-stage prefixes and the VM adds a source-line prefix the interpreter lacks).
- **Why it bites:** the original `Op::Neg` P0 (negating `i64::MIN`) hid in the gap that existed
  before `agree_err` — the Ok-only oracle never saw divergent *crashes*.
- **Third surface (M2.5 `phg build`):** a standalone binary runs its **embedded source** through
  `cli::cmd_runvm` at startup (the self-detect hook in `src/main.rs`), so its output MUST equal
  `phg runvm <file>`. **Enforced by** `tests/build.rs::built_binary_matches_runvm`. The startup
  hook must keep dispatching through `cmd_runvm` (never `cmd_run`) and must not transform the source
  before execution — otherwise the distribution layer silently drifts off the spine while the
  differential suite (which never builds a binary) stays green.
  - **Cross-targets (Phase 2):** the surface now spans cross-built binaries. The stub-cache key is the
    **FNV-1a-64 of the running phg binary's bytes**, so a rebuilt phorj ⇒ cache miss ⇒ fresh stub —
    a stale stub can never embed your source into an *old* VM. Cross-parity is gated by
    `cross_musl_binary_matches_runvm` (native exec) and `cross_windows_section_round_trips` (PE section
    round-trip). The object-file section readers (ELF/PE/Mach-O/fat) honor **EV-7**: every offset uses
    checked arithmetic, and malformed/adversarial images return `None`, never a panic or OOB read.

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

**`int` is a fixed 64-bit signed integer (`i64`), pinned by design.** Unlike PHP's `int`, whose width
is platform-dependent (32-bit on a 32-bit build, 64-bit elsewhere), Phorj's `int` is **always** `i64`
on every target — the backends all carry `Value::Int(i64)` and the transpile floor (PHP 8.5) is 64-bit
in practice, so the width never varies. `int` arithmetic that would leave the `i64` range is a **checked
fault** (`FAULT_INT_OVERFLOW`, EV-7 — never a silent wrap, unlike PHP which auto-promotes an overflowing
`int` to `float`). `float` is IEEE-754 double (`f64`). `decimal` is an exact i128-carried fixed-point
value (M-NUM S1). Conversions between these are **explicit** natives (`Core.Conversion`) — there is no
implicit coercion.

## 4. Float display parity — `12.0` renders as `"12"`
`printLine`/interpolation render via `Value::as_display`, which formats floats with Rust `{}`
(`12.0 → "12"`, design rule EV-6). Both backends use the same method, so the transpiled PHP and
both runtimes agree. Don't introduce a second formatting path.

## 5. Adding an `Op` variant requires its match arm in the same commit
A new `Op` touches **three** match surfaces, and as of M9 **all three are exhaustive (no `_`
wildcard)** — so a missing arm is a *compile error*, never a silent hole:
- `vm::Vm::exec_op` (`src/vm.rs`) — the per-op execution semantics (irreducibly per-`Op`).
- `compiler::Compiler::stack_effect` (`src/compiler.rs`) — the net stack delta.
- `BytecodeProgram::validate` (`src/chunk.rs`) — the operand-index bounds check (EV-7). Until M9
  this carried a `_ => None` wildcard, so a new index-carrying `Op` could silently skip its bounds
  check; it now enumerates every variant (index-checked arms via `.then(|| …)` + one explicit
  no-index `=> None` arm), matching the other two. **Do not reintroduce a `_` wildcard here** — it
  is the forcing function that makes the bounds check un-skippable.

A new `Op` that carries a *pool* index (`Const`, `Call`, jumps, `MakeInstance`/`GetField`/`MatchTag`,
`MakeClosure`, …) must add its bounds arm to `validate`; one that carries only a count or local slot
goes in the no-index arm. Either way the compiler now refuses to build until you choose.

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
The checker validates **without annotating** the AST. Backends that need a type re-derive it
structurally from the declared `Type` annotations — the compiler via `compiler::CTy` + `ctype`/
`num_ty` (M2 Wave 4 made this class-aware: `CTy::Class(name)` carries an instance's class so
`obj.field`/`obj.m()`/class-typed payloads resolve). Don't assume a node carries a resolved type.

## 10. The quality gate is a compile-time + pre-commit invariant
`#![forbid(unsafe_code)]` on both crate roots; `[lints] warnings = "deny"` + `clippy.all = "deny"`
in `Cargo.toml` (so a warning *fails the build*); the toolchain is pinned (`rust-toolchain.toml`)
to keep that gate reproducible. The tracked `scripts/git-hooks/pre-commit` runs
`fmt --check` + `clippy -Dwarnings` + `test`. Green means: `cargo test` + `cargo clippy
--all-targets` + `cargo fmt --check` + `cargo build --release`, all clean.

## 11. No perf change without a measured before/after
`phg benchmark <file>` (median-of-N, output-identity gated) is the baseline tool. Any
perf-motivated change (Copy-on-`Op`, deep-copy elimination, dispatch tweaks) must ship with a
before/after number from it — perf claims are **Verified**, not asserted.
