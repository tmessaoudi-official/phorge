# error-model-slice2 Plan (M-faults Slice 2)

The three-tier error model: enforced typed **`throws E`** (→ idiomatic PHP exceptions), **`Result<T,E>`**
value surface (the shipped generic enum + `?`), and unchecked **faults/panics** (crash + Slice-1 stack
trace, never declared up-chain). Byte-identical `run ≡ runvm ≡ real PHP`. Design-first (brainstorming),
then writing-plans.

## Decisions Log
- [2026-06-22] AGREED: next slice = **error model (Slice 2)** over method overloading — biggest GA
  lever, unblocked now that generic enums ship (`Result<T,E>` is expressible), completes the `never`
  story (`throw`/`panic` become the real `never` producers).
- [2026-06-22] AGREED: **design the full three-tier model in one spec**; build cadence (one-shot vs
  sub-sliced) **deferred to plan time** — my standing lean is sub-sliced (isolate the try/catch runtime
  risk), the developer leans one-shot; decide once the seams are visible.
- [2026-06-22] AGREED: `throw`/`try`/`catch` use **native unwinding** (not desugar-to-Result) — the
  locked decision requires *idiomatic PHP exception* output, so the backends must reproduce real
  catch/unwind. Realistically **one new VM Op** for the handler/landing-pad stack; the interpreter
  catches at the `try` boundary (Rust `Result`). The `throws` **declaration** still erases pre-backend
  (front-end-only, no Op) — only the control flow needs the Op. Full `Op`-coupling discipline applies.
- [2026-06-22] AGREED: **Section A** — three tiers as above; `throw`/`panic*` are **`never`-typed**
  (satisfy return-on-all-paths); call-site rule = **enforce-or-propagate-or-catch**; propagation operator
  is **postfix `?`** (locked by spec lines 41/43), disambiguated from `?.`/`??` by one-char lookahead
  (propagation `?` only when not followed by `.` or `?`). Panic tier = `panic(string)`/`todo()`/
  `unreachable()`/`assert(bool, string?)`, all reusing the existing `Op::Fault`.
- [2026-06-22] AGREED: **Section B** — a thrown type is a subtype of a **core `Error` base**
  (interface/class), transpiling to a PHP class extending `\Exception` (home for `.message()` +
  cause-chain). Enforcement = enforce-or-propagate-or-catch; **declare specific** (`E-THROWS-TOO-BROAD`
  on the bare root), **catch broad**; **`main()` may not throw** (`E-UNCAUGHT-THROW`); `throws A | B`
  reuses S4 unions. `?` is type-directed: throws-call → propagate throw; `Result` value → unwrap/early-Err.

## Formal Plan

> Plan style = the project house format (ordered steps + acceptance + rollback), which overrides the
> superpowers bite-sized-full-code default (`User preferences override`). One plan, **phased**; a review
> checkpoint between phases; **each phase is its own green, byte-identical commit** with a guide example.
> Per the skill's scope-check, the three phases are independent subsystems — **phase 2a is detailed
> below and built first; 2b and 2c each get their own detailed plan appended here once the prior phase
> lands** (the full design for all three already lives in the approved spec).

### Global constraints (every task)
- `export PATH=/stack/tools/cargo/bin:$PATH`. Gate before every commit: `cargo test`
  (`PHORGE_REQUIRE_PHP=1 PHORGE_PHP=/stack/tools/phpbrew/php/php-master/bin/php` so the PHP oracle
  *fails*, not skips) + `cargo clippy --all-targets` + `cargo fmt --check`. The pre-commit hook reruns
  fmt+clippy+test.
- **Byte-identity spine:** `run ≡ runvm ≡ real PHP` on every example/test program. TDD: add the
  differential/checker test first, watch it fail, implement, watch it pass.
- **`Op`-coupling discipline** (only relevant from 2b on): each new `Op` extends `src/vm.rs` `exec_op`,
  `src/chunk.rs` `BytecodeProgram::validate`, and `src/compiler.rs` `stack_effect` **in the same commit**.
- **Examples-ship-with-features:** every phase lands a runnable `examples/guide/*.phg` (byte-identity
  gated by the `examples/**/*.phg` glob) + an `examples/README.md` row, same commit.
- Git autonomy authorized here: commit green self-contained work; never push.

