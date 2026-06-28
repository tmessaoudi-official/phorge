# Phorj M3 Slice 1 — S0 (DX) · S1 (ergonomics) · S2 (null-safety) — Implementation Design

> Implementation-level refinement of slices **S0, S1, S2** from the M3 roadmap
> (`docs/specs/2026-06-17-m3-language-roadmap-design.md`). Fixes grammar deltas, token/AST/`Op`
> changes, checker rules, both-backend semantics, the PHP-transpile mapping, and tests — per feature.
> Governed by **D-L9** (every feature maps to idiomatic PHP) and the parity spine (`run` ≡ `runvm`,
> byte-identical; `docs/INVARIANTS.md`). **Build order: S0 → S1 → S2**, each green + byte-identical
> before the next. Adding any `Op` extends the three exhaustive matches in lockstep
> (`vm.rs::exec_op`, `chunk.rs::validate`, `compiler.rs::stack_effect` — memory
> `op-variant-match-coupling`).

## 0. Grounding — what already exists (verified against `ast.rs`/`chunk.rs`/`token.rs`)

| Already present | Consequence |
|---|---|
| `Type::Optional { inner }` (`T?` parses) | S2 reuses the type node; work is *semantics*, not parsing |
| `Expr::Index` + `Op::Index` (bounds-checked) + `Op::Len` | S1 indexing = **un-reject in checker + wire interpreter/transpile**; no new `Op` |
| `Expr::Match` is an `Expr`; `Pattern::{Null,Binding,Wildcard}` | expression-`match` done; null-matching reuses `Pattern::Null` |
| `null` literal (`Expr::Null`, `TokenKind::Null`) | S2 optionals are **null-based (PHP-way)**, not an `Option` enum (D-L9) |
| tokens `Question` `Bang` `Dot`; `Trait` reserved | S2 operators reuse/extend these tokens |
| `Stmt::If` (statement only); `Stmt::VarDecl { ty, name, init }` | expression-`if` and `var` inference are the new parser/checker work |

This materially shrinks the slice: S1 indexing and expression-`match` are nearly free; S2 maps onto
PHP's native nullable model.

---

## S0 — Developer experience (no `Op`/backend change; CLI + checker only)

### S0.1 Per-command `--help` with examples
- **Surface:** `phg <cmd> --help` / `-h` prints a command-specific usage block with a one-line
  description, the source forms, flags, and **1–2 worked examples**. `phg --help` lists commands.
- **Where:** `cli.rs` (a `help_for(cmd)` table) + `main.rs` dispatch (intercept `--help`/`-h` before
  running). No grammar/backend impact.
- **Transpile:** n/a. **Tests:** `tests/cli.rs` — `run --help` exit 0, contains the example + form list;
  unknown `--help` target → top-level help.

### S0.2 `var` — local type inference
- **Grammar:** `varDecl := ('var' | type) ident '=' expr ';'`. `var` infers the binding's type from
  `expr`; explicit types still allowed; the binding stays **immutable** (no reassignment — existing rule).
- **Token/AST:** add `TokenKind::Var`; represent inference as a new `Type::Infer(Span)` placeholder in
  `Stmt::VarDecl.ty` (minimal; avoids making `ty` optional everywhere).
- **Checker:** on `Type::Infer`, set the binding type to the checked type of `init`. `var x = null;` is
  an **error** ("cannot infer from `null` — annotate `T?`"), since `null` alone has no element type.
- **Backends:** none — values are dynamic; the type is erased after checking.
- **Transpile:** emit `$x = <expr>;` (PHP locals are untyped regardless).
- **Tests:** infer int/float/string/list/instance; `var` then misuse type-errors; `var x = null` rejected;
  `run`≡`runvm` on a `var` program; transpile snapshot.

### S0.3 `type` aliases
- **Grammar:** top-level `typeAlias := 'type' Ident '=' type ';'` → new `Item::TypeAlias { name, ty, span }`.
- **Token:** add `TokenKind::TypeKw` (`type`).
- **Checker:** pre-pass registers aliases; resolve alias names structurally wherever a `Type::Named` is
  looked up (alias of alias allowed; cycle → clean error). Compile-time only.
- **Backends/Transpile:** fully **erased** — aliases vanish after checking (PHP has no type aliases).
- **Tests:** `type UserId = int;` usable as a param/field/return type; alias cycle → error; erased in PHP.

