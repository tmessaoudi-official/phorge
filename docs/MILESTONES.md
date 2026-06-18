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
- **Docs/tests:** `README.md`, 3 runnable `examples/*.phg` (guarded by `tests/examples.rs`), 162 tests green at the M1 tag (223 suite-wide today), clippy clean.
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
> 4th backend (`phg build`, M2.5) per the Rule of Three.

## v2 — Native + systems — 🔲 FUTURE

Native-AOT, ownership/no-GC, sized-int perf.
