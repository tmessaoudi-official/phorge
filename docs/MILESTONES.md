# Phorge Milestones

Living status doc. Frozen design lives in `docs/specs/2026-06-15-phorge-language-design.md`
(§5 = roadmap). Per-milestone plans live in `docs/plans/`. `examples/README.md` is the living
showcase of the runnable language surface (every example byte-identical on both backends + the
Phorge→PHP transpile bridge).

## M1 — Tree-walking interpreter + transpiler — ✅ COMPLETE (2026-06-15, `9da6e56`)

The socle. Real Phorge programs run end-to-end (the frozen `Shape`/`area`/`match` sample).

- **Pipeline:** lexer → parser → type-checker → tree-walking evaluator (`src/{lexer,parser,checker,interpreter}.rs`).
- **CLI:** `phg <run|check|parse|lex|transpile> <file>`.
- **Phorge → PHP transpiler** (`src/transpile.rs`) — round-trip-verified against real PHP 8.6.
- **Docs/tests:** `README.md`, 3 runnable `examples/*.phg` (guarded by `tests/examples.rs`), 162 tests green at the M1 tag, clippy clean.
- **Delivered language surface:** static types, immutable-by-default bindings, functions, classes + constructor promotion, single-payload enums + exhaustive `match`, string interpolation, `List<T>` literals, `for…in`, checked int/float arithmetic.
- **Not yet implemented** (designed in §3, rejected cleanly — never panics): null safety / `T?` / `Option`, exceptions (try/catch/throw), `Map`/`Set`/tuples, `|>`, `is`, method overloading, traits, value types/structs, operator overloading, property accessors, sized ints / `decimal`, `const`/`final` enforcement, real `import` resolution, concurrency.

## M2 — Bytecode + VM — ✅ COMPLETE (2026-06-16, `dbf4a67`)

Design frozen: `docs/specs/2026-06-15-m2-bytecode-vm-design.md`. Bytecode compiler + stack
VM over the full M1 language surface; tree-walker kept as a differential oracle. Language
enrichment = M3; single-binary bundling = M2.5.

- **P1 ✅** — `Chunk` + typed `enum Op` + stack VM dispatch loop (`src/chunk.rs`, `src/vm.rs`).
- **P2 ✅** — AST→bytecode compiler (`src/compiler.rs`) for the `main`-only expression/
  statement surface (literals, int/float arithmetic, comparison, equality, short-circuit
  `&&`/`||`, unary, interpolation, `println`, list literals, slot-based locals, `if`/`else`,
  `for…in`, blocks) + `phg runvm` (`src/cli.rs`) + the **differential harness**
  (`tests/differential.rs`): `runvm` stdout is byte-identical to `run`. Plan:
  `docs/plans/2026-06-15-m2-plan2-compiler-runvm.md`.
- **P3 ✅** — user function calls + clox-style call frames (`Frame { func, ip, slot_base }`)
  + `Op::Call`/`Op::Return` + recursion and mutual recursion (`src/compiler.rs` multi-function
  compile → `BytecodeProgram`; `src/vm.rs` frame stack). `examples/fib.phg` runs on the VM,
  byte-identical to the tree-walker. Plan: `docs/plans/2026-06-15-m2-plan3-functions-callframes.md`.
- **P4 ✅** — single-payload enums + exhaustive `match` (P4a), classes + constructor promotion +
  field reads (P4b), instance methods + `this` (P4c). `runvm` now covers the full M1 surface;
  `examples/grades.phg` runs byte-identically on both backends. Plan:
  `docs/plans/2026-06-16-m2-p4-classes-enums-match.md`.
- **Wave 4 ✅** — class-aware compiler operand types (`TyTag` → `enum CTy { Int, Float,
  Class(String), Other }` + a recursive `ctype(&Expr)` resolver), closing the last `num_ty`
  parity gaps (field read on an arbitrary instance, method result, nested member, class-typed
  enum payload). Plan: `docs/plans/2026-06-16-m2-wave4-compiler-types.md`.
