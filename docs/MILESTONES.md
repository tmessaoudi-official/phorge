# Phorge Milestones

Living status doc. Frozen design lives in `docs/specs/2026-06-15-phorge-language-design.md`
(§5 = roadmap). Per-milestone plans live in `docs/plans/`.

## M1 — Tree-walking interpreter + transpiler — ✅ COMPLETE (2026-06-15, `9da6e56`)

The socle. Real Phorge programs run end-to-end (the frozen `Shape`/`area`/`match` sample).

- **Pipeline:** lexer → parser → type-checker → tree-walking evaluator (`src/{lexer,parser,checker,interpreter}.rs`).
- **CLI:** `phorge <run|check|parse|lex|transpile> <file>`.
- **Phorge → PHP transpiler** (`src/transpile.rs`) — round-trip-verified against real PHP 8.6.
- **Docs/tests:** `README.md`, 3 runnable `examples/*.phg` (guarded by `tests/examples.rs`), 162 tests green, clippy clean.
- **Delivered language surface:** static types, immutable-by-default bindings, functions, classes + constructor promotion, single-payload enums + exhaustive `match`, string interpolation, `List<T>` literals, `for…in`, checked int/float arithmetic.
- **Not yet implemented** (designed in §3, rejected cleanly — never panics): null safety / `T?` / `Option`, exceptions (try/catch/throw), `Map`/`Set`/tuples, `|>`, `is`, method overloading, traits, value types/structs, operator overloading, property accessors, sized ints / `decimal`, `const`/`final` enforcement, real `import` resolution, concurrency.

## M2 — Bytecode + VM — 🔲 PLANNING

Design frozen: `docs/specs/2026-06-15-m2-bytecode-vm-design.md`. Bytecode compiler + stack
VM + mark-sweep GC over the current language surface; tree-walker kept as a differential
oracle. Language enrichment = M3; single-binary bundling = M2.5.

## v2 — Native + systems — 🔲 FUTURE

Native-AOT, ownership/no-GC, sized-int perf.
