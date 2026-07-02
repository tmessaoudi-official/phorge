# Agent Q — CLAUDE.md full rewrite draft (rules-only)

> Deliverable of the full-audit fleet, Agent Q. Three artifacts: (1) the new rules-only
> CLAUDE.md, ready to paste; (2) the relocation map accounting for every section of the current
> 580-line CLAUDE.md; (3) the `docs/HISTORY.md` skeleton preserving the chronological narrative.
> Drift fixes are marked `[drift-fixed: old → new]` and were verified against the working tree
> (src/main.rs verb list, Cargo.toml, examples/guide/*.phg, scripts/) on 2026-07-02.

---

## ARTIFACT 1 — new CLAUDE.md (complete, ready to paste)

```markdown
# CLAUDE.md — phorj

> This file holds the RULES for how Claude delivers code here — quality, carefulness, gates.
> The language itself (surface, roadmap, milestones, decisions, history) lives in the docs
> files under "Where things live". Boundary test before adding anything: *does Claude need
> this to deliver correct code?* If not, it belongs in docs, not here.

Phorj is a statically-typed, PHP-inspired language implemented in Rust (edition 2021; core is
std-only with four vetted, feature-gated exceptions — `argon2`, `regex`, `ctrlc`, `corosensei` —
per `docs/specs/2026-06-27-dependency-policy.md`): lexer → parser → type-checker → tree-walking
interpreter (the reference oracle) + bytecode compiler/stack VM + Phorj→PHP transpiler, plus a
PHP→Phorj lifter, LSP, formatter, test runner, and debugger. Single developer, commits direct to
`master`, remote is GitHub (`tmessaoudi-official/phorj`). The binary is `phg`; sources are `.phg`.

## Routing

This sub-project is handled with the global reasoning framework (`~/.claude/CLAUDE.md`). It is
NOT `/stack` infrastructure — never route work here to `global-stack-lead-dev`. The parent
`/stack/CLAUDE.md` is excluded via `/stack/projects/.claude/settings.json` `claudeMdExcludes`.

## Toolchain & quality gate

- `export PATH=/stack/tools/cargo/bin:$PATH`.
- **Green means ALL of:** `cargo test --workspace` + `cargo clippy --all-targets` +
  `cargo fmt --check` + `cargo build --release`, clean. Warnings fail the build
  (`[lints] warnings = "deny"`); `#![forbid(unsafe_code)]` on both crate roots; toolchain pinned
  by `rust-toolchain.toml`. Tracked pre-commit hook: `scripts/git-hooks/pre-commit`.
- **Full correctness gate** (before claiming any feature done, and always before a push):
  `PHORJ_PHP=/stack/tools/phpbrew/php/php-8.5.7/bin/php PHORJ_REQUIRE_PHP=1 cargo test --workspace`
  — with `PHORJ_REQUIRE_PHP=1` a missing `php` FAILS the oracle (never skips). Transpile floor =
  **PHP 8.5**; the bare `php` on PATH is 8.6-dev and too permissive — never gate against it
  (CI runs it only as a non-gating canary).
- **Perf:** `phg benchmark <file>` (median-of-N, output-identity gated) for before/after numbers;
  CI regression gate: `scripts/perf-gate.sh`.
- **After each shipped feature:** `cargo build --release` and report the binary path
  (`target/release/phg`) — standing developer rule.

## Git autonomy (overrides global Rule 10 — authorized by the developer, 2026-06-16)

Autonomous `git add` + `git commit` are **authorized**: stage and commit ready work without
asking, when the quality gate above is green. Limits:
- **Authorized:** `git add`, `git commit` — descriptive messages, `feat:`/`fix:`/`docs:`/`test:`
  prefixes matching history; no `Co-Authored-By` line.
- **NOT authorized without an explicit request:** `git push` (force-push stays denied globally).
- Commit only green, self-contained changes — never a broken build or red tests.
- If the safety classifier blocks a `git commit`, present the exact command for manual execution;
  do not retry or bypass.

## Delivery invariants (the rules — details in `docs/INVARIANTS.md`)

1. **Byte-identity spine.** `phg run` ≡ `phg runvm` ≡ transpiled PHP under a real `php` —
   identical stdout AND identical failure behaviour, for every program and every example.
   Enforced by `tests/differential.rs` (globs `examples/**/*.phg`, project-aware). Nothing is
   "done" until the full correctness gate above has run green.
2. **The interpreter is the reference oracle.** When backends disagree, the interpreter is right
   by definition; validate the VM against it, never the reverse.
3. **A new `Op` variant extends three exhaustive matches in the same commit:** `vm::exec_op`
   (`src/vm/exec.rs`), `BytecodeProgram::validate` (`src/chunk.rs`), `compiler::stack_effect`
   (`src/compiler/mod.rs`). All three are wildcard-free — never reintroduce a `_` arm.
4. **Value kernels are single-sourced** in `src/value.rs` (checked int/float arithmetic,
   `compare_ord`, canonical fault strings). Never re-inline them in a backend; fault bodies are
   parity-affecting.
5. **Compile-time-only sugar is expanded OUT of the AST before any backend** (type aliases,
   generics erasure, html — all via the single `cli::check_and_expand` chokepoint). New sugar
   follows the same discipline: backends and the PHP output must never see it.
6. **Reified operands thread ALL vm-compile paths.** Anything that compiles for the VM
   (playground runvm, `disassemble`, `benchmark`, …) must go through
   `check_and_expand_reified` + `compile_with`, never plain `compile` — a miss hides run≠runvm
   off the differential's CLI path.
7. **CTy-operand trap (MUST-CHECK).** Un-rejecting an expression form, or adding one whose result
   can be an arithmetic operand, requires the compiler's `CTy` resolver to type it — and a
   differential case shaped `expr + 1`. Otherwise the VM rejects what the interpreter accepts.
8. **Mid-expression scratch slots (MUST-CHECK).** Ops that stash a receiver (`??`/`?.`/`!`-unwrap
   family) must use `self.height - 1`, not `locals.len() - 1`; any new such construct needs a
   differential case with TWO of them in one expression.
9. **Examples ship with features** (developer rule, definition-of-done): every shipped feature
   lands, in the same change, a runnable example under `examples/` (auto-gated by the
   differential glob) + an `examples/README.md` entry. CLI/tooling features get a walkthrough
   README + a small companion `.phg`. Faults can't be runnable examples — capture them in a
   README instead.
10. **Determinism.** `run`/`check`/`transpile` never touch the network (`phg vendor` is the only
    network command); examples use only deterministic inputs; any user-facing list derived from
    `HashMap`/`HashSet` iteration is sorted before rendering.
11. **No perf change without a measured before/after** from `phg benchmark` (and no perf claim
    above [Inferred] without one).
12. **Naming in code Claude writes:** packages/types/type-params PascalCase (`package Main;`,
    `Core.` reserved); functions/natives camelCase (`Output.printLine`); keyword `function`
    (never `fn`), return types `: T`, mandatory `new` for construction, explicit `this.field`.
    The naming SSOT is `docs/specs/2026-06-30-naming-overhaul-design.md`.
13. **File-size anti-regrowth** *(pending ratification)*: soft cap 800 lines / hard cap 1000 per
    source file; past the cap, split by cohesion into `foo/mod.rs` + sub-files (M-Decomp
    pattern, `pub(super)` for moved methods) — never by line count alone.

## Where things live (pointers — read these instead of duplicating them here)

- **Correctness invariants (detail):** `docs/INVARIANTS.md` — read before touching backends,
  value kernels, or the `Op` set.
- **Architecture / module map:** `docs/ARCHITECTURE.md`.
- **Language surface:** `FEATURES.md` + `examples/README.md` (living showcase);
  frozen designs in `docs/specs/`.
- **Roadmap:** `ROADMAP.md`; master triage SSOT `docs/specs/2026-06-21-php-parity-and-beyond.md`.
- **Milestone status:** `docs/MILESTONES.md`; per-change detail in `CHANGELOG.md`.
- **Decisions:** `docs/adr/` (canonical verdicts) + `## Decisions Log` sections in
  `docs/plans/*.plan.md` and `docs/specs/*` (exploration).
- **History (chronological narrative):** `docs/HISTORY.md`.
- **Known limitations / deferred work:** `KNOWN_ISSUES.md`.
- **Session-level gotchas:** auto-memory index (`MEMORY.md` in the project memory dir).
```

*(Artifact 1 is 118 lines.)*

---

## ARTIFACT 2 — relocation map (every section of the current CLAUDE.md accounted for)

Line numbers refer to the current `/stack/projects/phorj/CLAUDE.md` (580 lines).

| # | Old section (lines) | Disposition |
|---|---|---|
| 1 | Title + identity paragraph (1–6) | **keep-in-new** → identity paragraph. [drift-fixed: "std-only, no external crates" → "std-only core with four vetted feature-gated deps (argon2, regex, ctrlc, corosensei)", verified in Cargo.toml; pipeline list extended to lifter/LSP/format/test/debug per src/main.rs verb list] |
| 2 | Routing paragraph (8–11) | **keep-in-new** → Routing (verbatim). |
| 3 | Git autonomy (13–27) | **keep-in-new** → Git autonomy (condensed, same rules). |
| 4 | Toolchain: PATH + "~453 tests" baseline (31) | **keep-in-new** → Toolchain. [drift-fixed: pinned "~453 tests" count removed — count is long stale (600+ lib by totality); the rule is "suite green", not a number] |
| 5 | Differential spine + PHP oracle + 8.5 floor + 8.6-dev warning (32–39) | **keep-in-new** → gate command + Rule 1. |
| 6 | Op three-match coupling (39–41) | **keep-in-new** → Rule 3 (detail already owned by INVARIANTS §5 — new file states the rule, points for the why). |
| 7 | `phg bench` before/after (41–42) | **keep-in-new** → Rule 11 + Toolchain perf bullet. [drift-fixed: `phg bench` → `phg benchmark`, verified src/main.rs] |
| 8 | "Examples ship with features" (44–50) | **keep-in-new** → Rule 9 (condensed; same definition-of-done). |
| 9 | "## Active plan" header (52) | **obsolete-delete** — the "active plan" concept moves to `docs/plans/` + memory; a static file section is structurally always stale. |
| 10 | M2 P3.5 / P4 / Wave 4 (54–66) | **move-to-docs/HISTORY.md** (M2 entry). Status already owned by docs/MILESTONES.md. The CTy-operand insight graduates to new Rule 7. |
| 11 | M2 P5a Rc-shared heap + no-GC rationale (68–77) | **move-to-docs/HISTORY.md** (M2 entry). Rc/no-GC rationale already owned by ROADMAP.md M2 + mutation-milestone memory. |
| 12 | M2 closed + examples coverage set + zero-payload gotcha (79–87) | **move-to-docs/HISTORY.md**. Gotcha NOT carried verbatim — [drift-fixed: "construct with call form `V()`" is superseded; construction now uses `new V()` (mandatory-new), match still uses `V()`; per memory `zero-payload-variant-call-form`, verified 2026-07-01]. Current form belongs to KNOWN_ISSUES/memory, not HISTORY. |
| 13 | M2.5 build Phases 1–2 (89–101) | **move-to-docs/HISTORY.md** (M2.5 entry, incl. PE objcopy gotcha compressed). |
| 14 | CLI UX v0.4.0 (103–105) | **move-to-docs/HISTORY.md**. Current CLI surface is owned by README + `phg --help` + ARCHITECTURE. |
| 15 | Profiling & introspection v0.4.0 (107–114) | **move-to-docs/HISTORY.md**. [drift-fixed: `phg disasm` → `disassemble`; bench memory-sampler detail already owned by ARCHITECTURE (mem.rs row) + memory] |
| 16 | OSS doc set at v0.4.0 (116–118) | **move-to-docs/HISTORY.md** (one line). |
| 17 | M3 active + S0/S1/S2 (120–153) | **move-to-docs/HISTORY.md** (M3 entry). Scratch-slot gotcha graduates to new Rule 8 (also in memory `null-op-scratch-slot`). Transpile contract ("Phorj:PHP :: TS:JS") already owned by specs + VISION. |
| 18 | Post-S2 direction / D→B→A / URL-deferred (155–161) | **move-to-docs/HISTORY.md** (one line); decisions already owned by the design spec + plan Decisions Logs. |
| 19 | Namespace reshape design (163–172) | **already-owned-by-docs — delete** (spec `2026-06-18-m3-namespace-system-design.md` + INVARIANTS §12 own it). HISTORY gets a one-liner. [drift-fixed: `Console.println` → `Output.printLine` wherever carried] |
| 20 | Track B Wave 1 (174–186) | **move-to-docs/HISTORY.md** (compressed; E-SHADOW-IMPORT mechanics owned by checker + KNOWN_ISSUES). |
| 21 | Track B Wave 2 (188–201) | **move-to-docs/HISTORY.md**. Float-display divergence already owned by KNOWN_ISSUES. |
| 22 | M5 S1–S2d project model (203–242) | **move-to-docs/HISTORY.md** (M5 entry). Model itself owned by `2026-06-18-m5-project-model-design.md` + MILESTONES. |
| 23 | M5 S3 vendoring + lockfile (244–265) | **move-to-docs/HISTORY.md** (M5 entry). Determinism/offline rule graduates to new Rule 10. |
| 24 | M6 design lock + W0 + W1 + phg rename (267–293) | **move-to-docs/HISTORY.md** (M6 entry). Transpile gotchas already owned by KNOWN_ISSUES; binary rename owned by memory `binary-renamed-to-phg`. [drift-fixed: "W2 is next" — W2/W3/W4 have since shipped per memory] |
| 25 | M3 S3 lambdas + pipe (295–315) | **move-to-docs/HISTORY.md**. [drift-fixed: `fn(int x) => e` → `function(int x) => e` and `-> int` → `: int`, verified examples/guide/lambdas-pipe.phg] |
| 26 | M-RT intro + S1 + S2 + S3 (317–347) | **move-to-docs/HISTORY.md** (M-RT entry). Slice statuses owned by MILESTONES. |
| 27 | S7a erased generics (349–363) | **move-to-docs/HISTORY.md**. Erasure discipline graduates to new Rule 5. |
| 28 | Stdlib PascalCase rename + broader reshape (365–368) | **move-to-docs/HISTORY.md** (one line). [drift-fixed: `Core.Console` was later renamed again → `Core.Output` (naming overhaul); HISTORY records both hops] |
| 29 | Developer decisions post-S7a (370–373) | **move-to-master-plan / decision-register — delete** (already recorded in the M-RT plan Decisions Log; sequence long executed). |
| 30 | S7b-1/2/3 (375–393) | **move-to-docs/HISTORY.md**. Higher-order-native recipe owned by memory `higher-order-natives-reentrant-vm`. |
| 31 | GENERICS-ALL sub-slices 1–3 (395–437) | **move-to-docs/HISTORY.md**. The erased-operand limitation (`id(7)+1` VM-rejected) already owned by KNOWN_ISSUES; feeds Rule 7's rationale. |
| 32 | M-RT S4 unions (439–459) | **move-to-docs/HISTORY.md**. Deferred list owned by KNOWN_ISSUES. |
| 33 | M-RT S5 intersections (461–482) | **move-to-docs/HISTORY.md**. D1/D2 decisions owned by the S5 design spec. |
| 34 | Totality cluster (484–501) | **move-to-docs/HISTORY.md**. |
| 35 | Generic enums (503–519) | **move-to-docs/HISTORY.md**. The invariance fix note already owned by CHANGELOG + m-rt-progress memory. |
| 36 | M-RT closed + error model + M-Decomp (521–526) | **move-to-docs/HISTORY.md**; statuses owned by MILESTONES. M-Decomp cohesion pattern feeds new Rule 13. |
| 37 | Pattern cluster + primitives (528–542) | **move-to-docs/HISTORY.md**. Deferred narrowing list owned by KNOWN_ISSUES. |
| 38 | Roadmap-completeness audit (544–564) | **already-owned-by-docs — delete** (SSOT `docs/specs/2026-06-21-php-parity-and-beyond.md` + plan Decisions Log + ROADMAP/MILESTONES write-back, all named in the old text itself). HISTORY gets a one-liner. |
| 39 | Selective type import "ADOPTED, NOT impl" (566–573) | **obsolete-delete** — self-contradicted by lines 415–416 of the same file ("selective type import is now implemented", GENERICS-ALL sub-slice 2). [drift-fixed: status → implemented] Design owned by its spec; HISTORY one-liner. |
| 40 | Locked-decisions pointer + m-rt memory link (575–576) | **keep-in-new** → generalized into the "Decisions" pointer row. |
| 41 | INVARIANTS/ARCHITECTURE/CHANGELOG pointer (578–580) | **keep-in-new** → "Where things live" block (expanded to full pointer set). |

Nothing else exists in the old file; all 41 sections/paragraphs are accounted for.

---

## ARTIFACT 3 — docs/HISTORY.md skeleton

```markdown
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
```

*(End of HISTORY skeleton.)*