- **P5a ✅** — `Rc`-shared heap objects (`Value::Instance`/`Enum`/`List` → `Rc<…>`): `Op::GetLocal`
  and every interpreter var-read became an O(1) refcount bump instead of a deep clone. Object-heavy
  VM run **1537 ms → 634 ms (2.4×)**, VM advantage recovered **4.73× → 9.35×** (≈ scalar's 10.92×).
  Design: `docs/specs/2026-06-16-m2-p5-object-model-design.md`; plan:
  `docs/plans/2026-06-16-m2-p5a-rc-shared-heap.md`.

### Success criteria (design §10) — met

1. **Byte-identical backends ✅** — every `examples/*.phg` (`hello`/`fib`/`grades`) and
   `tests/fixtures/sample.phg` produce identical stdout under `phg runvm` and `phg run`,
   gated by `tests/differential.rs` (`examples_match_between_backends`, the per-feature program
   tables, and `agree_err` for failure parity). 244 tests green.
2. **Reclamation ✅ (GC stance revised)** — the original M2-4 decision was a handle/arena heap +
   mark-sweep collector. **P5a established that no tracing GC is needed for M2:** the M1 heap is
   *immutable + acyclic* (no reassignment, no field mutation, constructor args evaluated before the
   instance exists), so an `Rc` graph can never form a cycle — `Drop` reclaims everything, with no
   use-after-free and no panics (`#![forbid(unsafe_code)]` intact). A real tracing collector is
   **deferred to M3**, where mutation can finally create cycles that refcounting alone would leak.
3. **Quality gate ✅** — `cargo test` green (244), `cargo clippy --all-targets` clean,
   `cargo fmt --check` clean.

> **Superseded P5/P6 scope:** the in-progress doc previously listed "P5 mark-sweep collector · P6
> strings + full example sweep." Strings/interpolation parity landed in P2/P3.5, the full-surface
> example sweep landed with P4c + Wave 4, and the tracing GC is deferred to M3 (above). The arena's
> slot-indexed field layout (P5 Phase B) stays **bench-gated and unopened** — after P5a the object
> path is within ~15% of the scalar baseline, so field access no longer dominates.

## M2.5 — Standalone executables (`phg build`) — 🔨 IN PROGRESS (Phases 1–2 complete; Phase 3 next)

Single-binary bundling: `phg build foo.phg` → a standalone executable that runs `foo.phg` on the
VM with no Phorge install. Design (advisor-reviewed twice): payload = a **named section** (`.phorge`
on ELF, `__PHORGE,__source` on Mach-O — never a raw overlay, which breaks Mach-O signing) holding a
**versioned CRC-guarded container** (source→bytecode is a `payload_kind` flip, not a format break);
distribution via a **stub registry** (CI builds/signs per-target stubs once per release; `phorge
build` fetches+caches+`llvm-objcopy --add-section`s the payload); macOS signed+notarized **from
Linux** via `rcodesign` (no Mac needed). std-only line = the produced binary + the hand-rolled
section reader; build tooling (zig, llvm-tools, rcodesign, CI) is exempt. Spec:
`docs/specs/2026-06-16-m2.5-phorge-build-design.md`.

- **Phase 1 ✅ (2026-06-16)** — host `x86_64-linux-gnu`, no CI/signing:
  `docs/plans/2026-06-16-m2.5-phase1-build-linux-gnu.md`. `src/bundle.rs` (CRC-32 + versioned
  container + hand-rolled ELF64 reader + `embedded_source()`), the `main()` self-detect hook,
  `cli::cmd_build` (copy `current_exe` + `llvm-objcopy --add-section .phorge=…`), and `tests/build.rs`
  (built binary byte-identical to `runvm`). This is the **4th backend** the Rule-of-Three note below
  anticipated — still a free-function path, no `Backend` trait yet.
- **Phase 2 ✅ (2026-06-17)** — cross-OS builds via `cargo-zigbuild` (zig as the C/linker driver):
  `bundle.rs` split into a `bundle/` module + hand-rolled std-only **PE/COFF**, **Mach-O 64**, and
  **fat/universal** section readers (checked arithmetic, EV-7) behind a magic-sniffing `find_section`;
  `phg build --target/--all` with a per-target stub cache keyed on the phg binary's FNV-1a-64
  hash (stale stub → cache miss, protecting the parity spine). Targets: Linux `x86_64-musl`,
  `aarch64-{gnu,musl}`, `x86_64-pc-windows-gnu`. Cross-parity gated by `tests/build.rs` (musl native
  exec + real windows-PE round-trip). macOS reader ships + is fixture-tested; the Mac *stub* (signing)
  is deferred to Phase 3, and apple/darwin `--target` is rejected with a clear message. Spec/plan:
  `docs/specs/2026-06-16-m2.5-phase2-cross-os-design.md`, `docs/plans/2026-06-16-m2.5-phase2-cross-os.md`.
  **Gotcha (verified):** `llvm-objcopy --add-section` on **PE** needs `--set-section-flags
  …=noload,readonly` or it writes a zero-data section; the flags are applied unconditionally (ELF + PE).
- **Phase 3 🔲** — CI stub registry; final-artifact signing/notarization (opt-in `--sign`),
  Windows Authenticode + macOS codesign/notarize via `rcodesign` from Linux.

### Tooling (v0.4.0) — profiling + introspection

- `phg bench` reports **memory** (cold-execution peak-RSS growth + process `VmHWM`/`VmRSS`) next to
  its timing, via a std-only Linux `/proc` sampler (`src/mem.rs`); non-Linux prints "unavailable".
- `phg disasm <source>` dumps the compiled bytecode (per-function listings + descriptor tables).
- `examples/bench/workload.phg` (+ `examples/bench/README.md`) is the profiling showcase, auto
  byte-identity-gated like every example.

## M3 — Language enrichment — 🔨 IN PROGRESS

Slice-by-slice language growth under the transpile contract **Phorge : PHP :: TypeScript : JavaScript**
(every feature maps to idiomatic PHP; PHP-absent features are compile-time-only and erased). Shipped so
far: **S0** (developer experience — `var` inference, `type` aliases, sharp caret diagnostics + stable
codes, `phg explain`), **S1** (ergonomics — indexing `xs[i]`, integer ranges `a..b`/`a..=b`, expression
`if`), **S2** (null-safety — `T?`, `??`, `?.`, checked `opt!`, if-let binding, `match` over `T?`, the
warning channel), and **S3 Track A** (lambdas — expression + statement body — first-class function
values, and the pipe operator `|>`). Cross-cutting: stdlib **Track B** Waves 1–2
(`core.console`/`math`/`text`/`file`, namespaced natives) and **Track D** (`phg bench --vs-php`). All
slices are byte-identical on `run`/`runvm` and round-trip through real PHP. The live slice-by-slice
status + forward plan live in `CLAUDE.md` (Active plan) and `CHANGELOG.md`; design specs are under
`docs/specs/2026-06-17-m3-*` and `docs/specs/2026-06-18-m3-*`. Modules/packages and web capabilities were
promoted to their own milestones — **M5** (✅ closed) and **M6** (🔨 in progress), below. The Rich-Types
sub-track (**M-RT** — ✅ **CLOSED 2026-06-23**: `instanceof`, interfaces, `Map`/`Set`, erased generics
incl. methods+classes+enums (`Option<T>`/`Result<T, E>`), unions `A|B`, intersections `A&B`, the
**totality cluster** — return-on-all-paths + `never` + dead-code lints, **method/function overloading**,
**S6 inheritance** (single + multiple, final-by-default), and the finale **S8 traits** (`trait`/`use`
horizontal reuse — methods, state, constructors, abstract requirements, property hooks; reuse not a type;
native PHP `trait`/`use`)) and the **mutation milestone** (below) also run under M3's umbrella.

## M-Decomp — Codebase decomposition — ✅ COMPLETE (2026-06-23)

Behavior-preserving, cohesion-based split of the whale source files into per-module directories
(`foo/mod.rs` + cluster files), verified by the `run ≡ runvm ≡ real PHP 8.4` byte-identity spine
(823 tests green throughout; every wave its own commit). Plan
`docs/plans/2026-06-23-decomposition-milestone.plan.md`, design
`docs/specs/2026-06-23-decomposition-milestone-design.md`, module map in `docs/ARCHITECTURE.md`.

- **Axis = hybrid by-phase**, not by-construct (4 research agents converged: every production compiler
  files by phase). Splits live inside one `mod` so child files see the parent struct's private
  members; moved inherent methods take `pub(super)` — **zero crate-public widening**. The three coupled
  exhaustive `Op` matches (`vm::exec_op`, `chunk::validate`, `compiler::stack_effect`) stay **whole**,
  verified by a dummy-`Op`-variant smoke check.
- **Front-end:** `checker/` 9786→454 (11 clusters + 3 rewrite passes) · `parser/` 1934→199 (5 construct
  clusters) · `ast/` 1465→669 (`walk`/`classes`) · `loader/` 1220→588 (`resolve`/`fs`).
  **Backends:** `compiler/` 2967→740 · `transpile/` 2407→355 · `interpreter/` 1757→612 · `vm/` 915→322.
  No source file exceeds ~1500 lines; `lexer/` (621, one scanner) and `chunk.rs` (shared `Op` contract)
  left single by design.
- **Tests mirror the split** as sealed child modules: **by language feature** for the cross-cutting
  `checker/tests/` (integration tests through `check()`), **by construct** for `parser/tests/`. Backend
  test files (< 460 lines) kept flat.

## M-RT pattern cluster + primitives sweep — ✅ COMPLETE (2026-06-23)

Post-M-RT language ergonomics, front-end-only (no new `Op`, no `Value` change), byte-identical
`run ≡ runvm ≡ real PHP 8.4`. Plan `docs/plans/2026-06-23-pattern-cluster.plan.md`,
example `examples/guide/pattern-matching.phg`.

- **S5.1 match-arm guards** — `pat when <cond> => …` (contextual `when`); guarded arms don't discharge
  exhaustiveness (`E-MATCH-GUARD-EXHAUST`); `E-GUARD-TYPE`.
- **S5.2 struct destructuring** — `Pattern::Struct` (shorthand/rename/full-nesting, reuses
  `Op::IsInstance` + field reads; compiler `PathSeg` mixes enum-payload and named-field steps) + nested
  type patterns in variant payloads (`W(Circle c)`); refutable payloads no longer falsely discharge a
  variant's coverage (`is_irrefutable`). Codes `E-STRUCT-PAT-TYPE`/`-FIELD-UNKNOWN`/`E-PATTERN-DUP-BIND`.
- **S5.3 flow-narrowing** — `narrow_from_condition`: `instanceof` then/else (else → remaining union
  members), `!`/`&&`/`||` composition, early-return guards narrow the rest of a block; plus **if-let
  `when` guards** (parser-desugar, no `Stmt::If.guard` field). Deferred (KNOWN_ISSUES): `||`-true-side,
  equality/literal refinement and `== null` (Phorge rejects those comparisons), post-match narrowing
  (match is an expression), while-let guards.
- **Primitives sweep** — number-literal formats (`0x`/`0b`/`0o`/`_`/`1e3`), bitwise `& | ^ ~ << >>`
  (int-only; `>>` = two `Gt`), `Console.print`, byte-safe stdlib (`Text.startsWith`/`endsWith`/`repeat`,
  `Math.round`, `List.length`). M4 holds the optional-return/generic-ordering natives (`parseInt`, sort…).

## M-mut — In-place mutation — ✅ FEATURE-COMPLETE (2026-06-21)

Phorge began as a pure single-assignment language (no assignment statement); the mutation milestone
adds in-place mutation **immutable-by-default, `mutable` opt-in**, with **no tracing GC**. Locked spine
(forced by the real-PHP oracle, design `docs/specs/2026-06-21-mutation-milestone-design.md`):
`List`/`Map`/`Set`/`Bytes` are **copy-on-write value types** (can't cycle ⇒ `Rc`/`Drop` reclaims fully);
`Instance` is a **shared-mutable handle** (PHP/Java semantics). Every slice is byte-identical
`run ≡ runvm ≡ real PHP`.

- **M-mut.1** mutable locals + reassignment · **.2** compound-assign + `++`/`--` + `??=` · **.3** condition
  loops (`while`/`do-while`/C-`for`/while-let) + `break`/`continue` · **.4a** `obj with { f = e }` ·
  **.5** value-type element set `xs[i]=e`/`m[k]=e` (`Op::SetIndex`, COW) · **.6** shared-mutable instance
  fields `o.f=e` (`Op::SetField`; instances are handles; **no cycle collector** — Fork-3) · **.7a**
  `static`/`static mutable` class fields `ClassName.field` (`Op::GetStatic`/`SetStatic`) · **.7b**
  property hooks `T name { get => …; set(T v) { … } }` (virtual get/set, synthetic `$get`/`$set` methods
  via `Op::CallMethod` — no new `Op`; PHP 8.4 property hook; `examples/guide/property-hooks.phg`).
- **Deferred** (KNOWN_ISSUES, each a clean compile error or explicit non-goal): cycle collector,
  identity `===`, nested place-stores (`this.f[i]=e`), backed/static/interface/abstract hooks.

## Visibility modifiers — ✅ COMPLETE (2026-06-21)

Three-level declaration visibility on every top-level declaration (class, enum, interface, free
function): `public` (default — cross-package), `internal` (this package's files), `private` (this
`.phg` file). Lattice `file ⊂ package ⊂ public`. A dedicated `Visibility` enum (distinct from member
`Modifier` visibility), parsed as a leading keyword, **loader-enforced and backend-erased** — applied
at the loader's three resolution chokepoints before the merged program reaches any backend, so the
`run ≡ runvm ≡ real PHP` spine is safe by construction (PHP has no file/package-private declarations).
Codes `E-VIS-PRIVATE`/`E-VIS-INTERNAL` (with `phg explain`); example `examples/project/visibility/`.
Design `docs/specs/2026-06-21-visibility-modifiers-design.md`. Deferred (KNOWN_ISSUES): visibility on
`type` aliases / `import` re-exports; member-level `Modifier` visibility stays PHP-only-enforced.

## Error handling & stack traces — 🔨 Slice 1 ✅ COMPLETE (2026-06-21); Slice 2 designed

Two slices (developer-chosen, traces first). **Slice 1 — fault reporting — ✅ COMPLETE:** uncaught
runtime faults render a call stack (frames + `file:line`, source line) identically on `run`/`runvm`
(VM frame-walk + interpreter `trace_stack`, `run ≡ runvm` trace-parity test), in the CLI and a
`phg serve --dev` HTML 500 page (prod = bare 500, no leak). Front-end-only: stdout unchanged,
`FaultKind` preserved, oracle unaffected, no new `Op`. Spec
`docs/specs/2026-06-21-stack-traces-and-fault-reporting-design.md`. Deferred: method/ctor/closure
frames are line-only (Slice 1.1); fault cause-chain (needs the catchable model).

**Slice 2 — the catchable error model — DECIDED 2026-06-22 (locked), not yet built.** Three tiers,
one enforced-failure principle: **(1) enforced typed `throws E`** — the fix to PHP's *unchecked*
`@throws` docblock (checker-enforced at the call site, `?`-propagable, **specific type required** — no
`throws Exception` swallow), transpiles to **idiomatic PHP exceptions**; the PHP-familiar *default*
surface. **(2) `Result<T, E>`** — error-as-value (functional, `match`/`?`), transpiles to a PHP value;
rides generic enums. **(3) unchecked faults/panics** — programmer bugs (index-OOB, force-unwrap-null)
crash with a Slice-1 stack trace, never declared up the chain (the explicit fix to Java's
checked-everything mistake). `throws` is front-end-only (erases before the backends ⇒ byte-identity
safe, no new `Op`); `try/catch` discharges the `throws` surface and the imported-PHP interop bridge.
Examples use PascalCase packages (`package Main;`, `import Core.Console;`). Folds in the fault
cause-chain and the test runner's `assertFaults`. Detailed rationale + examples in
`docs/specs/2026-06-21-php-parity-and-beyond.md` §2.1.

## M5 — Modules & packages — ✅ COMPLETE (2026-06-18)

Go-shaped, `src/`-rooted project model: **mandatory `package` declarations** (`package Main` = runnable
entry), `phorge.toml` manifests (Composer *vocabulary* in a TOML container — `[require]`, git deps pinned by
tag/rev), strict folder = package path, **single-file brace-namespace PHP emission** (no Composer/autoloader
— [ADR-0004](adr/0004-single-file-brace-namespace-php.md)), cross-package qualified calls via a loader-side
name-mangling pass (`run ≡ runvm` structural; the transpiler de-mangles to `namespace` blocks), and
**offline-only** git dependencies — `phg vendor` is the sole network command; `run`/`check`/`transpile`
never fetch ([ADR-0005](adr/0005-offline-only-vendor.md)). Design
`docs/specs/2026-06-18-m5-project-model-design.md`.

## M6 — Web capabilities — 🔨 CORE COMPLETE (W0–W4 shipped; extensions deferred)

A portable `handle(Request) -> Response` model at the *value* level (PSR-7/15 shape); the socket bridge is
runtime glue, quarantined in `src/serve.rs` behind a `Transport` trait, outside the byte-identity spine.
Shipped: **W0** (`bytes` primitive + `b"…"` literals + `Core.Bytes`), **W1** (pure-Phorge
`Request`/`Response` + `parse_request`/`serialize_response`, `examples/web/handler.phg`), **W2** (the
`Core.Http` `Router` with **path parameters** `r"/users/{id}"` → `req.param`, literal>param precedence,
404 fallback, `examples/web/router.phg`, plus the PHP-8 **`#[Route(...)]` attribute** + the
`Http.autoRouter()` compile-time desugar, `examples/web/router-attrs.phg`; **W2-ext slice 1:**
`router.use(mw)` middleware + `router.group(prefix, build)` sub-routers (`examples/web/middleware.phg`)
+ **slice 2:** `{name:regex}` route constraints via `Core.Regex` (`examples/web/route-constraints.phg`)
+ **slice 3:** `#[Route]` on static class methods, the controller shape (`examples/web/controller.phg`)
— **W2-ext complete**; design `docs/specs/2026-06-28-m6-w2-extensions-design.md`; design
`docs/specs/2026-06-28-m6-w2-router-attributes-design.md`), **W3** (`src/serve.rs` socket transport
behind the `Transport` trait, tested via `tests/serve.rs` outside the spine), and **W4** (`phg serve` +
the PHP front-controller `examples/web/server.php`, full served app `examples/web/server.phg`).
**`Core.Json`** (parse/stringify/stringifyPretty) layers on top — `examples/web/json-api.phg` is a
byte-identity-gated JSON endpoint over the same `handle` contract. Deferred (later M6): middleware /
closure routes, regex route constraints, attribute targets beyond free functions, and green-threaded
concurrency under the *unchanged* contract. Design `docs/specs/2026-06-18-m6-web-design.md`.

## M7 — Correctness closure — ✅ COMPLETE (2026-06-19, `1c6119d` / `ac9bda8`)

Closed the third backend leg: `tests/differential.rs` now transpiles every example/project, runs it under a
real `php`, and asserts stdout byte-identical to the interpreter — so `run ≡ runvm ≡ php` is *enforced*, not
just `run ≡ runvm`. **Fails-not-skips:** `PHORGE_REQUIRE_PHP=1` makes a missing `php` a test failure
(`PHORGE_PHP=<path>` overrides). Four silent transpiler→PHP P0 divergences fixed via runtime helpers
(`__phorge_div`/`_rem`/`_str`/`_range`), plus a large-range cap. Spec
`docs/specs/2026-06-19-m7-correctness-closure-design.md`.

## M8–M12 — Road to GA 1.0 — 🔨 / 🔲

The sequenced path to a stable 1.0 lives in **`docs/plans/2026-06-19-phorge-ga-roadmap.plan.md`** — the
forward SSOT, mapping ~50 review findings: **M8** trust & hardening (vendor/serve/`write_atomic`, lints) ∥
**M9** engineering hygiene (CI enforcement ✅, ADRs ✅, exhaustive `validate` ✅, single-sourcing, doc-SSOT) →
**M10** erasure-first generics (`Ty::Var` — [ADR-0002](adr/0002-erasure-not-monomorphization.md)) → **M11**
stdlib breadth (`core.list`/`json`, `Map`/`Set`) → **M12** release automation + 1.0.

> **Superseded numbering:** the earlier ecosystem roadmap (`docs/specs/2026-06-15-ecosystem-roadmap-design.md`,
> M4 extension API → M5 modules → M6 concurrency+HTTP → M7 tooling → M8 migration) remains a historical
> design exploration; the **GA roadmap above is the authoritative milestone sequence from M5 on.**

> **As-built note:** no `Backend` trait exists — the three pipelines (`cmd_run`/`cmd_runvm`/`cmd_transpile`)
> are free functions dispatched by a string `match` in `src/main.rs`, deferred to the 4th backend
> (`phg build`) per the Rule of Three ([ADR-0001](adr/0001-no-shared-run-vm-ir.md)).

## Roadmap-completeness audit — ✅ DELIVERED (2026-06-22)

A one-shot 20-track (A–S + V) multi-agent gap review (41 agents) enumerated every gap vs PHP 8.0–8.4
parity, beyond-PHP capability, DX/tooling, correctness, security, stdlib, numerics, i18n, testing,
perf, build, observability, docs, and governance — the "stop finding gaps ad hoc" deliverable.
**555 candidates → 290 adopt · 187 defer · 81 reject.** SSOT:
**`docs/specs/2026-06-21-php-parity-and-beyond.md`** (deduplicated master triage table + per-milestone
rollup + top-10 spine + reject-list-with-reasons + 10 cross-track themes); raw per-track reports under
`docs/research/roadmap-completeness/`. Forward plan + new milestones folded into
[`ROADMAP.md`](../ROADMAP.md).

**Locked decisions (2026-06-22):**

1. **M-RT sequence:** totality cluster (return-totality + `never` + unreachable-after-return) **first**
   — the #1 soundness leak — *then* method overloading → `extends`/`abstract`/LSB (S6) → traits (S8);
   generic enums + the pattern cluster in parallel.
2. **Error model (Slice 2):** three tiers — enforced typed `throws E` (default) + `Result<T,E>` (value)
   + unchecked faults (bugs). See the error-handling section above.
3. **New milestones created:** M4 (stdlib charter), M-NUM (decimal/money + numerics), M-TIME (date/time),
   M-text (i18n core), M-Test (test framework), M-perf (VM opt behind a regression gate), M-Batteries
   (impure stdlib on the quarantine seam), M8.5 (interop / `.d.phg`), M13 (editions, post-1.0).
4. **Namespace PascalCase reshape** (audit-missed, now tracked): `package Main`, `E-PKG-CASE`
   (PascalCase package/folder segments, **enforced incl. vendor**; PHP/Composer deps case-mapped to
   PSR-4 at the importer boundary), manifest `name → module`, lift `E-PKG-TYPE` — a breaking codemod,
   design `docs/specs/2026-06-20-package-namespace-reshape-design.md`, pending build.

## v2 — Native + systems — 🔲 FUTURE

Native-AOT, ownership/no-GC, sized-int perf.
