# Phorj History

The chronological milestone narrative — what shipped, in what order, and what it taught us.
Compressed from the pre-rewrite CLAUDE.md log (which carried this record through the pattern
cluster) and continued from `docs/MILESTONES.md` / `CHANGELOG.md`. Status of record lives in
`docs/MILESTONES.md`; this file is the story. Newest last.

## M1 — Tree-walking interpreter + transpiler (2026-06-15)
Shipped: full pipeline (lexer → parser → checker → evaluator) + Phorj→PHP transpiler verified
against real PHP; core surface (static types, immutable-by-default, classes + constructor
promotion, enums + exhaustive `match`, interpolation, `List<T>`, checked arithmetic).

## M2 — Bytecode compiler + stack VM (2026-06-16, closed `33c6b78`)
Shipped: second backend (`phg runvm`) byte-identical to the interpreter over the full M1 surface;
the differential harness became the correctness spine. P3.5 hardening (waves 0–4); P4 enums/match,
classes, methods + `this`; Wave 4 class-aware compiler operand typing (`CTy`); P5a `Rc`-shared heap
(object-heavy VM run 1537→634 ms, 2.4×) — no tracing GC, the immutable+acyclic heap is fully
reclaimed by `Rc`/`Drop`. Full-coverage example set landed; `tests/differential.rs` globs
`examples/**/*.phg`.
Gotchas: the CTy-operand trap (an expression result used as an arithmetic operand must be typed by
the compiler); zero-payload enum variants — bare `V =>` in a match is a silent catch-all binding.

## M2.5 — Standalone executables `phg build` (v0.4.0; Phase 3a later)
Shipped: source embedded in a versioned CRC-guarded `.phorj` section (hand-rolled ELF64/PE/Mach-O
readers, EV-7 checked arithmetic); cross-OS builds via cargo-zigbuild stubs (musl/aarch64/windows),
FNV-1a-64 stub cache; Phase 3a stub registry (SHA-256 + verify-before-cache).
Gotcha: `llvm-objcopy --add-section` on PE needs `--set-section-flags …=noload,readonly` or the
section is written zero-data — only a real-binary windows round-trip test caught it.

## v0.4.0 platform work
CLI UX (`-v`/`-h`, stdin `-`, `-e`/`--eval`, `--` literal path; `cli::resolve_source`);
benchmark memory reporting (std-only Linux `/proc` sampler, cold-execution peak-RSS);
bytecode disassembly; the full OSS doc set (dual MIT OR Apache-2.0, CONTRIBUTING, ROADMAP,
VISION, FEATURES, KNOWN_ISSUES, …).

## M3 — Language enrichment (S0 → S3)
S0 DX: `var` inference, `type` aliases (expanded out pre-backend — the discipline every later
sugar follows), caret diagnostics + stable codes + `explain`. S1 ergonomics: list indexing,
integer ranges (`Op::MakeRange`), expression `if`. S2 null-safety: `T?`, `??`, `?.`, if-let,
checked force-unwrap, match-over-optional; the warning channel; `Op::MatchFail` generalized to
`Op::Fault`. S3 Track A: lambdas (`function(int x) => e` / block bodies), first-class function
values, pipe `|>` (parser-lowered); `Op::MakeClosure`/`Op::CallValue`.
Gotcha (S2): mid-expression scratch slots must be `self.height - 1` — two `??`/`?.` in one
expression silently broke run↔runvm with the naive slot.

## Namespace reshape + Track B stdlib (2026-06-18)
"Everything namespaced, nothing in the wind": Go-style module-qualified calls, reserved `Core.`
root, explicit imports even for stdlib, bare `println` retired. Wave 1: the `(module,name)` native
registry + `Op::CallNative` + import-driven resolution in all four backends + `E-SHADOW-IMPORT`.
Wave 2: Math/Text/File breadth. (Stdlib later PascalCased `c4479d6`, and renamed again in the
2026-06-30 naming overhaul — `Core.Console` → `Core.Output`, `println` → `printLine`.)

