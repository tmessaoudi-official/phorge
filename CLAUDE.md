# CLAUDE.md — phorge

Phorge is a statically-typed, PHP-inspired language implemented in Rust (edition 2021, std-only,
no external crates): lexer → parser → type-checker → tree-walking interpreter + Phorge→PHP
transpiler (M1) + bytecode compiler + stack VM (M2). Single developer, commits direct to `master`,
remote is GitHub (`tmessaoudi-official/phorge`).

This sub-project lives under `/stack/projects/` and is handled with the global reasoning framework
(`~/.claude/CLAUDE.md`). It is NOT `/stack` infrastructure — do not route work here to
`global-stack-lead-dev`. The parent `/stack/CLAUDE.md` is excluded via
`/stack/projects/.claude/settings.json` `claudeMdExcludes`.

## Git autonomy (overrides global Rule 10 — authorized by the developer, 2026-06-16)

Autonomous `git add` and `git commit` are **authorized** in this project: stage and commit ready
work without asking, when tests pass (`cargo test`) and the quality gate is clean
(`cargo clippy --all-targets`, `cargo fmt --check`). This mirrors the `/stack` auto-commit
precedent and overrides global Rule 10 **for this project only**.

Scope and limits:
- **Authorized:** `git add`, `git commit` (descriptive messages — `feat:`/`fix:`/`docs:`/`test:`
  prefixes, matching existing history; no `Co-Authored-By` line).
- **NOT authorized without an explicit request:** `git push` (and any force-push — `push --force`
  remains denied globally).
- Commit only green, self-contained changes. Do not commit a broken build or red tests.
- If the safety classifier blocks a specific `git commit`, present the exact command for manual
  execution rather than retrying — do not attempt to bypass it.

## Toolchain & gate

`export PATH=/stack/tools/cargo/bin:$PATH`. Baseline: 332 tests green, clippy clean (pedantic off).
The differential harness (`tests/differential.rs`) is the correctness spine — `run` and `runvm`
must stay byte-identical. Adding an `Op` variant requires extending three exhaustive matches in
the same commit: `src/vm.rs` `exec_op`, `src/chunk.rs` `BytecodeProgram::validate`, and
`src/compiler.rs` `stack_effect`. `phorge bench <file>` measures the two backends (median-of-N,
output-identity gated) — run it for a before/after number before any perf change.

**Examples ship with features** (developer rule, 2026-06-17): every shipped feature lands with a
runnable example under `examples/` (auto-gated by the `tests/differential.rs` glob, so it must run
byte-identically on both backends) and an `examples/README.md` entry (index + coverage matrix), in
the **same change** as the feature. CLI/tooling features that aren't a single program (e.g.
`phorge build`, `explain`) get a walkthrough README + a small companion `.phg` (see `examples/build/`,
`examples/cli/`). Faults can't be a runnable example (every example must produce identical *Ok*
output) — capture them in a README instead.

## Active plan

The M2 P3.5 hardening roadmap (`docs/plans/2026-06-16-m2-p3.5-hardening-roadmap.md`, Waves 0–4) is
**complete**. **M2 P4 is COMPLETE** (`docs/plans/2026-06-16-m2-p4-classes-enums-match.md`): P4a
(enums + `match`), P4b (classes + constructor promotion + field reads), and P4c (methods + `this`)
all landed — **`runvm` now covers the full M1 language surface** and `examples/grades.phg` runs
byte-identically on both backends (VM ≈3.2×). The VM object model is value-native (reuses the shared
`Value::Enum`/`Instance`). **M2 Wave 4 is COMPLETE**
(`docs/plans/2026-06-16-m2-wave4-compiler-types.md`): the compiler's operand-type inference is now
class-aware (`enum CTy { Int, Float, Class(String), Other }` + a recursive `ctype(&Expr)` resolver),
so a field read on an arbitrary instance (`p.x + 1`), a method-call result (`c.get() + 1`), a nested
`a.inner.x`, and a class-typed enum payload all compile and run byte-identically — closing the last
known `run`↔`runvm` parity gaps. (M3 S1.1 later extended `CTy` with a `List(elem)` variant so a
list-element read `xs[i]` resolves as an arithmetic operand too — indexing is now part of the surface,
no longer rejected.)

**M2 P5a is COMPLETE** (`docs/specs/2026-06-16-m2-p5-object-model-design.md`,
`docs/plans/2026-06-16-m2-p5a-rc-shared-heap.md`): heap objects are now **`Rc`-shared**
(`Value::Instance`/`Enum`/`List`), so the `Op::GetLocal` hot path is a refcount bump, not a deep
clone — object-heavy VM run **1537 ms → 634 ms (2.4×)**, recovering the VM's advantage to 9.35× (≈
scalar's 10.92×). **There is no tracing GC and none is planned for M2:** the M1 heap is immutable +
acyclic, so `Rc`/`Drop` reclaims completely (a tracing collector is deferred to **M3**, when
mutation could create cycles). **Phase B** (slot-indexed `Vec` field layout, replacing the
per-instance `HashMap`) is **bench-gated and unopened** — after P5a the object path is within ~15% of
scalar's advantage, so field access no longer dominates; the slab-arena was rejected (no locality
evidence).