### S0.4 Sharper diagnostics + `phg explain`
- **Diagnostics:** extend `diagnostic.rs` rendering to underline the offending span with a **caret line**
  (`^^^`) under the source line, and add an optional `hint: Option<String>` ("did you mean `…`?",
  nearest-identifier suggestion via edit distance over in-scope names).
- **`phg explain <code>`:** a new read-only subcommand mapping a diagnostic code (e.g. `E-OPT-UNWRAP`)
  to a paragraph + example. Diagnostics gain a stable `code` field.
- **Backends/Transpile:** n/a. **Tests:** a known type error renders a caret + hint; `explain E-…` prints
  the entry; unknown code → exit 1 with a clean message.

---

## S1 — Core ergonomics

### S1.1 Indexing `xs[i]` (un-reject; infra exists)
- **Grammar/AST:** already parse to `Expr::Index`. No parser change.
- **Checker:** **accept** `Index { object, index }` when `object: List<T>` and `index: int` → type `T`
  (currently rejected). Any other receiver/index type → clean type error. (Maps + strings: deferred to S4/S7.)
- **Semantics:** interpreter + `Op::Index` already clone the element with a **bounds-checked** fault on
  out-of-range (D-L8 — clean runtime error, never PHP's null+warning). Confirm both backends emit the
  identical fault message.
- **Transpile:** `$xs[$i]` (PHP). PHP's silent-null OOB differs from Phorj's checked fault — acceptable:
  Phorj guarantees the index is valid by the time it runs, and the differential harness compares only
  programs that don't fault. (Document in KNOWN_ISSUES.)
- **Tests:** `xs[0]` reads; OOB faults identically on both backends; non-int index type-errors; transpile snapshot.

### S1.2 Ranges `0..n` (exclusive) and `0..=n` (inclusive)
- **Grammar:** `range := expr ('..' | '..=') expr` (int bounds). Primary use: `for (int i in 0..n)`.
- **Token:** add `TokenKind::DotDot` (`..`) and `TokenKind::DotDotEq` (`..=`); lexer must read `..`/`..=`
  before `.` (longest-match).
- **AST:** new `Expr::Range { start, end, inclusive, span }`.
- **Checker:** both bounds `int` → the range is iterable as `List<int>` (its only role this slice).
- **Semantics (decision S1-R):** the compiler/interpreter **materialize** a range into a `List<int>`
  (`start..end` → `[start, …, end-1]`, `..=` includes `end`; empty if `start >= end`). New `Op::MakeRange`
  (pop two ints, push the `List`) keeps the VM honest and avoids unbounded compile-time expansion. Adding
  `MakeRange` extends the three exhaustive matches.
- **Transpile:** `range($a, $b - 1)` / `range($a, $b)` (PHP `range` is inclusive).
- **Tests:** `for (int i in 0..3)` yields 0,1,2; `..=` includes the end; empty range; `run`≡`runvm`; transpile.

### S1.3 Expression `if` (expression-`match` already exists)
- **Grammar:** allow `if (cond) { e } else { e }` in **expression** position; both arms required and
  same-typed; arms are blocks whose value is the trailing expression (or a new `Expr::Block`).
- **AST:** new `Expr::If { cond, then_expr, else_expr, span }` (the statement `Stmt::If` is unchanged; the
  parser chooses expr-vs-stmt by context). Introduce `Expr::Block(Vec<Stmt>, tail: Box<Expr>)` for
  block-valued arms.
- **Checker:** `cond: bool`; `then`/`else` same type `T` → result `T` (no `else` in expression position →
  error).
- **Semantics:** compiler emits the existing branch ops leaving the arm value on the stack (mirrors how
  `Expr::Match` already lowers). Interpreter evaluates the taken arm to a value.
- **Transpile:** PHP `match(true){ cond => e, default => e2 }` (keeps it an expression; cleaner than nested
  ternary for blocks).
- **Tests:** `var x = if c { 1 } else { 2 };`; type-mismatch arms error; missing-else error; `run`≡`runvm`; transpile.

### S1.4 Smart-cast narrowing
- **Scope this slice:** narrow an optional `T?` to `T` inside the truthy branch of an `if (var x = opt)`
  (S2.5) and inside a `match` arm that excludes `null`. (General `is`-narrowing on enum/class hierarchies
  → S5.)
- **Checker-only:** flow-sensitive type refinement in the relevant scope; no runtime/transpile effect.
- **Tests:** within `if (var x = opt)`, `x` is usable as `T`; outside, it is still `T?`.

---

## S2 — Null-safety (PHP-native nullable, compile-time non-null)

**Model (decision S2-N):** `T?` = "`T` or `null`", represented at runtime by the existing **`null`**
value — the PHP-way (D-L9). The *guarantee* lives in the checker: a non-optional `T` can **never** hold
`null`; the only way to use a `T?` as `T` is to handle absence. This is exactly TypeScript
`strictNullChecks` over PHP's nullable runtime — and transpiles 1:1.

### S2.1 Non-null discipline (checker)
- A `null` literal has type `T?`-compatible only; assigning `null` (or a `T?`) to a non-optional `T`
  (binding, param, field, return) is a **type error** (`E-OPT-ASSIGN`).
- Member/method access / arithmetic on a `T?` without unwrapping is an error (`E-OPT-USE`, with a hint to
  use `?.`/`??`/`if (var …)`/`!`).
- A non-optional `T` **auto-widens** to `T?` where a `T?` is expected (covariant; `T` is a subtype of `T?`).
- **Transpile:** types erase; `null` → PHP `null`.

### S2.2 `??` null-coalesce
- **Grammar:** `a ?? b`; lowest-but-one precedence (below `||`). **Token:** add `TokenKind::QuestionQuestion`
  (lex `??` before `?`). **AST:** `BinaryOp` gains `Coalesce` (reuses `Expr::Binary`).
- **Checker:** `a: T?`, `b: U` where `U` is `T` or `T?` → result `T` (if `b: T`) else `T?`.
- **Semantics:** evaluate `a`; if non-null push it, else evaluate/push `b`. New `Op::Coalesce`? — no: lower
  to existing branch ops (dup/JumpIfNull-equivalent) to avoid an `Op`. (decision S2-OPS: S2 adds **no new
  `Op`** — `??`/`?.`/`!`/`if-let` lower to existing branch + `Pop` + a null-test.) A null-test reuses
  `Op::Eq` against a `null` const.
- **Transpile:** `($a ?? $b)`.
- **Tests:** `null ?? 3 == 3`; `x ?? y` short-circuits (y not evaluated when x non-null); type rules; parity; transpile.

### S2.3 `?.` safe access
- **Grammar:** `opt?.member` / `opt?.method(args)`. **Token:** add `TokenKind::QuestionDot` (lex `?.`).
  **AST:** `Expr::Member`/`Call` gain a `safe: bool` (or new `Expr::SafeMember`). 
- **Checker:** `opt: T?`; result is `(member type)?` — the whole chain is optional.
- **Semantics:** if `opt` is null → null; else normal access. Lowers to a null-test + branch (no new `Op`).
- **Transpile:** `$opt?->member` (PHP nullsafe).
- **Tests:** `null?.x == null`; `obj?.x` reads; chaining `a?.b?.c`; parity; transpile.

### S2.4 `if (var x = opt)` binding
- **Grammar:** `if ('(' 'var' ident '=' expr ')') block [else block]` where `expr: T?`; binds `x: T`
  (smart-cast S1.4) in the then-block when non-null.
- **AST:** extend `Stmt::If` with an optional `bind: Option<(String, Expr)>` (or a dedicated `IfLet`).
- **Checker:** `expr: T?` required; `x: T` in scope only in `then_block`.
- **Semantics:** evaluate `expr`; if non-null, bind + run then; else run else. Lowers to null-test + branch
  + a local slot (existing `SetLocal`/`GetLocal`).
- **Transpile:** `if (($x = <expr>) !== null) { … } else { … }`.
- **Tests:** binds when present, skips when null; `x` is `T` (not `T?`) inside; parity; transpile.

### S2.5 `opt!` checked force-unwrap
- **Grammar:** postfix `expr '!'` (reuse `TokenKind::Bang`). **AST:** new `Expr::Force { inner, span }`.
- **Checker:** `inner: T?` → `T`. A lint (`W-FORCE-UNWRAP`) flags every use, nudging to `??`/`?.`/`if-let`
  (D-L1 guardrail). `!` on a non-optional → error.
- **Semantics:** if null → **clean runtime fault** `"force-unwrap of null `<name>` at <line>"` (named +
  located; never a crash). Lowers to null-test + a fault op (reuse the `MatchFail`-style fault path, or a
  generic `Op::Fault(msg)` if none exists — confirm in `vm.rs`; prefer reusing the existing fault channel,
  no new `Op`).
- **Transpile:** a tiny runtime helper `__phorj_unwrap($v, 'name', line)` that `throw`s on null, else returns
  `$v` (emitted once per file when `!` is used).
- **Tests:** `opt!` returns the value when present; faults **identically on both backends** when null
  (FaultKind parity — memory `error-parity-faultkind`); lint fires; transpile round-trips under real PHP.

### S2.6 `match` over `T?`
- Reuse `Expr::Match` + `Pattern::Null` + `Pattern::Binding`: `match opt { null => …, v => … }` is exhaustive
  for `T?` (null arm + binding arm); the binding arm narrows `v: T`. Checker treats null-arm + catch-all
  binding as covering `T?`.
- **Transpile:** `match(true){ $opt === null => …, default => (fn($v)=>…)($opt) }` or an `if/else` lowering.
- **Tests:** exhaustiveness (missing null arm with no catch-all → error); parity; transpile.

---

## Cross-cutting

- **Parity spine:** every S1/S2 feature gets a differential program under `examples/` (auto byte-identity
  gated) **and** a `tests/differential.rs` case. New `Op::MakeRange` is the only `Op` added (S1.2) — it
  extends `vm.rs::exec_op`, `chunk.rs::validate` (no static index ⇒ documented like `GetEnumField`), and
  `compiler.rs::stack_effect` in one commit.
- **Transpile:** each feature adds a `tests/cli.rs` PHP round-trip (run the emitted PHP under real `php`,
  assert byte-identical to `runvm`). The two genuinely-divergent points (indexing OOB; `!` on null) are
  fault cases the differential harness already excludes, and are documented in `KNOWN_ISSUES.md`.
- **Diagnostics codes** introduced: `E-OPT-ASSIGN`, `E-OPT-USE`, `E-OPT-UNWRAP`, `W-FORCE-UNWRAP`,
  `E-INFER-NULL`, `E-RANGE-TYPE`, `E-IF-EXPR-ELSE` — each gets a `phg explain` entry (S0.4).

## Decisions log

| # | Decision | Choice | Rationale |
|---|---|---|---|
| S2-N | Optional representation | **null-based (PHP nullable)**, not an `Option` enum | D-L9: maps 1:1 to PHP `?T`/`null`/`??`/`?->`; guarantee lives in the checker (TS `strictNullChecks` model) |
| S2-OPS | New ops for `??`/`?.`/`!`/if-let | **none** — lower to existing branch + null-test (`Eq` vs `null`) | keeps the `Op` set + the three coupled matches minimal; nullable is control-flow, not a new value kind |
| S1-R | Range representation | **materialize to `List<int>`** via one new `Op::MakeRange` | only role this slice is `for…in`; matches PHP `range()`; avoids unbounded compile-time expansion |
| S0-INF | `var` inference encoding | `Type::Infer` placeholder resolved in the checker | minimal AST change; backends already type-erased |
| S0-ALIAS | `type` aliases | compile-time, fully erased | PHP has no type aliases (D-L9) |
| S1-IDX | Indexing | un-reject the existing `Index`/`Op::Index` path | infra already present + bounds-checked (D-L8) |

## Risks

- **`Op::MakeRange` parity** — the one new op; covered by the three-match coupling + a differential range
  program. A huge `0..n` materializes a big list (acceptable for the slice; lazy ranges deferred).
- **`null` literal's current checker treatment** — `Expr::Null`/`Pattern::Null` exist; the implementation
  must confirm how `null` is typed today and route it through the new non-null discipline without breaking
  existing programs (grounding task for the plan: read `checker.rs` + `value.rs` null handling first).
- **Transpile divergence at faults** (OOB index, `!`-on-null) — excluded by the differential harness
  (fault cases) and documented; not a parity break for non-faulting programs.
- **Scope** — S2 is the heavy third; S0 and S1 ship first (fast, low-risk) so value lands before the
  type-system work.