## M5 — Modules, packages, vendoring (closed)
Go-shaped project model: mandatory `package`, `package Main` entry, `phorj.toml` walk-up,
folder=path, loader-side name-mangling (backends consume the rewritten AST unchanged — run≡runvm
structural by construction), brace-namespace single-file PHP emission, import aliasing;
git deps + `phorj.lock` + `phg vendor` (the only network-touching command), offline-only loads.

## M6 — Web capabilities (W0 → W4)
Design lock: portable unit is `handle(Request) -> Response` at the value level; single-threaded
forced by the `Rc` heap; socket quarantined behind a `Transport` seam outside the differential.
W0 `bytes` + literals + `Core.Bytes`; W1 pure-Phorj Request/Response handler; W2 router +
`#[Route]` attributes + middleware; W3 `serve.rs`; W4 `phg serve` + graceful shutdown. The CLI
binary was renamed `phorj` → `phg` (`70ea75d`) during this arc.
Gotchas: `package Main` functions become global PHP functions (builtin-name collisions); PHP
enforces `private` where Phorj backends didn't (externally-read promoted fields needed `public`).

## M-RT — Rich types (closed 2026-06-23)
The TypeScript-grade type system mapped to PHP 8.0/8.1, slice by slice: S1 `instanceof`
(`Op::IsInstance`); S2 interfaces/nominal subtyping (`class_implements` table shared by all
backends); S3 `Map<K,V>` (`Op::MakeMap`, polymorphic `Op::Index`); S7a/S7b erased generics + the
generic-typed native path + `Set<T>` + higher-order natives (re-entrant VM `run_until` — fault
parity extended to control flow); GENERICS-ALL (methods, cross-package types via
`import type` — E-PKG-TYPE lifted — and generic classes `Box<T>`, reified-in-checker /
erased-in-backend); S4 unions + match-over-union (`Pattern::Type`); S5 intersections (≤1 concrete
class, require-agreement signatures); the totality cluster (`E-MISSING-RETURN`, `never`,
`W-UNREACHABLE`, `W-MATCH-UNREACHABLE`) closing the #1 soundness leak; generic enums
(`Option<T>`/`Result<T,E>`); method overloading; S6 inheritance (final-by-default, abstract);
S8 traits. Same-head generic-type invariance was fixed later (Soundness Batch B).

## Cross-cutting audits & clusters (2026-06-21 → 23)
Roadmap-completeness audit (41 agents, 555 candidates → SSOT
`docs/specs/2026-06-21-php-parity-and-beyond.md`); error model slice 2 (`throws`/`Result`/faults);
pattern cluster (match guards, struct destructuring, flow-narrowing) + primitives sweep (number
literal bases, bitwise ops); M-Decomp (whale files → cohesion `mod/` clusters, byte-identity-gated).

## Later milestones (2026-06-24 → 07-01) — fill from docs/MILESTONES.md + CHANGELOG.md
Placeholders (each existed after the old CLAUDE.md log stopped; compress the same way):
M-NUM (decimal) · syntax reshape / `var` retirement · mutation milestone (COW containers) ·
M4 stdlib breadth + native module waves · class entry points · M-Test (`phg test`) ·
`phg format` · Core.Regex/Crypto (first vetted deps) · M-TIME · M8.5 interop/`.d.phg` · LSP +
editor extensions · M-perf (FNV, slot-indexed layout, inline caches) · super/parent ·
green-threads cooperative cutover · M-DOGFOOD (O(n²)→O(1) index-assign) · naming overhaul
(`fn`→`function`, `Console`→`Output`, CLI verbs `benchmark`/`format`/`disassemble`/`tokenize`) ·
M-DX (diagnostics, build profiles, `--dump-on-fault`, assertions, debugger + DAP).