**M2 is now formally CLOSED** (`docs/MILESTONES.md`, `33c6b78`): all design §10 success criteria met
(backends byte-identical, quality gate green; the mark-sweep GC criterion was revised — `Rc`/`Drop`
reclaims the immutable+acyclic heap fully, tracing GC deferred to M3). A **full-coverage example set**
also landed (`docs/specs/2026-06-16-examples-coverage-design.md`): four real-world programs
(`examples/realworld/`), six focused guide programs (`examples/guide/`), and the Phorge→PHP transpile
bridge (`examples/transpile/`) — `tests/differential.rs` now **globs `examples/**/*.phg`** so every
example (and any added later) is byte-identity-gated automatically; `examples/README.md` is the
living surface showcase. **Gotcha:** zero-payload enum variants need call form `V()` both to
construct AND in a `match` pattern (bare `V =>` is a silent catch-all binding).

**M2.5 `phorge build` (standalone executables) — Phases 1 & 2 COMPLETE** (released as **v0.4.0**).
Phase 1 (host `x86_64-linux-gnu`): `phorge build foo.phg` embeds the program **source** in a `.phorge`
section (versioned CRC-guarded container + hand-rolled ELF64 reader); `main()` self-detects + runs it
on the VM. Phase 2 (`docs/plans/2026-06-16-m2.5-phase2-cross-os.md`): `src/bundle.rs` split into a
`bundle/` module — `container`, per-format readers `elf`/`pe`/`macho` (thin + fat), a magic-sniffing
`section::find_section`, and a `cross` orchestrator — plus `phorge build --target/--all` cross-compiling
stubs via **cargo-zigbuild** (zig linker) for Linux `x86_64-musl`/`aarch64-{gnu,musl}` +
`x86_64-pc-windows-gnu`, cached under an **FNV-1a-64 of the phorge binary's bytes**. All readers honor
EV-7 (checked arithmetic, `None` on bad input). macOS reader ships + fixture-tested; apple `--target`
is **rejected** (Mac stub deferred to Phase 3). `tests/build.rs` gates cross-parity (musl native exec +
real windows-PE round-trip). **Hard-won gotcha (verified):** `llvm-objcopy --add-section` on **PE**
needs `--set-section-flags …=noload,readonly` or it writes a zero-data section — applied unconditionally
for ELF + PE (a prior "skip on PE" attempt was the bug; only the real-binary windows test caught it).

**CLI UX (v0.4.0):** global `-v`/`--version`, `-h`/`--help`; run-family source forms `<file>` | `-`
(stdin) | `-e`/`--eval <code>` (inline) | `--` (literal path). `cli::resolve_source` is the pure,
tested resolver; built binaries still ignore argv (run their embedded program).

**Profiling & introspection (v0.4.0):** `phorge bench` now reports **memory** (cold-execution
peak-RSS growth + process `VmHWM`/`VmRSS`) beside its timing, via a std-only **Linux** `/proc`
sampler (`src/mem.rs` — `/proc/self/status` + `clear_refs`=5 peak reset; non-Linux prints
"unavailable"). Per-phase/sequential per-backend RSS is *deliberately not* reported — it reads ~0
after the 101× timing loop warms the allocator (glibc rarely returns freed pages). `phorge disasm
<source>` dumps the compiled bytecode (per-function listings via `Op` `Debug` + a `_`-fall-through
annotator, so no second match surface to drift; plus enum/class/method descriptor tables).
Showcase: `examples/bench/workload.phg` (+ its README), auto byte-identity-gated like every example.

**Docs:** a full OSS doc set landed at v0.4.0 (README rewrite, dual **MIT OR Apache-2.0**, CONTRIBUTING,
CODE_OF_CONDUCT, SECURITY, ROADMAP, VISION, FEATURES, KNOWN_ISSUES, THIRD-PARTY-NOTICES, CITATION.cff,
`.github/` templates). See **`ROADMAP.md`** / **`VISION.md`** for the forward plan.