### Lexer fact (locks the `?` design — verified)
The lexer already maximal-munches `??`→`QuestionQuestion`, `?.`→`QuestionDot`, and a lone `?`→`Question`
(`src/lexer.rs:535-569`, `src/token.rs:70-72`). So the propagation operator is the **existing `Question`
token consumed in postfix position** — **no new token, no lookahead**. The "one-char lookahead" in the
spec is already done by the tokenizer.

---

### PHASE 2a — value tier + panics (front-end only, NO new `Op`) — built first

Self-contained: `Result` `?` propagation + the `panic`/`todo`/`unreachable`/`assert` intrinsics. Lowers
to existing machinery (enum-match + `return`, and `Op::Fault`). Completes the `never` story.

**Files touched:** `src/ast.rs` (`Expr::Propagate`), `src/parser.rs` (`parse_postfix` `Question` arm),
`src/checker.rs` (propagate typing + intrinsic recognition), `src/interpreter.rs` + `src/compiler.rs`
+ `src/vm.rs` (lower propagate via existing enum-tag-test + return; intrinsics via `Op::Fault`),
`src/transpile.rs` (`__phorge_try` helper for Result-`?`; intrinsics → PHP throw),
`examples/guide/result.phg`, `examples/guide/errors.phg` is **2b** (this phase is Result+panic only).

**Task 2a.1 — `Expr::Propagate` parse.** Add `Expr::Propagate(Box<Expr>, Span)` to `ast.rs`. In
`parse_postfix` (`src/parser.rs:258`), add a `TokenKind::Question` arm *after* the `Bang` arm, wrapping
the current expr: `e = Expr::Propagate(Box::new(e), sp)`. TDD: parser test asserting `a?` parses as
`Propagate(Ident a)` and `a?.b` still parses as a safe `Member` (proves no collision). Update
`ast::free_vars`/any exhaustive `Expr` match (`collect_free_expr`, the transpiler/compiler/interpreter
`match` arms — the compiler will flag every non-exhaustive site; fix each). Commit.

