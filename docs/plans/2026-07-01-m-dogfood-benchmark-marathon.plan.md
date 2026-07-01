# M-DOGFOOD — Dogfood Benchmark Marathon Plan

> Autonomous full-day marathon. Validate the language against a real program (port the PHP
> `benchforge` suite to Phorj), close every gap that blocks it, and ship both automated
> (`phg bench --vs-php`) and **manual** in-language benchmarking. Plan-location: `repo`.
> Started 2026-07-01. Autonomous mode: `_AUTONOMOUS_3C=1` (session sentinel set).

## Decisions Log

- [2026-07-01] AGREED: **Direction = dogfood.** Port PHP `benchforge` (`/stack/projects/phorj-app/files/benchforge`) to Phorj as the forcing function; each wall becomes a prioritized fix. Pause A2 (generators) until the base is validated.
- [2026-07-01] AGREED: **Type-system proposals REJECTED** — (1) no force-import of primitives (`int`/`float`); they are keyword-class builtins, importing them breaks familiarity-first ([[philosophy-of-phorj]]). (2) No `Integer`/`Float`/`Decimal` object wrappers — Java-autoboxing anti-pattern; every need is already met by UFCS (`n.abs()`), `int?` (nullability), Value-native generics (`List<int>`), and the `decimal` primitive. Instead: document the **keyword-vs-import 3-way rule**.
- [2026-07-01] AGREED: **Keyword-vs-import 3-way rule** — built-in types (`int float string bool bytes decimal void never`, `List Map Set`, `T?`, function types, ranges) are keywords, NEVER imported; user/library types use `import type Pkg.Path.Name`; stdlib functions use `import Core.X`. Write into `docs/INVARIANTS.md`.
- [2026-07-01] AGREED: **Manual benchmarking reconciled with the determinism spine** — non-deterministic measurement is legal but **quarantined from the byte-identity example set** (like `Core.Time`/`Core.Process`/serve socket, all `pure:false`). Add `Core.Runtime.memoryBytes()`/`peakMemoryBytes()` (reuse `src/mem.rs`) + a `Stopwatch` over `Core.Time`. Programs using them live outside `tests/differential.rs` with dedicated tests + a README walkthrough (not glob-gated). `phg bench --vs-php` stays as the automated path.
- [2026-07-01] NOTE: Pre-existing uncommitted `0.5.0→0.5.1-alpha.1` version bump (Cargo.toml/lock, `src/bundle/manifest.rs`, CHANGELOG heading) is left untouched — stage only marathon files per commit (explicit `git add`, never `-a`).
- [2026-07-01] OUT OF SCOPE: the locked Phorj naming-overhaul breaking codemod (task #25); A2 generators (resume after).

## Design forks to batch (surface via AskUserQuestion — genuine taste decisions)

- **DF-1 return-type syntax**: parser accepts BOTH `function f(): void` and `function f() -> void` (`src/parser/items.rs:239`) — identical after parse. Canonicalize on one (PHP/TS `:` vs Rust/lambda `->`)? Decision → then `phg fmt` normalizes + optional `W-` deprecation. Non-blocking.

## Verified gap inventory (Phase 0)

| Gap | Status | Evidence |
|---|---|---|
| `List<T> xs = []` empty init | **Real bug** | live-ran → `cannot infer element type of empty list literal` (`src/checker/expr.rs:898`, expected-type threading YAGNI'd) |
| `throws A, B, C` comma form | **Absent** (only `A \| B` union works) | `src/parser/items.rs:244` |
| Line-wrapping `extends`/`implements`/`throws`/params | Unverified | verify-then-fix |
| `phg bench --vs-php` | **Exists** | `src/cli/bench.rs:26` |
| `phg bench` peak-RSS memory | **Exists** | `src/mem.rs`, CLAUDE.md |
| `Core.Time.nowMilliseconds()` stopwatch primitive | **Exists** (`pure:false`) | `src/native/time.rs` |
| In-language memory API | **Missing** | no `Core.Runtime` |
| Old lambda syntax in examples | **Not real** — `function(x) -> T {}` is current | grep `examples/` clean, only Rust `fn(` in `differential.rs` |
| `Core.File.read` ("can't read a file") | Unverified | `src/native/file.rs` exists — verify/fix |
| Conformance old syntax (219 `.phg`) | Unverified | audit |

## Waves (each: TDD → `run≡runvm≡real PHP 8.5` → clippy/fmt → commit green)

- **W0 Unblockers** — empty-collection init (`List<T> xs = []`, expected-type at decl/return/arg site); `throws A, B, C` comma sugar; line-wrapping audit+fix; keyword-vs-import rule → `docs/INVARIANTS.md`.
- **W1 Manual benchmarking tier** — `Core.Runtime.memoryBytes()`/`peakMemoryBytes()` (`pure:false`, reuse `src/mem.rs`); `Stopwatch`; quarantine plumbing + `examples/bench/manual/` README.
- **W2 File IO + JSON hardening** — verify/fix `Core.File.read`; `Core.Json` round-trip on real nested data.
- **W3 OOP + error-model dogfood** — port benchforge's interfaces, `abstract AbstractBenchmark`, `BenchmarkStatus` enum, custom exceptions + `throws`/`try`/`catch`.
- **W4 Benchmark port (simplest-first)** — Fibonacci → PrimeSieve → Sorting → Aggregation → StringProcessing → Search → Matrix → ObjectGraph; each → `phg bench --vs-php` number → fix surfaced gap.
- **W5 Stdlib breadth gap-fill** — close whatever Text/List/Map ops the port reveals missing.
- **W6 Large-data memory stress** — validate large `List`/`Map`; measure memory scaling.
- **W7 Capstone demo** — full runnable phorj `benchforge` in `/stack/projects/phorj-app`; side-by-side PHP-vs-phorj perf/memory report.
- **W8 Perf investigation** — profile + fix (or evidence-document) any benchmark where phorj is unexpectedly slow.
- **W9 CLI report formatting** — `phg bench --vs-php --json` / clean suite output.
- **W10 IDE extensions gap-fill** — VSCode + PhpStorm: new keywords/natives/diagnostics/hover/completion.
- **W11 Playground/WASM refresh** — surface new features in the browser playground.
- **W12 Conformance + docs refresh** — audit 219 `conformance/*.phg`; refresh guides, `examples/README.md` matrix, `KNOWN_ISSUES.md`.
- **Wrap** — full gate (`PHORJ_PHP=/stack/tools/phpbrew/php/php-8.5.7/bin/php PHORJ_REQUIRE_PHP=1 cargo test --workspace`, clippy, fmt) → release build → CHANGELOG/memory/plan → handoff (comparison table). No auto-push.

## Progress

- [x] **W0** (empty-list init `2350428`; comma-throws + line-wrap `debe230`; keyword-vs-import rule → INVARIANTS §12)
- [ ] W1  - [ ] W2  - [ ] W3  - [ ] W4  - [ ] W5  - [ ] W6
- [ ] W7  - [ ] W8  - [ ] W9  - [ ] W10  - [ ] W11  - [ ] W12  - [ ] Wrap

### New gaps surfaced during the marathon (feed later waves)
- **`Core.List` has no append/push/add** — only query ops (map/filter/reduce/concat…). Imperative `$arr[] = x` has no direct equivalent; idiom is `List.map(range, fn)` / `List.concat`. → W5 (decide: add mutable append or keep functional).
- **`import <unknown module>` type-checks clean** (e.g. `import Core.Types` on a non-existent module is a silent no-op) — candidate `W-UNKNOWN-IMPORT` lint. → W12.
- **DF-1 return-type syntax** (`:` vs `->`, both accepted) — batch as a design question.