**M3 is now the active milestone** (`docs/specs/2026-06-17-m3-language-roadmap-design.md` +
`docs/specs/2026-06-17-m3-slice1-s0-s1-s2-design.md`). The transpile contract is **Phorge : PHP ::
TypeScript : JavaScript** — every feature maps to idiomatic PHP; PHP-absent features (generics) are
compile-time-only and erased. **Slice S0 (developer experience) is COMPLETE**
(`docs/plans/2026-06-17-m3-s0-dx.md`): per-command `--help` with worked examples; `var` local type
inference (`Type::Infer`, resolved in the checker; the VM derives the local's operand `CTy` from the
initializer so arithmetic still specializes); `type` aliases (`Item::TypeAlias`, resolved +
cycle/duplicate/built-in-shadow-checked in the checker, then expanded out of the AST by
`checker::expand_aliases` so the interpreter/VM/transpiler — and the PHP output — are alias-free);
sharper diagnostics (caret-underlined span + did-you-mean hints + stable codes, `Diagnostic`
construction centralized through `Diagnostic::new`, front-end-only so runtime parity is untouched); and
`phorge explain <CODE>`. **Slice S1 (core ergonomics) is COMPLETE**
(`docs/plans/2026-06-17-m3-s1-ergonomics.md`): list indexing `xs[i]` (un-rejected in both backends —
the checker already typed it — reusing the bounds-checked `Op::Index`; OOB → byte-identical
`"list index out of range"` fault, classified `FaultKind::IndexOob` in the differential harness);
integer ranges `a..b`/`a..=b` (the one new `Op::MakeRange(bool)`, extending the three coupled matches;
both backends materialize a `List<int>` via native Rust ranges, so `for (int i in 0..n)` works
unchanged; transpiles to PHP `range()`); and expression `if` (`if (c) { e } else { e }` in value
position — parens + mandatory `else`, single-expression arms; lowers via the existing branch ops like
`&&`/`||`, transpiles to a PHP ternary). All three are byte-identical on `run`/`runvm` **and**
round-tripped through real PHP; `examples/guide/ergonomics.phg` showcases them. **Slice S2
(null-safety) is COMPLETE** (`docs/plans/2026-06-17-m3-s2-null-safety.md`): optionals `T?`
(`Ty::Optional` + `Value::Null`) with a compile-time non-null guarantee (a non-optional `T` is never
null — TypeScript `strictNullChecks` over PHP's nullable runtime); `??` null-coalesce; `?.` safe
access (PHP `?->`); `if (var x = opt)` if-let binding + smart-cast (S1.4 landed here); `opt!` checked
force-unwrap (clean `force-unwrap of null` fault, `FaultKind::ForceUnwrap` parity) with the
**`W-FORCE-UNWRAP`** lint; and `match` over `T?` with null-arm narrowing. Two cross-cutting additions:
the **warning channel** (first non-fatal lint — `check()` returns `Ok(warnings)`, rendered to stderr,
never gating the build) and the generalization of `Op::MatchFail` → **`Op::Fault(FaultMsg)`** (so S2
adds **no new `Op` variant**). All byte-identical on `run`/`runvm` + round-tripped through real PHP;
`examples/guide/null-safety.phg` showcases the suite. **Gotcha (fixed this slice):** `??`/`?.`/`opt!`
stash their receiver in a scratch slot that must be `self.height - 1` (the receiver's frame slot), not
`add_local()`'s `locals.len()-1` — two such ops in one expression (e.g. `"{a ?? -1} {b ?? -1}"`) put a
live transient below the receiver and the old slot was off, a silent `run`↔`runvm` break.

**Post-S2 direction (designed 2026-06-18, `docs/specs/2026-06-18-m3-next-intuitive-features-and-io-design.md`):**
developer asked for more intuitive features + exhaustive examples (file/URL/imports) + a Phorge-vs-PHP
benchmark. Locked: **build order D→B→A**; **URL/network deferred to M6** (Rust std has no HTTP client →
breaks zero-dep, *and* network is non-deterministic → breaks the byte-identical spine; determinism, not
the dependency, gates examples); **rich std-only stdlib now**; multiple inheritance = traits/mixins at
S5 (rejected as MI, D-L3). **Track D DONE** — `phorge bench --vs-php` (3-way interpreter/VM/PHP, VM ≈3.2×
faster than a debug PHP 8.6 on the workload). **ACTIVE: Track B** (std-only stdlib + I/O + real
`import std.*`) — plan `docs/plans/2026-06-18-trackB-stdlib-io-imports.md`. **Resume at Task 1: the
`NativeModule` foundation** (registry + dual+ registration; `Op::Print`→`Op::CallNative`; migrate
`println`). Then Track A (S3 lambdas/pipeline). **Parked:** M2.5 Phase 3 (CI stub registry + `--sign`) —
`docs/specs/2026-06-17-m2.5-phase3a-stub-registry-design.md`.

Project invariants and layout now live in-repo: **`docs/INVARIANTS.md`** (the load-bearing
correctness rules — read before touching backends, value kernels, or the `Op` set) and
**`docs/ARCHITECTURE.md`** (pipeline + module map). `CHANGELOG.md` tracks milestone progress.