**Task 2a.2 — checker: `?` typing (Result mode only this phase).** In `check_expr`, add an
`Expr::Propagate(inner)` arm: type `inner`; if it is `Ty::Named("Result", [t, e])`, the propagate value
is `t`, and the **enclosing function must return `Result<_, e'>` with `e <: e'`** (track the current
fn's return type — the checker already stores it for return-checking; reuse that) else
`E-PROPAGATE-CONTEXT`. (A `throws`-call operand is **2b** — until then, `?` on a non-Result is
`E-PROPAGATE-CONTEXT`.) TDD: checker tests — `?` on a `Result` inside a `Result`-returning fn is clean;
inside a non-`Result` fn errors; `?` on an `int` errors. `phg explain E-PROPAGATE-CONTEXT`. Commit.

**Task 2a.3 — lower `?` on the three backends (no new `Op`).** `x?` where `x: Result<T,E>` ≡
`match x { Ok(v) => v, Err(e) => return Err(e) }`. Implement by lowering in each backend exactly as the
existing variant-`match` + `return` do:
- *Interpreter* (`src/interpreter.rs`): eval `inner`; if `Ok` payload → value; if `Err` → return the
  `Err` instance as the function result (reuse the existing `return` signal).
- *Compiler/VM* (`src/compiler.rs`/`src/vm.rs`): emit the enum-tag test (reuse `Op::IsInstance`/the
  variant-discriminant test the compiler already emits for a `match` arm) + `JumpIfFalse` to an
  Err-return path that reconstructs/forwards the `Err` and emits the existing return op. **No new `Op`.**
- *Transpiler* (`src/transpile.rs`): a once-per-file `__phorge_try` helper — `function __phorge_try($r){
  if ($r is Err) return [false,$r]; return [true,$r->value]; }` pattern, or inline an
  `if ($r instanceof Err) { return $r; } $v = $r->value;` at the call site (match the existing
  `__phorge_*` helper convention; pick inline if cleaner). TDD: `tests/differential.rs` case — a
  `Result`-returning fn using `a?` + `b?` runs byte-identical on run/runvm/PHP for both the `Ok` and the
  early-`Err` path. Commit.

**Task 2a.4 — panic/todo/unreachable intrinsics (`never`).** In `check_expr`'s `Expr::Call` arm,
recognize a bare callee `panic`(1 string arg)/`todo`(0)/`unreachable`(0); type them `Ty::Never`
(reserve the names in `is_builtin_type_name`-adjacent validation so a user can't shadow them — add
`E-RESERVED-INTRINSIC`). Lower: interpreter → `Err(Fault(msg))`; VM → `Op::Fault(FaultMsg)` (reuse —
**no new Op**, the message is the panic string / a fixed `"not yet implemented"` / `"unreachable"`);
transpiler → `throw new \RuntimeException($msg)` (panic/todo) / `\LogicException` (unreachable). Add
`FaultKind::Panic` to `tests/differential.rs` so `agree_err` classifies them. TDD: differential
`agree_err` case — `panic("boom")` faults identically on run/runvm; a `never`-typed `panic` at a fn tail
satisfies return-on-all-paths (no `E-MISSING-RETURN`). Commit.

**Task 2a.5 — `assert(bool, string?)`.** Recognize `assert` in `check_expr` (1-2 args, returns `unit`);
lower to `if (!cond) <fault "assertion failed: {msg}">` using the existing branch ops + `Op::Fault`
(interpreter `Err(Fault)`); transpiler → `if (!$c) { throw new \RuntimeException(...); }`. TDD:
differential — `assert(true)` is a no-op (byte-identical), `assert(false,"x")` faults identically. Commit.

**Task 2a.6 — example + docs.** `examples/guide/result.phg`: a `Result<T,E>`-returning pipeline using
`a?`/`b?` (both `Ok` and `Err` paths, printed) + a `panic`/`assert` shown in prose comments (faults can't
be in a runnable example). `examples/README.md` row + coverage-matrix line. KNOWN_ISSUES: panics are
uncatchable-by-design (until 2b there's no `catch` anyway). Update `CHANGELOG.md` + `m-rt-progress`
memory. Run the full gate with `PHORGE_REQUIRE_PHP=1`. Commit.

**Phase 2a acceptance:** `?` on `Result` + `panic`/`todo`/`unreachable`/`assert` byte-identical
run≡runvm≡real PHP; new checker codes self-document via `phg explain`; full suite green; clippy+fmt
clean; **no new `Op`**. → review checkpoint, then write the detailed 2b plan.

---

### PHASE 2b — exceptions (control-flow core, ≈2 new `Op`s) — OUTLINE (detailed plan written after 2a)
Core `Error` base type (built-in interface → PHP class `extends \Exception`); `throws E` declaration +
call-site enforcement (`E-THROW-UNDECLARED`/`E-CALL-UNHANDLED`/`E-UNCAUGHT-THROW`/`E-THROWS-TOO-BROAD`);
`throw` (`never`); `try`/`catch` (native unwinding — interpreter `Throw(Value)` vs `Fault(msg)` signal
split; VM `Op::Throw` + a handler push/pop mechanism, full `Op`-coupling); `?` extended to the
throws-call mode; PHP `try/catch` 1:1 + bare-call `?`; totality engine extended for `try`.
`examples/guide/errors.phg`. *Detailed task breakdown authored at the 2a→2b checkpoint.*

### PHASE 2c — finally + cause-chain + imported-PHP catch bridge — OUTLINE
`finally` (compiler-emitted on normal + unwinding paths; totality: terminates iff body+catches do);
exception cause-chain (`A-fault-cause-chain`, hung off the `Error` base); catching PHP-thrown exceptions
across the interop boundary. *Detailed task breakdown authored at the 2b→2c checkpoint.*

## Self-review (plan vs spec)
- Spec §2 surface (`throws`/`try`/`catch`/`finally`/`?`/panics) → 2a covers `?`(Result)+panics; 2b
  covers `throws`/`throw`/`try`/`catch`+`?`(throws); 2c covers `finally`. ✓ full coverage across phases.
- Spec §3 enforcement + `Error` base → 2b. §4 backends: 2a is front-end/no-Op (matches "value tier +
  panics"); §4.3 VM Ops → 2b. §5 testing/examples → per-phase acceptance + guide examples. ✓
- Placeholder scan: 2b/2c are *intentionally* outlines (skill scope-check: one detailed plan per
  subsystem, written when its turn comes) — not lazy TBDs; 2a has concrete files/steps/tests. ✓
- Type/name consistency: `Expr::Propagate`, `FaultKind::Panic`, `__phorge_try`, the `E-*` codes are used
  consistently between the plan and spec. ✓
