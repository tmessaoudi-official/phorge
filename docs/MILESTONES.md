# Phorge Milestones

Living status doc. Frozen design lives in `docs/specs/2026-06-15-phorge-language-design.md`
(§5 = roadmap). Per-milestone plans live in `docs/plans/`.

## M1 — Tree-walking interpreter + transpiler — ✅ COMPLETE (2026-06-15, `9da6e56`)

The socle. Real Phorge programs run end-to-end (the frozen `Shape`/`area`/`match` sample).

- **Pipeline:** lexer → parser → type-checker → tree-walking evaluator (`src/{lexer,parser,checker,interpreter}.rs`).
- **CLI:** `phorge <run|check|parse|lex|transpile> <file>`.
- **Phorge → PHP transpiler** (`src/transpile.rs`) — round-trip-verified against real PHP 8.6.
- **Docs/tests:** `README.md`, 3 runnable `examples/*.phg` (guarded by `tests/examples.rs`), 162 tests green at the M1 tag (223 suite-wide today), clippy clean.
- **Delivered language surface:** static types, immutable-by-default bindings, functions, classes + constructor promotion, single-payload enums + exhaustive `match`, string interpolation, `List<T>` literals, `for…in`, checked int/float arithmetic.
- **Not yet implemented** (designed in §3, rejected cleanly — never panics): null safety / `T?` / `Option`, exceptions (try/catch/throw), `Map`/`Set`/tuples, `|>`, `is`, method overloading, traits, value types/structs, operator overloading, property accessors, sized ints / `decimal`, `const`/`final` enforcement, real `import` resolution, concurrency.

## M2 — Bytecode + VM — 🔄 IN PROGRESS (P1–P4 done)

Design frozen: `docs/specs/2026-06-15-m2-bytecode-vm-design.md`. Bytecode compiler + stack
VM + mark-sweep GC over the current language surface; tree-walker kept as a differential
oracle. Language enrichment = M3; single-binary bundling = M2.5.

- **P1 ✅** — `Chunk` + typed `enum Op` + stack VM dispatch loop (`src/chunk.rs`, `src/vm.rs`).
- **P2 ✅** — AST→bytecode compiler (`src/compiler.rs`) for the `main`-only expression/
  statement surface (literals, int/float arithmetic, comparison, equality, short-circuit
  `&&`/`||`, unary, interpolation, `println`, list literals, slot-based locals, `if`/`else`,
  `for…in`, blocks) + `phorge runvm` (`src/cli.rs`) + the **differential harness**
  (`tests/differential.rs`): `runvm` stdout is byte-identical to `run`. Plan:
  `docs/plans/2026-06-15-m2-plan2-compiler-runvm.md`.
- **P3 ✅** — user function calls + clox-style call frames (`Frame { func, ip, slot_base }`)
  + `Op::Call`/`Op::Return` + recursion and mutual recursion (`src/compiler.rs` multi-function
  compile → `BytecodeProgram`; `src/vm.rs` frame stack). `examples/fib.phg` runs on the VM,
  byte-identical to the tree-walker. Plan: `docs/plans/2026-06-15-m2-plan3-functions-callframes.md`.
- **P4 ✅** — single-payload enums + exhaustive `match` (P4a), classes + constructor promotion +
  field reads (P4b), instance methods + `this` (P4c). `runvm` now covers the full M1 surface;
  `examples/grades.phg` runs byte-identically on both backends (VM ≈3.2×). Object model is
  value-native (reuses `Value::Enum`/`Instance`); the **arena** is deferred to a bench-gated perf
  milestone. Plan: `docs/plans/2026-06-16-m2-p4-classes-enums-match.md`.
- **P5 🔲 next** — mark-sweep collector (the arena object model lands here, bench-gated) · P6 strings + full example sweep.

## M2.5+ — Ecosystem — 🔲 PLANNED

Full ecosystem strategy + ROI-ranked roadmap frozen in
`docs/specs/2026-06-15-ecosystem-roadmap-design.md`: two backends (native VM + optional
PHP-transpile) behind clean pluggable traits; PHP backend as a bootstrap-ecosystem lever;
M3 language enrichment → M4 extension API + stdlib → M5 modules + git-based packages → M6
concurrency (uncolored `spawn`+channels) + native HTTP server + Postgres → M7 tooling/
connectors → M8 PHP→Phorge migration tool. Rejected: live PHP transpile, PHP C-ext FFI,
dynamic `.so` plugins.

> **As-built note (M2 P3.5):** no `Backend` trait exists yet — `grep 'trait ' src/` returns
> nothing. The three pipelines (`cmd_run`, `cmd_runvm`, `cmd_transpile`) are free functions
> dispatched by a string `match` in `src/main.rs`; the pluggable-backend trait is deferred to the
> 4th backend (`phorge build`, M2.5) per the Rule of Three.

## v2 — Native + systems — 🔲 FUTURE

Native-AOT, ownership/no-GC, sized-int perf.
